//! Shared fuzz target implementations.
//!
//! Both libFuzzer and the mandatory regression runner call these functions, so
//! a target cannot silently bit-rot outside the scheduled fuzz workflow.

use arandu_query::{AnalysisHost, DatabaseImpl};

pub const MAX_INPUT_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    Lex,
    Syntax,
    Pipeline,
    Cycles,
    LexSimd,
    Structured,
}

impl Target {
    pub const ALL: [Self; 6] = [
        Self::Lex,
        Self::Syntax,
        Self::Pipeline,
        Self::Cycles,
        Self::LexSimd,
        Self::Structured,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Lex => "lex",
            Self::Syntax => "syntax",
            Self::Pipeline => "pipeline",
            Self::Cycles => "cycles",
            Self::LexSimd => "lex-simd",
            Self::Structured => "structured",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|target| target.name() == name)
    }
}

pub fn run(target: Target, data: &[u8]) {
    if data.len() > MAX_INPUT_BYTES {
        return;
    }
    match target {
        Target::Lex => run_lex(data),
        Target::Syntax => run_syntax(data),
        Target::Pipeline => run_pipeline(data),
        Target::Cycles => run_cycles(data),
        Target::LexSimd => run_lex_simd(data),
        Target::Structured => run_structured(data),
    }
}

fn source(data: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(data)
}

fn run_lex(data: &[u8]) {
    let _ = arandu_lexer::lex_recovering(&source(data));
}

fn run_syntax(data: &[u8]) {
    let source = source(data);
    let _ = arandu_parser::parse_syntax(&source);
    let _ = arandu_parser::parse_recovering(&source);
}

fn run_pipeline(data: &[u8]) {
    let mut db = DatabaseImpl::default();
    let file = db.new_file("fuzz/input.aru".into(), source(data).into_owned());
    let _ = arandu_query::syntax_tree(&db, file);
    let _ = arandu_query::passes::parse(&db, file);
    let _ = arandu_query::passes::resolve(&db, file);
    let _ = arandu_query::passes::type_check(&db, file);
    let _ = arandu_query::lower_amir(&db, file);
}

fn run_cycles(data: &[u8]) {
    let width = 2 + data.first().copied().unwrap_or(0) as usize % 3;
    let mut host = AnalysisHost::new();
    let mut files = Vec::with_capacity(width);
    for index in 0..width {
        let next = (index + 1) % width;
        let payload = data.get(index + 1).copied().unwrap_or(index as u8);
        files.push(host.new_file(
            format!("mod_{index}.aru"),
            format!("import mod_{next}\nfunc value_{index}(): int {{ return {payload} }}\n"),
        ));
    }
    for &file in files.iter().rev() {
        let _ = arandu_query::passes::module_dependency_graph(host.db(), file);
        let _ = arandu_query::passes::resolve(host.db(), file);
        let _ = arandu_query::passes::type_check(host.db(), file);
    }
    let mut workers = Vec::new();
    for file in files {
        let snapshot = host.snapshot();
        workers.push(std::thread::spawn(move || {
            let _ = arandu_query::passes::module_dependency_graph(&snapshot.db, file);
            let _ = arandu_query::passes::resolve(&snapshot.db, file);
            let _ = arandu_query::passes::type_check(&snapshot.db, file);
        }));
    }
    for worker in workers {
        assert!(worker.join().is_ok(), "concurrent cyclic query panicked");
    }
}

fn run_lex_simd(data: &[u8]) {
    if data.is_empty() {
        return;
    }
    let scalar_ws = arandu_lexer::simd::scalar::skip_whitespace(data);
    let scalar_ident = arandu_lexer::simd::scalar::scan_identifier(data);
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    #[cfg(target_feature = "sse2")]
    unsafe {
        assert_eq!(scalar_ws, arandu_lexer::simd::sse2::skip_whitespace(data));
        assert_eq!(
            scalar_ident,
            arandu_lexer::simd::sse2::scan_identifier(data)
        );
    }
    #[cfg(target_arch = "aarch64")]
    #[cfg(target_feature = "neon")]
    unsafe {
        assert_eq!(scalar_ws, arandu_lexer::simd::neon::skip_whitespace(data));
        assert_eq!(
            scalar_ident,
            arandu_lexer::simd::neon::scan_identifier(data)
        );
    }
}

fn run_structured(data: &[u8]) {
    if data.is_empty() {
        return;
    }
    let mut source = String::from("func main() {\n");
    for (index, byte) in data.iter().take(4096).enumerate() {
        match byte % 6 {
            0 => source.push_str(&format!("let v{index}: int = {};\n", *byte as i16 - 128)),
            1 => source.push_str("if true { let x = 1; }\n"),
            2 => source.push_str("while false { break; }\n"),
            3 => source.push_str("let s = \"structured\";\n"),
            4 => source.push_str("let x = (((1 + 2) * 3) - 4);\n"),
            _ => source.push_str("unknown(value);\n"),
        }
    }
    source.push_str("}\n");
    let _ = arandu_parser::parse_recovering(&source);
}
