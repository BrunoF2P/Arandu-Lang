pub use arandu_base::span::Span;
pub use arandu_base::source_registry::SourceRegistry;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Note,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticKind {
    User,
    InternalCompilerError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeReplacement {
    pub span: Span,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub message: String,
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

    // ── Warnings & Linting (W) ──
    W001VariableAssignedNotUsed,
    W002DeadCode,
    W003UnreachableCode,
    W004VariableShadowing,
    W005UnnecessaryMutability,
    W006UnhandledResult,

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
            DiagCode::W001VariableAssignedNotUsed => "W001",
            DiagCode::W002DeadCode => "W002",
            DiagCode::W003UnreachableCode => "W003",
            DiagCode::W004VariableShadowing => "W004",
            DiagCode::W005UnnecessaryMutability => "W005",
            DiagCode::W006UnhandledResult => "W006",
            DiagCode::U001FeatureNotSupported => "U001",
            DiagCode::ICELX001 => "ICE-LX-001",
            DiagCode::ICEP001 => "ICE-P-001",
            DiagCode::ICEN001 => "ICE-N-001",
            DiagCode::ICET001 => "ICE-T-001",
            DiagCode::ICEO001 => "ICE-O-001",
            DiagCode::ICEL001 => "ICE-L-001",
            DiagCode::ICEGEN001 => "ICE-GEN-001",
        }
    }
}

impl fmt::Display for DiagCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub severity: Severity,
    pub kind: DiagnosticKind,
    pub message: String,
    pub span: Span,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub hints: Vec<Hint>,
}

pub mod registry;
pub use arandu_base::index_vec;
pub use arandu_base::stable_id;


impl Diagnostic {
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

    #[must_use]
    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
        });
        self
    }

    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(Hint {
            message: hint.into(),
            replacement: None,
        });
        self
    }

    #[must_use]
    pub fn with_hint_replacement(mut self, hint: Hint) -> Self {
        self.hints.push(hint);
        self
    }

    #[must_use]
    pub fn format_for_cli(&self, registry: &SourceRegistry) -> String {
        use std::fmt::Write;
        let mut out = String::new();

        let (filepath, start_line, start_col) = if let Some(file) = registry.get_file(self.span.file_id) {
            let (line, col) = file.line_index.line_col(self.span.start);
            (file.path.as_str(), line, col)
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
        let _ = writeln!(
            out,
            "  --> {}{}:{}",
            file_prefix, start_line, start_col
        );

        for label in &self.labels {
            let (l_start_line, l_start_col, l_end_line, l_end_col) = if let Some(file) = registry.get_file(label.span.file_id) {
                let (s_line, s_col) = file.line_index.line_col(label.span.start);
                let (e_line, e_col) = file.line_index.line_col(label.span.end);
                (s_line, s_col, e_line, e_col)
            } else {
                (1, 1, 1, 1)
            };
            let _ = writeln!(
                out,
                "  label: {}:{}-{}:{} {}",
                l_start_line,
                l_start_col,
                l_end_line,
                l_end_col,
                label.message
            );
        }
        for note in &self.notes {
            let _ = writeln!(out, "  note: {note}");
        }
        for hint in &self.hints {
            let _ = writeln!(out, "  hint: {}", hint.message);
            if let Some(ref rep) = hint.replacement {
                let (r_start_line, r_start_col, r_end_line, r_end_col) = if let Some(file) = registry.get_file(rep.span.file_id) {
                    let (s_line, s_col) = file.line_index.line_col(rep.span.start);
                    let (e_line, e_col) = file.line_index.line_col(rep.span.end);
                    (s_line, s_col, e_line, e_col)
                } else {
                    (1, 1, 1, 1)
                };
                let _ = writeln!(
                    out,
                    "  replacement: at {}:{}-{}:{} with {:?}",
                    r_start_line,
                    r_start_col,
                    r_end_line,
                    r_end_col,
                    rep.new_text
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
