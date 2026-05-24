use std::fmt;

use arandu_lexer::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagCode {
    N001UndefinedValue,
    N002UndefinedType,
    N003RedefinedName,
    N004TypeUsedAsValue,
    N005ValueUsedAsType,
    N006UnresolvedImport,
    N007UndefinedAssignmentTarget,
    N008NamespaceUsedAsValue,
    N009UndefinedNamespaceMember,
    N010UndefinedAssociatedFunction,

    // ── Type Checker ───────────────────────────────────────────────
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

    // ── Lowering (HIR / AMIR) ──────────────────────────────────────
    L001LoweringUnresolvedSymbol,
    L002AmirUnsupportedFeature,

    T019ResultNotHandled,
    T021MethodSelfRequired,
    T022BreakContinueOutsideLoop,
    T023FreeRequiresPtr,
    T024NonExhaustiveMatch,
    T025InterfaceNotSatisfied,
    G001GenericInstantiationCycle,
    G002GenericInstantiationLimit,

    // ── Ownership / Initialization ────────────────────────────────
    O008UseBeforeInit,
}

impl DiagCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DiagCode::N001UndefinedValue => "N001",
            DiagCode::N002UndefinedType => "N002",
            DiagCode::N003RedefinedName => "N003",
            DiagCode::N004TypeUsedAsValue => "N004",
            DiagCode::N005ValueUsedAsType => "N005",
            DiagCode::N006UnresolvedImport => "N006",
            DiagCode::N007UndefinedAssignmentTarget => "N007",
            DiagCode::N008NamespaceUsedAsValue => "N008",
            DiagCode::N009UndefinedNamespaceMember => "N009",
            DiagCode::N010UndefinedAssociatedFunction => "N010",
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
            DiagCode::L001LoweringUnresolvedSymbol => "L001",
            DiagCode::L002AmirUnsupportedFeature => "L002",
            DiagCode::T019ResultNotHandled => "T019",
            DiagCode::T021MethodSelfRequired => "T021",
            DiagCode::T022BreakContinueOutsideLoop => "T022",
            DiagCode::T023FreeRequiresPtr => "T023",
            DiagCode::T024NonExhaustiveMatch => "T024",
            DiagCode::T025InterfaceNotSatisfied => "T025",
            DiagCode::G001GenericInstantiationCycle => "G001",
            DiagCode::G002GenericInstantiationLimit => "G002",
            DiagCode::O008UseBeforeInit => "O008",
        }
    }
}

impl fmt::Display for DiagCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub type Severity = arandu_diagnostics::Severity;
pub type Label = arandu_diagnostics::Label;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub severity: Severity,
    pub message: String,
    pub span: Span,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub hints: Vec<String>,
}

impl Diagnostic {
    pub fn error(code: DiagCode, message: impl Into<String>, span: Span) -> Self {
        Self {
            code,
            severity: Severity::Error,
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
            message: message.into(),
            span,
            labels: Vec::new(),
            notes: Vec::new(),
            hints: Vec::new(),
        }
    }

    pub fn with_label(mut self, span: Span, message: impl Into<String>) -> Self {
        self.labels.push(Label {
            span,
            message: message.into(),
        });
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hints.push(hint.into());
        self
    }

    #[must_use]
    pub fn format_for_cli(&self, filepath: &str) -> String {
        let diag = arandu_diagnostics::Diagnostic::from(self.clone());
        diag.format_for_cli(filepath)
    }
}

impl From<Diagnostic> for arandu_diagnostics::Diagnostic {
    fn from(diag: Diagnostic) -> Self {
        arandu_diagnostics::Diagnostic {
            code: diag.code.as_str().to_string(),
            severity: diag.severity,
            message: diag.message,
            span: diag.span,
            labels: diag.labels,
            notes: diag.notes,
            hints: diag.hints,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let diag = arandu_diagnostics::Diagnostic::from(self.clone());
        write!(f, "{diag}")
    }
}
