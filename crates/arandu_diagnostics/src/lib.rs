//! Compiler diagnostic types for Arandu.
//!
//! A [`Diagnostic`] represents a single compiler message (error, warning, note,
//! or hint) and carries a machine-readable [`DiagCode`], a human-readable
//! message, a primary [`Span`], optional secondary [`Label`]s, notes, and
//! [`Hint`]s with optional code replacements.
//!
//! # Building a diagnostic
//! ```ignore
//! Diagnostic::error(DiagCode::T001CannotInferType, "cannot infer type", span)
//!     .with_label(extra_span, "declared here")
//!     .with_hint("add an explicit type annotation")
//! ```

pub use arandu_base::source_registry::SourceRegistry;
pub use arandu_base::span::Span;
use std::fmt;

/// Severity level of a compiler diagnostic.
///
/// Controls how the message is displayed and whether compilation is aborted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// A hard error that prevents successful compilation.
    Error,
    /// A potential issue that does not prevent compilation.
    Warning,
    /// Informational context attached to an error or warning.
    Note,
    /// A suggestion for how to fix an issue, optionally with a code replacement.
    Hint,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Error => write!(f, "error"),
            Severity::Warning => write!(f, "warning"),
            Severity::Note => write!(f, "note"),
            Severity::Hint => write!(f, "hint"),
        }
    }
}

/// Whether a diagnostic was produced by the user's code or by an internal compiler bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    /// The diagnostic describes a problem in the user's source code.
    User,
    /// The diagnostic describes an unexpected compiler bug (ICE — Internal Compiler Error).
    InternalCompilerError,
}

/// A suggested text replacement attached to a [`Hint`].
///
/// When present in a hint, editors and CLI can offer to automatically apply
/// the replacement to the source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeReplacement {
    /// The source span to replace.
    pub span: Span,
    /// The text that should replace the spanned region.
    pub new_text: String,
}

/// A human-readable suggestion attached to a [`Diagnostic`].
///
/// Hints optionally carry a [`CodeReplacement`] so that tooling can offer
/// a one-click fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// The hint message shown to the user.
    pub message: String,
    /// An optional automatic code fix for the suggestion.
    pub replacement: Option<CodeReplacement>,
}

impl From<String> for Hint {
    fn from(message: String) -> Self {
        Self {
            message,
            replacement: None,
        }
    }
}

impl From<&str> for Hint {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
            replacement: None,
        }
    }
}

