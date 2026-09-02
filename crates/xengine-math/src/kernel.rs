//! SIMD-reserved internal computation kernel.
//!
//! This module is the single place where a future SIMD implementation is
//! allowed to perform wide load → compute → store on the public layout. The
//! public fields are the FFI layout contract; enabling SIMD here never changes
//! field order, size, or alignment (any alignment change is gated by the
//! `xmath_align64` feature and documented separately).
//!
//! # SIMD contract
//!
//! On the SSE/AVX path these functions are the *only* sites that issue a wide
//! load/store over a public value, for example:
//!
//! ```text
//! __m128 a0 = _mm_load_ps(&self.m[r][0]);   // load  a row  (4 x f32)
//! __m128 b0 = _mm_load_ps(&rhs.m[k][0]);    // load  a row  (4 x f32)
//! ... // compute
//! _mm_store_ps(&out.m[r][0], acc);          // store a row  (4 x f32)
//! ```
//!
//! The scalar kernels below are the current reference semantics; the SIMD
//! replacements MUST produce bit-identical results for the public API, which
//! is locked by the unit tests (the "SIMD 后行为不变" scenario).

use crate::scalar::ScalarNum;

/// Row-major 4×4 matrix product `a · b` (scalar kernel).
pub(crate) fn mat4_mul<T: ScalarNum>(a: &[[T; 4]; 4], b: &[[T; 4]; 4]) -> [[T; 4]; 4] {
    let mut out = [[T::ZERO; 4]; 4];
    for r in 0..4 {
        for c in 0..4 {
            let mut sum = T::ZERO;
            for k in 0..4 {
                sum = sum + a[r][k] * b[k][c];
            }
            out[r][c] = sum;
        }
    }
    out
}

/// Row-major 3×3 matrix product `a · b` (scalar kernel).
pub(crate) fn mat3_mul<T: ScalarNum>(a: &[[T; 3]; 3], b: &[[T; 3]; 3]) -> [[T; 3]; 3] {
    let mut out = [[T::ZERO; 3]; 3];
    for r in 0..3 {
        for c in 0..3 {
            let mut sum = T::ZERO;
            for k in 0..3 {
                sum = sum + a[r][k] * b[k][c];
            }
            out[r][c] = sum;
        }
    }
    out
}

/// Row-vector point transform (`w = 1`, translation applied) — scalar kernel.
pub(crate) fn mat4_mul_point<T: ScalarNum>(m: &[[T; 4]; 4], v: &[T; 3]) -> [T; 3] {
    [
        v[0] * m[0][0] + v[1] * m[1][0] + v[2] * m[2][0] + m[3][0],
        v[0] * m[0][1] + v[1] * m[1][1] + v[2] * m[2][1] + m[3][1],
        v[0] * m[0][2] + v[1] * m[1][2] + v[2] * m[2][2] + m[3][2],
    ]
}

/// Row-vector direction transform (`w = 0`, no translation) — scalar kernel.
pub(crate) fn mat4_mul_dir<T: ScalarNum>(m: &[[T; 4]; 4], v: &[T; 3]) -> [T; 3] {
    [
        v[0] * m[0][0] + v[1] * m[1][0] + v[2] * m[2][0],
        v[0] * m[0][1] + v[1] * m[1][1] + v[2] * m[2][1],
        v[0] * m[0][2] + v[1] * m[1][2] + v[2] * m[2][2],
    ]
}

/// Row-vector 4-component transform `v · m` (no perspective divide) — scalar kernel.
pub(crate) fn mat4_mul_vec4<T: ScalarNum>(m: &[[T; 4]; 4], v: &[T; 4]) -> [T; 4] {
    [
        v[0] * m[0][0] + v[1] * m[1][0] + v[2] * m[2][0] + v[3] * m[3][0],
        v[0] * m[0][1] + v[1] * m[1][1] + v[2] * m[2][1] + v[3] * m[3][1],
        v[0] * m[0][2] + v[1] * m[1][2] + v[2] * m[2][2] + v[3] * m[3][2],
        v[0] * m[0][3] + v[1] * m[1][3] + v[2] * m[2][3] + v[3] * m[3][3],
    ]
}
