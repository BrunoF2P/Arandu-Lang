//! Source formatting command (`arandu fmt`).

use std::fs;
use std::path::{Path, PathBuf};

use crate::cli_error::{CliFailure, CliResult, CliSuccess};

const IGNORED_DIRECTORY_NAMES: &[&str] = &[
    ".git",
    ".arandu",
    "target",
    "build",
    "dist",
    "node_modules",
    "vendor",
];

pub fn cmd_format_paths(paths: &[PathBuf], check_only: bool, quiet: bool) -> CliResult {
    let mut files = Vec::new();
    for path in paths {
        let metadata = fs::symlink_metadata(path).map_err(|error| {
            CliFailure::operational(
                "resolve formatter input",
                Some(path.clone()),
                error.to_string(),
            )
        })?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            return Err(CliFailure::operational(
                "resolve formatter input",
                Some(path.clone()),
                "symbolic links are not followed by the formatter".to_string(),
            ));
        }
        if file_type.is_dir() {
            collect_aru_files(path, &mut files).map_err(|error| {
                CliFailure::operational(
                    "list source directory",
                    Some(path.clone()),
                    error.to_string(),
                )
            })?;
        } else if file_type.is_file() {
            if !is_aru_file(path) {
                return Err(CliFailure::operational(
                    "resolve formatter input",
                    Some(path.clone()),
                    "expected an Arandu source file with the .aru extension".to_string(),
                ));
            }
            files.push(path.clone());
        } else {
            return Err(CliFailure::operational(
                "resolve formatter input",
                Some(path.clone()),
                "path is not a regular file or directory".to_string(),
            ));
        }
    }
    files.sort();
    files.dedup();
    if files.is_empty() {
        return Err(CliFailure::operational(
            "find Arandu sources",
            None,
            "no .aru source files found".to_string(),
        ));
    }

    // Read and format every input before writing any file, so input/format
    // errors cannot leave a partially formatted batch.
    let mut formatted_files = Vec::with_capacity(files.len());
    for path in &files {
        let source = fs::read_to_string(path).map_err(|error| {
            CliFailure::operational("read source", Some(path.clone()), error.to_string())
        })?;
        if source.len() > arandu_fmt::MAX_FORMAT_SOURCE_BYTES {
            return Err(CliFailure::operational(
                "format source",
                Some(path.clone()),
                format!(
                    "source exceeds the formatter limit of {} bytes",
                    arandu_fmt::MAX_FORMAT_SOURCE_BYTES
                ),
            ));
        }
        let formatted = arandu_fmt::format_source(&source);
        formatted_files.push((path.clone(), source, formatted));
    }

    let changed = formatted_files
        .iter()
        .filter(|(_, source, formatted)| source != formatted)
        .count();
    if check_only {
        if changed > 0 {
            for (path, source, formatted) in &formatted_files {
                if source != formatted {
                    eprintln!("would reformat {}", path.display());
                }
            }
            return Err(CliFailure::operational(
                "check formatting",
                None,
                format!("{changed} of {} file(s) need formatting", files.len()),
            ));
        }
        if !quiet {
            eprintln!("all {} file(s) are formatted", files.len());
        }
        return Ok(CliSuccess::Done);
    }

    for (path, source, formatted) in formatted_files {
        if source != formatted {
            fs::write(&path, formatted).map_err(|error| {
                CliFailure::operational(
                    "write formatted source",
                    Some(path.clone()),
                    error.to_string(),
                )
            })?;
            if !quiet {
                eprintln!("formatted {}", path.display());
            }
        }
    }
    if changed == 0 && !quiet {
        eprintln!("already formatted ({} file(s))", files.len());
    }
    Ok(CliSuccess::Done)
}

fn is_aru_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "aru")
}

fn collect_aru_files(directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| IGNORED_DIRECTORY_NAMES.contains(&name))
            {
                continue;
            }
            collect_aru_files(&path, files)?;
        } else if file_type.is_file() && is_aru_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}