/// Machine-readable diagnostic code.
///
/// Every [`Diagnostic`] carries exactly one `DiagCode` that uniquely identifies
/// the error category. Codes are grouped by compiler phase:
///
/// | Prefix | Phase |
/// |--------|-------|
/// | `LX`   | Lexer |
/// | `P`    | Parser |
/// | `M`    | Module / import resolution |
/// | `N`    | Name resolution / scope |
/// | `T`    | Type checker |
/// | `L`    | Lowering |
/// | `G`    | Generics |
/// | `O`    | Ownership / move checker |
/// | `W`    | Warnings / linting |
/// | `U`    | Unimplemented / future features |
/// | `ICE`  | Internal compiler error |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagCode {
    // ── Lexical Analysis (LX) ──
    LX001UnterminatedString,
    LX002InvalidUnicodeChar,
    LX003InvalidNumericLiteral,

    // ── Parser / Syntax (P) ──
    P001UnexpectedToken,
    P002UnclosedBlock,
    P003InvalidAssignmentOperator,
    P004ExpectedIdentifier,
    P005ExpectedExpression,
    P006MalformedAttribute,

    // ── Modules & Imports (M) ──
    M001UnresolvedImport,
    M002UndefinedNamespaceMember,
    M003NamespaceUsedAsValue,

    // ── Name Resolution / Scope (N) ──
    N001UndefinedValue,
    N002UndefinedType,
    N003RedefinedName,
    N004TypeUsedAsValue,
    N005ValueUsedAsType,
    N007UndefinedAssignmentTarget,
    N010UndefinedAssociatedFunction,
    N011BreakContinueOutsideLoop,

    // ── Type Checker (T) ──
    T001CannotInferType,
    T002IncompatibleAssignment,
    T003IncompatibleCallArg,
    T004IncompatibleReturnType,
    T005OperatorNotApplicable,
    T006NotNullable,
    T007IfBranchMismatch,
    T008MatchArmMismatch,
    T009ConditionNotBool,
    T010InvalidCast,
    T011GenericConstraintNotSatisfied,
    T012WrongArgCount,
    T013UnknownNamedArg,
    T014InvalidVariadicType,
    T015ImplicitWidening,
    T016TryInvalid,
    T017InvalidIndex,
    T018UndefinedField,
    T021MethodSelfRequired,
    T024NonExhaustiveMatch,
    T025InterfaceNotSatisfied,
    T026CannotAssignImmutable,
    T027MissingStructFields,
    T028DuplicateFieldInit,
    T029RecursiveStructInfiniteSize,
    T030DuplicateFieldDecl,
    T031Reserved,
    T032AwaitInvalid,

    // ── Lowering (L) ──
    L001LoweringUnresolvedSymbol,

    // ── Generics (G) ──
    G001GenericInstantiationCycle,
    G002GenericInstantiationLimit,

    // ── Ownership / Memory (O) ──
    O001UseAfterMove,
    O002BorrowAfterMove,
    O003MutableBorrowConflict,
    O004SharedBorrowConflict,
    O005DoubleFree,
    O006DanglingReference,
    O007InconsistentMoveBetweenBranches,
    O008UseBeforeInit,
    O009LifetimeMismatch,
    O010EscapeOfBorrowedValue,
    O011FreeRequiresPtr,
    O012AllocRequiresUnsafe,
    O013ExternRequiresUnsafe,
    O014FreeRequiresUnsafe,

    // ── Warnings & Linting (W) ──
    W001VariableAssignedNotUsed,
    W002DeadCode,
    W003UnreachableCode,
    W004VariableShadowing,
    W005UnnecessaryMutability,
    W006UnhandledResult,
    W007UnusedImport,

    // ── Unimplemented (U) ──
    U001FeatureNotSupported,

    // ── Internal Compiler Errors (ICE) ──
    ICELX001,
    ICEP001,
    ICEN001,
    ICET001,
    ICEO001,
    ICEL001,
    ICEGEN001,
    ICEGEN002,
}

