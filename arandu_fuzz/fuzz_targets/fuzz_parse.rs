#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Parser works on &str — skip non-UTF-8 inputs
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let _ = arandu_parser::parse(source);
});
