//! # xengine-math
//!
//! Zero-dependency, `std`-only math primitives for XEngine. The types are
//! generic over the scalar component (`f32`/`f64` for floats, `i32`/`i64` for
//! integers) and are the data-contract root for the core layer's `Transform`
//! and the C++ device-layer mirror.
//!
//! ## Math conventions (locked by the `core-math` spec)
//!
//! These conventions must never drift — they are the FFI contract:
//!
//! * **Row-major** matrices — `m[row][col]`.
//! * **Row-vector × matrix** transforms — `v' = v · M`; `A·B` applies `A`
//!   first, then `B`.
//! * **Left-handed** coordinate system, identity forward `= +Z`.
//! * Quaternion components `(x, y, z, w)` with `w` **last**.
//! * Euler angles in **YXZ** order, matching DirectXMath
//!   `XMQuaternionRotationRollPitchYaw(pitch, yaw, roll)`.
//! * `Matrix4` translation in `m[3][0..2]`.
//! * `perspective_lh` maps depth to `[0, 1]` (D3D convention).
//!
//! ## Layout / FFI contract
//!
//! Every public type is `#[repr(C)]`. By default they are 16-byte aligned
//! (**`Vector2<T>` is fixed at 8-byte alignment** — 2D is not a SIMD hot
//! path). The crate feature `xmath_align64` switches the remaining types to
//! 64-byte alignment:
//!
//! ```toml
//! xengine-math = { features = ["xmath_align64"] }
//! ```
//!
//! **C++ device-layer obligation:** when `xmath_align64` is enabled the C++
//! mirror headers must update their layout to the 64-byte alignment, and the
//! array stride grows accordingly (cache usage drops). Two build configurations
//! are locked by layout tests (`size_of` / `align_of` / field offsets).
//!
//! ## SIMD reservation
//!
//! The public structure fields are the layout contract; SIMD may only happen
//! inside the internal [`kernel`] module's explicit load/store points. Enabling
//! SIMD never changes field order/size/alignment, and the public scalar
//! semantics are locked by unit tests.

// `AABBF`/`Matrix*F`/`Vector*F` are deliberate all-caps acronym + suffix type
// aliases mandated by the `core-math` spec (no abbreviated names like
// `AabbF`/`Vec3f`); they are *not* misspellings, so the lint is suppressed
// crate-wide.
#![allow(clippy::upper_case_acronyms)]
#![deny(unsafe_code)]

pub mod scalar;

pub(crate) mod kernel;

pub mod aabb;
pub mod matrix;
pub mod quaternion;
pub mod vector;

pub use aabb::{AABB, AABBF};
pub use matrix::{Matrix3, Matrix3F, Matrix4, Matrix4F};
pub use quaternion::{Quaternion, QuaternionF};
pub use scalar::{EPSILON, FloatNum, IntNum, ScalarNum};
pub use vector::{
    Vector2, Vector2F, Vector2I, Vector3, Vector3F, Vector3I, Vector4, Vector4F, Vector4I,
};

// Convenience re-exports of the free-form transform helpers.
pub use matrix::{transform_point, transform_vec3};

#[cfg(test)]
mod layout_tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    // Field offsets are identical across both alignment configs.
    #[test]
    fn vector3_field_offsets() {
        assert_eq!(offset_of!(Vector3F, x), 0);
        assert_eq!(offset_of!(Vector3F, y), 4);
        assert_eq!(offset_of!(Vector3F, z), 8);
    }

    #[test]
    fn quaternion_w_offsets_last() {
        assert_eq!(offset_of!(QuaternionF, x), 0);
        assert_eq!(offset_of!(QuaternionF, y), 4);
        assert_eq!(offset_of!(QuaternionF, z), 8);
        assert_eq!(offset_of!(QuaternionF, w), 12);
    }

    #[test]
    fn aabb_min_max_offsets() {
        assert_eq!(offset_of!(AABBF, min), 0);
        #[cfg(not(feature = "xmath_align64"))]
        assert_eq!(offset_of!(AABBF, max), 16);
        #[cfg(feature = "xmath_align64")]
        assert_eq!(offset_of!(AABBF, max), 64);
    }

    #[cfg(not(feature = "xmath_align64"))]
    mod default_16 {
        use super::*;
        #[test]
        fn alignments_are_16_except_vector2() {
            assert_eq!(align_of::<Vector2F>(), 8);
            assert_eq!(size_of::<Vector2F>(), 8);
            assert_eq!(align_of::<Vector3F>(), 16);
            assert_eq!(align_of::<Vector4F>(), 16);
            assert_eq!(align_of::<QuaternionF>(), 16);
            assert_eq!(align_of::<Matrix3F>(), 16);
            assert_eq!(align_of::<Matrix4F>(), 16);
            assert_eq!(align_of::<AABBF>(), 16);
        }
        #[test]
        fn sizes_are_locked() {
            assert_eq!(size_of::<Vector3F>(), 16); // 12 padded to 16 (align 16)
            assert_eq!(size_of::<Vector4F>(), 16);
            assert_eq!(size_of::<QuaternionF>(), 16);
            assert_eq!(size_of::<Matrix3F>(), 48); // 36 padded to 48
            assert_eq!(size_of::<Matrix4F>(), 64);
            assert_eq!(size_of::<AABBF>(), 32);
        }
    }

    #[cfg(feature = "xmath_align64")]
    mod feature_64 {
        use super::*;
        #[test]
        fn alignments_switch_to_64_except_vector2() {
            assert_eq!(align_of::<Vector2F>(), 8); // unaffected
            assert_eq!(align_of::<Vector3F>(), 64);
            assert_eq!(align_of::<Vector4F>(), 64);
            assert_eq!(align_of::<QuaternionF>(), 64);
            assert_eq!(align_of::<Matrix3F>(), 64);
            assert_eq!(align_of::<Matrix4F>(), 64);
            assert_eq!(align_of::<AABBF>(), 64);
        }
        #[test]
        fn sizes_are_multiples_of_64() {
            assert_eq!(size_of::<Vector4F>(), 64);
            assert_eq!(size_of::<QuaternionF>(), 64);
            assert_eq!(size_of::<Matrix3F>(), 64); // 36 padded to 64
            assert_eq!(size_of::<Matrix4F>(), 64);
            assert_eq!(size_of::<AABBF>(), 128);
            for s in [
                size_of::<Vector3F>(),
                size_of::<Vector4F>(),
                size_of::<QuaternionF>(),
                size_of::<Matrix4F>(),
                size_of::<AABBF>(),
            ] {
                assert_eq!(s % 64, 0, "size {s} must be a multiple of 64");
            }
        }
    }
}
