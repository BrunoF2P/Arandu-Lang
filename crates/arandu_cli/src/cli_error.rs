use std::path::PathBuf;

#[derive(Debug)]
pub enum CliSuccess {
    Done,
    ProgramExit(i32),
}

impl CliSuccess {
    #[must_use]
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::Done => 0,
            Self::ProgramExit(code) => code,
        }
    }
}

#[derive(Debug)]
pub enum CliFailure {
    Usage {
        message: String,
    },
    Operational {
        operation: &'static str,
        context: Option<PathBuf>,
        source: String,
    },
    Diagnostics {
        diagnostics: Vec<arandu_middle::Diagnostic>,
        source_path: Option<PathBuf>,
    },
}

impl CliFailure {
    #[must_use]
    pub const fn exit_code(&self) -> i32 {
        match self {
            Self::Usage { .. } => 2,
            Self::Operational { .. } | Self::Diagnostics { .. } => 1,
        }
    }

    #[must_use]
    pub fn usage(message: impl Into<String>) -> Self {
        Self::Usage {
            message: message.into(),
        }
    }

    #[must_use]
    pub fn operational(
        operation: &'static str,
        context: Option<PathBuf>,
        source: impl Into<String>,
    ) -> Self {
        Self::Operational {
            operation,
            context,
            source: source.into(),
        }
    }

    #[must_use]
    pub fn diagnostics(
        diagnostics: impl IntoIterator<Item = arandu_middle::Diagnostic>,
        source_path: Option<PathBuf>,
    ) -> Self {
        Self::Diagnostics {
            diagnostics: diagnostics.into_iter().collect(),
            source_path,
        }
    }

    pub fn render(&self) {
        match self {
            Self::Usage { message } => eprintln!("{message}"),
            Self::Operational {
                operation,
                context,
                source,
            } => match context {
                Some(path) => eprintln!("error: {operation} {}: {source}", path.display()),
                None => eprintln!("error: {operation}: {source}"),
            },
            Self::Diagnostics {
                diagnostics,
                source_path,
            } => {
                let path = source_path
                    .as_deref()
                    .unwrap_or_else(|| std::path::Path::new(""));
                let source = std::fs::read_to_string(path).unwrap_or_default();
                let named_source = miette::NamedSource::new(path.to_string_lossy(), source);
                for diagnostic in diagnostics {
                    let report = miette::Report::new(diagnostic.clone())
                        .with_source_code(named_source.clone());
                    eprintln!("{report:?}");
                }
            }
        }
    }
}

pub type CliResult = Result<CliSuccess, CliFailure>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_arandu_failure_classes() {
        assert_eq!(CliSuccess::Done.exit_code(), 0);
        assert_eq!(CliSuccess::ProgramExit(42).exit_code(), 42);
        assert_eq!(CliFailure::usage("bad flag").exit_code(), 2);
        assert_eq!(
            CliFailure::operational("read", None, "missing").exit_code(),
            1
        );
        assert_eq!(CliFailure::diagnostics([], None).exit_code(), 1);
    }
}