impl DiagCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DiagCode::LX001UnterminatedString => "LX001",
            DiagCode::LX002InvalidUnicodeChar => "LX002",
            DiagCode::LX003InvalidNumericLiteral => "LX003",
            DiagCode::P001UnexpectedToken => "P001",
            DiagCode::P002UnclosedBlock => "P002",
            DiagCode::P003InvalidAssignmentOperator => "P003",
            DiagCode::P004ExpectedIdentifier => "P004",
            DiagCode::P005ExpectedExpression => "P005",
            DiagCode::P006MalformedAttribute => "P006",
            DiagCode::M001UnresolvedImport => "M001",
            DiagCode::M002UndefinedNamespaceMember => "M002",
            DiagCode::M003NamespaceUsedAsValue => "M003",
            DiagCode::N001UndefinedValue => "N001",
            DiagCode::N002UndefinedType => "N002",
            DiagCode::N003RedefinedName => "N003",
            DiagCode::N004TypeUsedAsValue => "N004",
            DiagCode::N005ValueUsedAsType => "N005",
            DiagCode::N007UndefinedAssignmentTarget => "N007",
            DiagCode::N010UndefinedAssociatedFunction => "N010",
            DiagCode::N011BreakContinueOutsideLoop => "N011",
            DiagCode::T001CannotInferType => "T001",
            DiagCode::T002IncompatibleAssignment => "T002",
            DiagCode::T003IncompatibleCallArg => "T003",
            DiagCode::T004IncompatibleReturnType => "T004",
            DiagCode::T005OperatorNotApplicable => "T005",
            DiagCode::T006NotNullable => "T006",
            DiagCode::T007IfBranchMismatch => "T007",
            DiagCode::T008MatchArmMismatch => "T008",
            DiagCode::T009ConditionNotBool => "T009",
            DiagCode::T010InvalidCast => "T010",
            DiagCode::T011GenericConstraintNotSatisfied => "T011",
            DiagCode::T012WrongArgCount => "T012",
            DiagCode::T013UnknownNamedArg => "T013",
            DiagCode::T014InvalidVariadicType => "T014",
            DiagCode::T015ImplicitWidening => "T015",
            DiagCode::T016TryInvalid => "T016",
            DiagCode::T017InvalidIndex => "T017",
            DiagCode::T018UndefinedField => "T018",
            DiagCode::T021MethodSelfRequired => "T021",
            DiagCode::T024NonExhaustiveMatch => "T024",
            DiagCode::T025InterfaceNotSatisfied => "T025",
            DiagCode::T026CannotAssignImmutable => "T026",
            DiagCode::T027MissingStructFields => "T027",
            DiagCode::T028DuplicateFieldInit => "T028",
            DiagCode::T029RecursiveStructInfiniteSize => "T029",
            DiagCode::T030DuplicateFieldDecl => "T030",
            DiagCode::T031Reserved => "T031",
            DiagCode::T032AwaitInvalid => "T032",
            DiagCode::L001LoweringUnresolvedSymbol => "L001",
            DiagCode::G001GenericInstantiationCycle => "G001",
            DiagCode::G002GenericInstantiationLimit => "G002",
            DiagCode::O001UseAfterMove => "O001",
            DiagCode::O002BorrowAfterMove => "O002",
            DiagCode::O003MutableBorrowConflict => "O003",
            DiagCode::O004SharedBorrowConflict => "O004",
            DiagCode::O005DoubleFree => "O005",
            DiagCode::O006DanglingReference => "O006",
            DiagCode::O007InconsistentMoveBetweenBranches => "O007",
            DiagCode::O008UseBeforeInit => "O008",
            DiagCode::O009LifetimeMismatch => "O009",
            DiagCode::O010EscapeOfBorrowedValue => "O010",
            DiagCode::O011FreeRequiresPtr => "O011",
            DiagCode::O012AllocRequiresUnsafe => "O012",
            DiagCode::O013ExternRequiresUnsafe => "O013",
            DiagCode::O014FreeRequiresUnsafe => "O014",
            DiagCode::W001VariableAssignedNotUsed => "W001",
            DiagCode::W002DeadCode => "W002",
            DiagCode::W003UnreachableCode => "W003",
            DiagCode::W004VariableShadowing => "W004",
            DiagCode::W005UnnecessaryMutability => "W005",
            DiagCode::W006UnhandledResult => "W006",
            DiagCode::W007UnusedImport => "W007",
            DiagCode::U001FeatureNotSupported => "U001",
            DiagCode::ICELX001 => "ICE-LX-001",
            DiagCode::ICEP001 => "ICE-P-001",
            DiagCode::ICEN001 => "ICE-N-001",
            DiagCode::ICET001 => "ICE-T-001",
            DiagCode::ICEO001 => "ICE-O-001",
            DiagCode::ICEL001 => "ICE-L-001",
            DiagCode::ICEGEN001 => "ICE-GEN-001",
            DiagCode::ICEGEN002 => "ICE-GEN-002",
        }
    }
}

impl fmt::Display for DiagCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A secondary source location annotation attached to a [`Diagnostic`].
///
/// Labels highlight additional spans of code that are relevant to the main
/// diagnostic message (e.g. "value moved here", "declared here").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// The source span this label points to.
    pub span: Span,
    /// A short message describing why this span is relevant.
    pub message: String,
}

