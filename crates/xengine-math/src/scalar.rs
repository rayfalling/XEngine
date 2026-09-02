//! Scalar numeric traits and constants for `xengine_math`.
//!
//! The project is intentionally zero-dependency (`std` only), so this module
//! provides its own tiny abstraction instead of pulling in `num-traits`. The
//! split mirrors the C++ template + SFINAE (is_same_v<float>) pattern:
//!
//! * [`ScalarNum`] — the arithmetic + conversion requirements shared by the
//!   float and integer variants (component/layout math only).
//! * [`FloatNum`] — adds the floating-point-only ops (`sqrt`, `abs`, `min`,
//!   `max`, `is_finite`) used by length/normalize/distance/approx comparisons.
//! * [`IntNum`] — a marker trait for the integer variants, which only support
//!   component/layout math (no `sqrt`/`normalize`/`approx_eq`).
//!
//! Both `FloatNum` and `IntNum` extend [`ScalarNum`]; the integer variants
//! deliberately never implement [`FloatNum`], so float-only methods are simply
//! unavailable to them at compile time (matching the spec's "i32 变体仅布局与
//! 分量运算" rule).

use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// Default absolute tolerance used by [`approx_eq`](crate::vector) helpers
/// where an explicit epsilon is not supplied.
///
/// Chosen as a reasonable value for `f32` in the `~1.0` domain; callers that
/// need tighter/looser control should pass an explicit `eps`.
pub const EPSILON: f32 = 1e-5;

/// The scalar component bound shared by the float (`f32`/`f64`) and integer
/// (`i32`/`i64`) variants.
///
/// `T: ScalarNum` guarantees the component arithmetic and conversions used by
/// the generic primitives, and is therefore the bound for the pure layout /
/// component operations (add/sub/mul/div, dot/cross, length_sqr, lerp, ...).
pub trait ScalarNum:
    Copy
    + PartialEq
    + PartialOrd
    + Send
    + Sync
    + fmt::Debug
    + 'static
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
{
    /// Additive identity (`0`).
    const ZERO: Self;
    /// Multiplicative identity (`1`).
    const ONE: Self;
    /// Convert an `f32` to this scalar (lossy for the integer variants).
    fn from_f32(v: f32) -> Self;
    /// Convert this scalar to an `f32` (lossy for the integer variants).
    fn to_f32(self) -> f32;
}

/// Floating-point numeric operations used by length/normalize/distance and
/// the `approx_eq`/`is_finite` safety helpers.
///
/// Implemented for `f32` and `f64`. The integer variants never implement this,
/// which ensures float-only methods are a compile error on `Vector3I` & co.
pub trait FloatNum: ScalarNum {
    /// Square root.
    fn sqrt(self) -> Self;
    /// Absolute value.
    fn abs(self) -> Self;
    /// Minimum of two values.
    fn min(self, other: Self) -> Self;
    /// Maximum of two values.
    fn max(self, other: Self) -> Self;
    /// Whether the value is finite (neither NaN nor infinite).
    fn is_finite(self) -> bool;
    /// Sine.
    fn sin(self) -> Self;
    /// Cosine.
    fn cos(self) -> Self;
    /// Arc sine.
    fn asin(self) -> Self;
    /// Arc cosine.
    fn acos(self) -> Self;
    /// Arc tangent of `self / other` (two-argument atan2, `self` is the `y`).
    fn atan2(self, other: Self) -> Self;
    /// The scalar-type epsilon, for default `approx_eq` tolerance.
    const EPS: Self;
}

/// Marker trait for the integer variants (`i32`/`i64`).
///
/// These support layout and component operations only; they never implement
/// [`FloatNum`], so no float-only operation is exposed on them.
pub trait IntNum: ScalarNum {}

impl ScalarNum for f32 {
    const ZERO: Self = 0.0;
    const ONE: Self = 1.0;
    #[inline]
    fn from_f32(v: f32) -> Self {
        v
    }
    #[inline]
    fn to_f32(self) -> f32 {
        self
    }
}

