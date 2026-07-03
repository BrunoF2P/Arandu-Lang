#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = arandu_lexer::lex_recovering(s);
    } else {
        let _ = arandu_lexer::lex_recovering(&String::from_utf8_lossy(data));
    }
});