/// A single compiler diagnostic message.
///
/// Diagnostics are the primary output of every compiler pass. They carry a
/// machine-readable [`DiagCode`], a human-readable message, a primary source
/// [`Span`], optional secondary [`Label`]s, free-form notes, and [`Hint`]s
/// with optional code replacements.
///
/// Use the [`Diagnostic::error`], [`Diagnostic::warning`], etc. constructors
/// and chain [`with_label`](Diagnostic::with_label), [`with_note`](Diagnostic::with_note),
/// and [`with_hint`](Diagnostic::with_hint) to build a complete diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Machine-readable error code for tooling and tests.
    pub code: DiagCode,
    /// Display severity (controls abort behaviour and rendering colour).
    pub severity: Severity,
    /// Whether this is a user error or an internal compiler error.
    pub kind: DiagnosticKind,
    /// Primary human-readable message.
    pub message: String,
    /// Primary source location that the message refers to.
    pub span: Span,
    /// Secondary annotated spans for additional context.
    pub labels: Vec<Label>,
    /// Free-form explanatory notes appended after the main message.
    pub notes: Vec<String>,
    /// Actionable hints, optionally with automatic code replacements.
    pub hints: Vec<Hint>,
}

pub mod registry;
pub use arandu_base::index_vec;
pub use arandu_base::stable_id;