impl FloatNum for f32 {
    #[inline]
    fn sqrt(self) -> Self {
        f32::sqrt(self)
    }
    #[inline]
    fn abs(self) -> Self {
        f32::abs(self)
    }
    #[inline]
    fn min(self, other: Self) -> Self {
        f32::min(self, other)
    }
    #[inline]
    fn max(self, other: Self) -> Self {
        f32::max(self, other)
    }
    #[inline]
    fn is_finite(self) -> bool {
        f32::is_finite(self)
    }
    #[inline]
    fn sin(self) -> Self {
        f32::sin(self)
    }
    #[inline]
    fn cos(self) -> Self {
        f32::cos(self)
    }
    #[inline]
    fn asin(self) -> Self {
        f32::asin(self)
    }
    #[inline]
    fn acos(self) -> Self {
        f32::acos(self)
    }
    #[inline]
    fn atan2(self, other: Self) -> Self {
        f32::atan2(self, other)
    }
    const EPS: Self = EPSILON;
}

impl ScalarNum for f64 {
    const ZERO: Self = 0.0;
    const ONE: Self = 1.0;
    #[inline]
    fn from_f32(v: f32) -> Self {
        v as f64
    }
    #[inline]
    fn to_f32(self) -> f32 {
        self as f32
    }
}

impl FloatNum for f64 {
    #[inline]
    fn sqrt(self) -> Self {
        f64::sqrt(self)
    }
    #[inline]
    fn abs(self) -> Self {
        f64::abs(self)
    }
    #[inline]
    fn min(self, other: Self) -> Self {
        f64::min(self, other)
    }
    #[inline]
    fn max(self, other: Self) -> Self {
        f64::max(self, other)
    }
    #[inline]
    fn is_finite(self) -> bool {
        f64::is_finite(self)
    }
    #[inline]
    fn sin(self) -> Self {
        f64::sin(self)
    }
    #[inline]
    fn cos(self) -> Self {
        f64::cos(self)
    }
    #[inline]
    fn asin(self) -> Self {
        f64::asin(self)
    }
    #[inline]
    fn acos(self) -> Self {
        f64::acos(self)
    }
    #[inline]
    fn atan2(self, other: Self) -> Self {
        f64::atan2(self, other)
    }
    const EPS: Self = EPSILON as f64;
}

impl ScalarNum for i32 {
    const ZERO: Self = 0;
    const ONE: Self = 1;
    #[inline]
    fn from_f32(v: f32) -> Self {
        v as i32
    }
    #[inline]
    fn to_f32(self) -> f32 {
        self as f32
    }
}

impl IntNum for i32 {}

impl ScalarNum for i64 {
    const ZERO: Self = 0;
    const ONE: Self = 1;
    #[inline]
    fn from_f32(v: f32) -> Self {
        v as i64
    }
    #[inline]
    fn to_f32(self) -> f32 {
        self as f32
    }
}

impl IntNum for i64 {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_scalars_support_float_ops() {
        assert_eq!(f32::from_f32(2.5), 2.5);
        assert_eq!(f64::from_f32(2.5), 2.5f64);
        assert_eq!(9.0f32.sqrt(), 3.0);
        assert_eq!((-2.0f32).abs(), 2.0);
        assert_eq!(f32::min(1.0, 2.0), 1.0);
        assert_eq!(f32::max(1.0, 2.0), 2.0);
        assert!(1.0f32.is_finite());
        assert!(!f32::INFINITY.is_finite());
    }

    #[test]
    fn int_scalars_support_component_ops_only() {
        assert_eq!(i32::from_f32(2.9), 2);
        // Arithmetic operators are available via ScalarNum.
        let a: i32 = 7;
        let b: i32 = 3;
        assert_eq!(a + b, 10);
        assert_eq!(a * b, 21);
        assert_eq!(a / b, 2);
        let n = -a;
        assert_eq!(n, -7);
        // Both integer types satisfy the IntNum marker.
        fn needs_int<T: IntNum>() {}
        needs_int::<i32>();
        needs_int::<i64>();
    }
}
