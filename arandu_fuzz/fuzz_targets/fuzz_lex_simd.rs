#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    // Run the scalar backend
    let scalar_ws = arandu_lexer::simd::scalar::skip_whitespace(data);
    let scalar_ident = arandu_lexer::simd::scalar::scan_identifier(data);

    // Run SIMD backends that are available on this platform
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        #[cfg(target_feature = "sse2")]
        unsafe {
            let sse2_ws = arandu_lexer::simd::sse2::skip_whitespace(data);
            let sse2_ident = arandu_lexer::simd::sse2::scan_identifier(data);
            assert_eq!(scalar_ws, sse2_ws, "SSE2 whitespace mismatch");
            assert_eq!(scalar_ident, sse2_ident, "SSE2 ident mismatch");
        }

        #[cfg(target_feature = "avx2")]
        unsafe {
            let avx2_ws = arandu_lexer::simd::avx2::skip_whitespace(data);
            let avx2_ident = arandu_lexer::simd::avx2::scan_identifier(data);
            assert_eq!(scalar_ws, avx2_ws, "AVX2 whitespace mismatch");
            assert_eq!(scalar_ident, avx2_ident, "AVX2 ident mismatch");
        }
    }

    #[cfg(target_arch = "aarch64")]
    {
        #[cfg(target_feature = "neon")]
        unsafe {
            let neon_ws = arandu_lexer::simd::neon::skip_whitespace(data);
            let neon_ident = arandu_lexer::simd::neon::scan_identifier(data);
            assert_eq!(scalar_ws, neon_ws, "NEON whitespace mismatch");
            assert_eq!(scalar_ident, neon_ident, "NEON ident mismatch");
        }
    }
});