impl Diagnostic {
    /// Creates a user-facing error diagnostic.
    pub fn error(code: DiagCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            severity: Severity::Error,
            kind: DiagnosticKind::User,
            message: message.into(),
            span,
            labels: Vec::new(),
            notes: Vec::new(),
            hints: Vec::new(),
        }
    }

    /// Creates a user-facing warning diagnostic.
    pub fn warning(code: DiagCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            severity: Severity::Warning,
            kind: DiagnosticKind::User,
            message: message.into(),
            span,
            labels: Vec::new(),
            notes: Vec::new(),
            hints: Vec::new(),
        }
    }

    /// Creates a user-facing informational note.
    pub fn note(code: DiagCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            severity: Severity::Note,
            kind: DiagnosticKind::User,
            message: message.into(),
            span,
            labels: Vec::new(),
            notes: Vec::new(),
            hints: Vec::new(),
        }
    }

    /// Creates a user-facing hint (suggestion).
    pub fn hint(code: DiagCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            severity: Severity::Hint,
            kind: DiagnosticKind::User,
            message: message.into(),
            span,
            labels: Vec::new(),
            notes: Vec::new(),
            hints: Vec::new(),
        }
    }

    /// Creates an Internal Compiler Error (ICE) diagnostic.
    ///
    /// ICEs indicate a bug in the compiler itself, not in the user's code.
    /// They are always rendered as errors regardless of the provided code's category.
    pub fn ice(code: DiagCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            severity: Severity::Error,
            kind: DiagnosticKind::InternalCompilerError,
            message: message.into(),
            span,
            labels: Vec::new(),
            notes: Vec::new(),
            hints: Vec::new(),
        }
    }

    /// Attaches a secondary source label to this diagnostic.
    #[must_use]
    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
        });
        self
    }

    /// Appends a free-form explanatory note to this diagnostic.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Appends a plain-text hint (no code replacement).
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(Hint {
            message: hint.into(),
            replacement: None,
        });
        self
    }

    /// Appends a hint with an optional automatic code replacement.
    #[must_use]
    pub fn with_hint_replacement(mut self, hint: Hint) -> Self {
        self.hints.push(hint);
        self
    }

    #[must_use]
    pub fn format_for_cli(&self, registry: &SourceRegistry) -> String {
        use std::fmt::Write;
        let mut out = String::new();

        let (filepath, start_line, start_col) =
            if let Some(file) = registry.get_file(self.span.file_id) {
                let (line, col) = file.line_index.line_col(self.span.start);
                (&file.path[..], line, col)
            } else {
                ("", 1, 1)
            };

        let file_prefix = if filepath.is_empty() {
            String::new()
        } else {
            format!("{filepath}:")
        };

        // Format code prefix based on ICE vs regular error
        let code_prefix = self.code.as_str();

        let _ = writeln!(out, "{}: {}", code_prefix, self.message);
        let _ = writeln!(out, "  --> {}{}:{}", file_prefix, start_line, start_col);

        for label in &self.labels {
            let (l_start_line, l_start_col, l_end_line, l_end_col) =
                if let Some(file) = registry.get_file(label.span.file_id) {
                    let (s_line, s_col) = file.line_index.line_col(label.span.start);
                    let (e_line, e_col) = file.line_index.line_col(label.span.end);
                    (s_line, s_col, e_line, e_col)
                } else {
                    (1, 1, 1, 1)
                };
            let _ = writeln!(
                out,
                "  label: {}:{}-{}:{} {}",
                l_start_line, l_start_col, l_end_line, l_end_col, label.message
            );
        }
        for note in &self.notes {
            let _ = writeln!(out, "  note: {note}");
        }
        for hint in &self.hints {
            let _ = writeln!(out, "  hint: {}", hint.message);
            if let Some(ref rep) = hint.replacement {
                let (r_start_line, r_start_col, r_end_line, r_end_col) =
                    if let Some(file) = registry.get_file(rep.span.file_id) {
                        let (s_line, s_col) = file.line_index.line_col(rep.span.start);
                        let (e_line, e_col) = file.line_index.line_col(rep.span.end);
                        (s_line, s_col, e_line, e_col)
                    } else {
                        (1, 1, 1, 1)
                    };
                let _ = writeln!(
                    out,
                    "  replacement: at {}:{}-{}:{} with {:?}",
                    r_start_line, r_start_col, r_end_line, r_end_col, rep.new_text
                );
            }
        }

        // Remove trailing newline
        if out.ends_with('\n') {
            out.pop();
        }
        if out.ends_with('\r') {
            out.pop();
        }

        out
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let registry = SourceRegistry::default();
        f.write_str(&self.format_for_cli(&registry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_span() -> Span {
        Span::new(0, 0, 0)
    }

    fn registry_with(content: &str) -> SourceRegistry {
        let mut reg = SourceRegistry::new();
        reg.register("test.arandu", content);
        reg
    }

    // ── Severity Display ──

    #[test]
    fn severity_display() {
        assert_eq!(Severity::Error.to_string(), "error");
        assert_eq!(Severity::Warning.to_string(), "warning");
        assert_eq!(Severity::Note.to_string(), "note");
        assert_eq!(Severity::Hint.to_string(), "hint");
    }

    // ── DiagCode ──

    #[test]
    fn diag_code_as_str() {
        for (code, expected) in &[
            (DiagCode::LX001UnterminatedString, "LX001"),
            (DiagCode::P001UnexpectedToken, "P001"),
            (DiagCode::M001UnresolvedImport, "M001"),
            (DiagCode::N001UndefinedValue, "N001"),
            (DiagCode::T001CannotInferType, "T001"),
            (DiagCode::L001LoweringUnresolvedSymbol, "L001"),
            (DiagCode::G001GenericInstantiationCycle, "G001"),
            (DiagCode::O001UseAfterMove, "O001"),
            (DiagCode::W001VariableAssignedNotUsed, "W001"),
            (DiagCode::U001FeatureNotSupported, "U001"),
            (DiagCode::ICELX001, "ICE-LX-001"),
        ] {
            assert_eq!(code.as_str(), *expected, "mismatch for {code:?}");
        }
    }

    #[test]
    fn diag_code_display_matches_as_str() {
        let codes = [
            DiagCode::T002IncompatibleAssignment,
            DiagCode::T003IncompatibleCallArg,
            DiagCode::N003RedefinedName,
            DiagCode::ICEP001,
        ];
        for code in &codes {
            assert_eq!(code.to_string(), code.as_str());
        }
    }

    // ── Hint ──

    #[test]
    fn hint_from_string() {
        let h: Hint = "hello".to_string().into();
        assert_eq!(h.message, "hello");
        assert_eq!(h.replacement, None);
    }

    #[test]
    fn hint_from_str() {
        let h: Hint = "hello".into();
        assert_eq!(h.message, "hello");
        assert_eq!(h.replacement, None);
    }

    // ── Diagnostic builder: error ──

    #[test]
    fn diagnostic_error_builder() {
        let d = Diagnostic::error(DiagCode::T001CannotInferType, "oops", dummy_span());
        assert_eq!(d.code, DiagCode::T001CannotInferType);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.kind, DiagnosticKind::User);
        assert_eq!(d.message, "oops");
        assert_eq!(d.span, dummy_span());
    }

    #[test]
    fn diagnostic_warning_builder() {
        let d = Diagnostic::warning(
            DiagCode::W001VariableAssignedNotUsed,
            "unused",
            dummy_span(),
        );
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.kind, DiagnosticKind::User);
    }

    #[test]
    fn diagnostic_note_builder() {
        let d = Diagnostic::note(DiagCode::N001UndefinedValue, "info", dummy_span());
        assert_eq!(d.severity, Severity::Note);
        assert_eq!(d.kind, DiagnosticKind::User);
    }

    #[test]
    fn diagnostic_hint_builder() {
        let d = Diagnostic::hint(
            DiagCode::T005OperatorNotApplicable,
            "try cast",
            dummy_span(),
        );
        assert_eq!(d.severity, Severity::Hint);
        assert_eq!(d.kind, DiagnosticKind::User);
    }

    #[test]
    fn diagnostic_ice_builder() {
        let d = Diagnostic::ice(DiagCode::ICET001, "internal", dummy_span());
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.kind, DiagnosticKind::InternalCompilerError);
    }

    // ── Diagnostic builder: with_* ──

    #[test]
    fn diagnostic_with_label() {
        let d = Diagnostic::error(
            DiagCode::T002IncompatibleAssignment,
            "type mismatch",
            dummy_span(),
        )
        .with_label(Span::new(0, 2, 5), "here");
        assert_eq!(d.labels.len(), 1);
        assert_eq!(d.labels[0].message, "here");
        assert_eq!(d.labels[0].span, Span::new(0, 2, 5));
    }

    #[test]
    fn diagnostic_with_multiple_labels() {
        let d = Diagnostic::error(
            DiagCode::T002IncompatibleAssignment,
            "type mismatch",
            dummy_span(),
        )
        .with_label(Span::new(0, 2, 5), "a")
        .with_label(Span::new(0, 6, 8), "b");
        assert_eq!(d.labels.len(), 2);
    }

    #[test]
    fn diagnostic_with_note() {
        let d = Diagnostic::error(
            DiagCode::T002IncompatibleAssignment,
            "type mismatch",
            dummy_span(),
        )
        .with_note("consider adding a cast");
        assert_eq!(d.notes, vec!["consider adding a cast"]);
    }

    #[test]
    fn diagnostic_with_hint() {
        let d = Diagnostic::error(
            DiagCode::T002IncompatibleAssignment,
            "type mismatch",
            dummy_span(),
        )
        .with_hint("try using `as`");
        assert_eq!(d.hints.len(), 1);
        assert_eq!(d.hints[0].message, "try using `as`");
        assert_eq!(d.hints[0].replacement, None);
    }

    #[test]
    fn diagnostic_with_hint_replacement() {
        let hint = Hint {
            message: "replace with int".to_string(),
            replacement: Some(CodeReplacement {
                span: Span::new(0, 0, 3),
                new_text: "int".to_string(),
            }),
        };
        let d = Diagnostic::error(DiagCode::T010InvalidCast, "bad cast", dummy_span())
            .with_hint_replacement(hint);
        assert_eq!(d.hints.len(), 1);
        assert_eq!(d.hints[0].message, "replace with int");
        assert!(d.hints[0].replacement.is_some());
    }

    #[test]
    fn diagnostic_starts_empty() {
        let d = Diagnostic::error(DiagCode::T001CannotInferType, "x", dummy_span());
        assert!(d.labels.is_empty());
        assert!(d.notes.is_empty());
        assert!(d.hints.is_empty());
    }

    // ── format_for_cli: no registry ──

    #[test]
    fn format_no_registry() {
        let d = Diagnostic::error(
            DiagCode::T001CannotInferType,
            "cannot infer type of `x`",
            dummy_span(),
        );
        let out = d.format_for_cli(&SourceRegistry::new());
        assert_eq!(out, "T001: cannot infer type of `x`\n  --> 1:1");
    }

    #[test]
    fn format_with_registry() {
        let reg = registry_with("let x = 1;");
        let d = Diagnostic::warning(
            DiagCode::W001VariableAssignedNotUsed,
            "unused variable `x`",
            Span::new(0, 4, 5),
        );
        let out = d.format_for_cli(&reg);
        // Source: "let x = 1;" — byte 4 = line 1, col 5 (1-based)
        assert_eq!(out, "W001: unused variable `x`\n  --> test.arandu:1:5");
    }

    #[test]
    fn format_with_label() {
        let reg = registry_with("let x: int = 5;");
        let d = Diagnostic::error(
            DiagCode::T002IncompatibleAssignment,
            "type mismatch",
            Span::new(0, 0, 3),
        )
        .with_label(Span::new(0, 8, 11), "expected `int`");
        let out = d.format_for_cli(&reg);
        assert!(out.contains("expected `int`"));
        assert!(out.contains("label: 1:9-1:12"));
    }

    #[test]
    fn format_with_note() {
        let d = Diagnostic::error(
            DiagCode::T002IncompatibleAssignment,
            "mismatch",
            dummy_span(),
        )
        .with_note("try casting");
        let out = d.format_for_cli(&SourceRegistry::new());
        assert!(out.contains("note: try casting"));
    }

    #[test]
    fn format_with_hint() {
        let d = Diagnostic::error(
            DiagCode::T002IncompatibleAssignment,
            "mismatch",
            dummy_span(),
        )
        .with_hint("use `as`");
        let out = d.format_for_cli(&SourceRegistry::new());
        assert!(out.contains("hint: use `as`"));
    }

    #[test]
    fn format_with_hint_replacement() {
        let hint = Hint {
            message: "replace with int".to_string(),
            replacement: Some(CodeReplacement {
                span: Span::new(0, 0, 4),
                new_text: "int".to_string(),
            }),
        };
        let reg = registry_with("let x: str = 5;");
        let d = Diagnostic::error(
            DiagCode::T010InvalidCast,
            "invalid cast",
            Span::new(0, 0, 0),
        )
        .with_hint_replacement(hint);
        let out = d.format_for_cli(&reg);
        assert!(out.contains("replacement: at 1:1-1:5 with \"int\""));
    }

    #[test]
    fn format_ice_code_prefix() {
        let d = Diagnostic::ice(DiagCode::ICET001, "internal error", dummy_span());
        let out = d.format_for_cli(&SourceRegistry::new());
        assert!(out.starts_with("ICE-T-001: internal error"));
    }

    #[test]
    fn format_no_trailing_newline() {
        let d = Diagnostic::error(
            DiagCode::P001UnexpectedToken,
            "unexpected token",
            dummy_span(),
        );
        let out = d.format_for_cli(&SourceRegistry::new());
        assert!(
            !out.ends_with('\n'),
            "output should not have trailing newline"
        );
    }

    // ── Display ──

    #[test]
    fn display_delegates_to_format_for_cli() {
        let d = Diagnostic::error(DiagCode::T001CannotInferType, "x", dummy_span());
        let display = d.to_string();
        let formatted = d.format_for_cli(&SourceRegistry::default());
        assert_eq!(display, formatted);
    }
}
