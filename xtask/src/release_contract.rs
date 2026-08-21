use std::fs;
use std::path::{Path, PathBuf};

pub fn check(root: &Path, explicit_tag: Option<String>) -> i32 {
    let environment_tag = github_tag();
    match validate(root, explicit_tag.as_deref().or(environment_tag.as_deref())) {
        Ok(summary) => {
            println!(
                "check-release-contract: ok (version={}, {} component manifests{})",
                summary.version,
                summary.components,
                summary
                    .tag
                    .as_deref()
                    .map_or_else(String::new, |tag| format!(", tag={tag}"))
            );
            0
        }
        Err(error) => {
            eprintln!("check-release-contract: error: {error}");
            1
        }
    }
}

struct Summary {
    version: String,
    components: usize,
    tag: Option<String>,
}

fn validate(root: &Path, tag: Option<&str>) -> Result<Summary, String> {
    let canonical_path = root.join("crates/arandu_cli/Cargo.toml");
    let canonical = cargo_package_version(&canonical_path)?;
    validate_version(&canonical)?;

    let mut manifests = Vec::new();
    collect_cargo_manifests(&root.join("crates"), &mut manifests)?;
    manifests.extend([
        root.join("arandu_fuzz/Cargo.toml"),
        root.join("xtask/Cargo.toml"),
    ]);
    manifests.sort();
    manifests.dedup();

    for manifest in &manifests {
        let version = cargo_package_version(manifest)?;
        if version != canonical {
            return Err(format!(
                "{} has version {version}, expected {canonical}",
                display_relative(root, manifest)
            ));
        }
    }

    let extension = root.join("editors/vscode/package.json");
    let extension_version = json_string_field(&extension, "version")?;
    if extension_version != canonical {
        return Err(format!(
            "{} has version {extension_version}, expected {canonical}",
            display_relative(root, &extension)
        ));
    }

    let normalized_tag = tag.map(str::to_owned);
    if let Some(tag) = tag {
        let Some(tag_version) = tag.strip_prefix('v') else {
            return Err(format!("release tag must start with `v`: {tag}"));
        };
        validate_version(tag_version)?;
        if tag_version != canonical {
            return Err(format!(
                "tag {tag} implies version {tag_version}, expected {canonical}"
            ));
        }
    }

    Ok(Summary {
        version: canonical,
        components: manifests.len() + 1,
        tag: normalized_tag,
    })
}

fn github_tag() -> Option<String> {
    if std::env::var("GITHUB_REF_TYPE").as_deref() == Ok("tag") {
        return std::env::var("GITHUB_REF_NAME").ok();
    }
    None
}

fn collect_cargo_manifests(dir: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|error| format!("cannot read {}: {error}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("cannot enumerate {}: {error}", dir.display()))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_cargo_manifests(&path, output)?;
        } else if path.file_name().is_some_and(|name| name == "Cargo.toml") {
            output.push(path);
        }
    }
    Ok(())
}

fn cargo_package_version(path: &Path) -> Result<String, String> {
    let text = read_text(path)?;
    let mut in_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_package = line == "[package]";
            continue;
        }
        if in_package {
            if let Some(value) = line
                .strip_prefix("version")
                .and_then(parse_assignment_string)
            {
                return Ok(value.to_owned());
            }
        }
    }
    Err(format!("missing [package] version in {}", path.display()))
}

fn parse_assignment_string(value: &str) -> Option<&str> {
    let value = value.trim_start().strip_prefix('=')?.trim();
    value.strip_prefix('"')?.strip_suffix('"')
}

fn json_string_field(path: &Path, field: &str) -> Result<String, String> {
    let text = read_text(path)?;
    let prefix = format!("\"{field}\"");
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = line.strip_prefix(&prefix).and_then(parse_json_string_value) {
            return Ok(value.to_owned());
        }
    }
    Err(format!(
        "missing JSON string field {field:?} in {}",
        path.display()
    ))
}

fn parse_json_string_value(value: &str) -> Option<&str> {
    let value = value.trim_start().strip_prefix(':')?.trim();
    value
        .strip_suffix(',')
        .unwrap_or(value)
        .strip_prefix('"')?
        .strip_suffix('"')
}

fn validate_version(version: &str) -> Result<(), String> {
    let (core, prerelease) = version
        .split_once('-')
        .map_or((version, None), |(core, pre)| (core, Some(pre)));
    let parts: Vec<_> = core.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(format!("version is not canonical SemVer: {version}"));
    }
    if prerelease.is_some_and(|pre| {
        pre.is_empty()
            || pre.split('.').any(|identifier| {
                identifier.is_empty()
                    || !identifier
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            })
    }) {
        return Err(format!("invalid SemVer prerelease: {version}"));
    }
    Ok(())
}

fn read_text(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

fn display_relative<'a>(root: &'a Path, path: &'a Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::validate_version;

    #[test]
    fn accepts_release_and_rc_versions() {
        for version in ["0.0.1", "0.1.0-rc.1", "1.2.3-beta.2"] {
            assert!(validate_version(version).is_ok(), "{version}");
        }
    }

    #[test]
    fn rejects_noncanonical_versions() {
        for version in ["v0.1.0", "0.1", "00.1.0", "0.1.0-", "0.1.0-rc!"] {
            assert!(validate_version(version).is_err(), "{version}");
        }
    }
}
