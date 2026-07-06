pub mod scalar;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod avx2;
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod sse2;

#[cfg(target_arch = "aarch64")]
pub mod neon;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimdBackendKind {
    Scalar,
    Sse2,
    Avx2,
    Neon,
}

impl SimdBackendKind {
    #[must_use]
    pub fn detect() -> Self {
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if is_x86_feature_detected!("avx2") {
                return SimdBackendKind::Avx2;
            }
            if is_x86_feature_detected!("sse2") {
                return SimdBackendKind::Sse2;
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            // Neon is standard/guaranteed on aarch64, but we check using standard APIs for safety.
            if std::arch::is_aarch64_feature_detected!("neon") {
                return SimdBackendKind::Neon;
            }
        }
        SimdBackendKind::Scalar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_whitespace_equivalence() {
        let inputs = vec![
            "",
            "   ",
            "\n\n\n",
            "\r\n\r\n",
            " \t\r\n \t\r\n",
            "   \n  \t  x",
            "x   \n",
            " \t\r\n \t\r\n \t\r\n \t\r\n \t\r\n \t\r\n \t\r\n \t\r\n \t\r\n \t\r\n \t\r\n \t\r\n \t\r\n \t\r\n", // > 32 bytes
            "                                                  \n", // > 32 bytes
        ];

        for input in inputs {
            let bytes = input.as_bytes();
            let scalar_res = scalar::skip_whitespace(bytes);

            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                if is_x86_feature_detected!("sse2") {
                    let sse2_res = unsafe { sse2::skip_whitespace(bytes) };
                    assert_eq!(scalar_res, sse2_res, "SSE2 mismatch for input {:?}", input);
                }
                if is_x86_feature_detected!("avx2") {
                    let avx2_res = unsafe { avx2::skip_whitespace(bytes) };
                    assert_eq!(scalar_res, avx2_res, "AVX2 mismatch for input {:?}", input);
                }
            }

            #[cfg(target_arch = "aarch64")]
            {
                if std::arch::is_aarch64_feature_detected!("neon") {
                    let neon_res = unsafe { neon::skip_whitespace(bytes) };
                    assert_eq!(scalar_res, neon_res, "NEON mismatch for input {:?}", input);
                }
            }
            
            let _ = scalar_res; // Prevent unused warning on archs without SIMD (like ARMv7)
        }
    }

    #[test]
    fn test_identifier_equivalence() {
        let inputs = vec![
            "",
            "abc",
            "a_b_c_1_2_3",
            "123", // starts with digit, but scanned as ident continue
            "abc def",
            "abc\ndef",
            "abc_á_def", // stops at á
            "a_very_long_identifier_that_spans_more_than_sixteen_characters", // > 16
            "a_very_long_identifier_that_spans_more_than_thirty_two_characters_to_trigger_avx2_loop", // > 32
        ];

        for input in inputs {
            let bytes = input.as_bytes();
            let scalar_res = scalar::scan_identifier(bytes);

            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            {
                if is_x86_feature_detected!("sse2") {
                    let sse2_res = unsafe { sse2::scan_identifier(bytes) };
                    assert_eq!(scalar_res, sse2_res, "SSE2 mismatch for input {:?}", input);
                }
                if is_x86_feature_detected!("avx2") {
                    let avx2_res = unsafe { avx2::scan_identifier(bytes) };
                    assert_eq!(scalar_res, avx2_res, "AVX2 mismatch for input {:?}", input);
                }
            }

            #[cfg(target_arch = "aarch64")]
            {
                if std::arch::is_aarch64_feature_detected!("neon") {
                    let neon_res = unsafe { neon::scan_identifier(bytes) };
                    assert_eq!(scalar_res, neon_res, "NEON mismatch for input {:?}", input);
                }
            }
            
            let _ = scalar_res; // Prevent unused warning on archs without SIMD (like ARMv7)
        }
    }
}
