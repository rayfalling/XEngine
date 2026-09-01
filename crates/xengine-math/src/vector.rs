//! Generic vector primitives: [`Vector2`], [`Vector3`], [`Vector4`].
//!
//! All vectors are `#[repr(C)]` and their fields are the FFI layout contract.
//! `Vector2<T>` is fixed at 8-byte alignment (2D is not a SIMD hot path);
//! `Vector3/4<T>` follow the crate-wide 16-byte (default) / 64-byte
//! (`xmath_align64`) alignment switch. See the crate docs for the C++ mirror
//! obligation.

use crate::scalar::{FloatNum, ScalarNum};
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// Shared implementation generator for the vector types.
///
/// Generates the component/layout operations (bounded by [`ScalarNum`]) and the
/// floating-point-only operations (bounded by [`FloatNum`]). Type-specific
/// behavior (`cross` for 3D, `perpendicular` for 2D) is added separately.
macro_rules! vector_impl {
    ($ty:ident, $($fld:ident),+) => {
        /// A generic N-component vector stored in the given component layout.
        impl<T: ScalarNum> $ty<T> {
            /// Constructs a vector from its components.
            #[inline]
            pub fn new($($fld: T),+) -> Self {
                Self { $($fld),+ }
            }

            /// Splats a single value into every component.
            #[inline]
            pub fn splat(v: T) -> Self {
                Self { $($fld: v),+ }
            }

            /// Additive identity vector.
            pub const ZERO: Self = Self { $($fld: T::ZERO),+ };

            /// All-ones vector.
            pub const ONE: Self = Self { $($fld: T::ONE),+ };

            /// Component-wise dot product.
            #[inline]
            pub fn dot(&self, rhs: &Self) -> T {
                let products = [ $( self.$fld * rhs.$fld ),+ ];
                products.into_iter().fold(T::ZERO, |a, b| a + b)
            }

            /// Squared length (no square root; avoids a `FloatNum` bound).
            #[inline]
            pub fn length_sqr(&self) -> T {
                self.dot(self)
            }

            /// Component-wise linear interpolation: `self + (rhs - self) * t`.
            #[inline]
            pub fn lerp(&self, rhs: &Self, t: T) -> Self {
                *self + (*rhs - *self) * t
            }
        }

        impl<T: FloatNum> $ty<T> {
            /// Euclidean length.
            #[inline]
            pub fn length(&self) -> T {
                self.length_sqr().sqrt()
            }

            /// Normalizes to unit length, returning the zero vector when the
            /// input is a zero vector (no NaN).
            #[inline]
            pub fn normalize_or_zero(&self) -> Self {
                let len_sq = self.length_sqr();
                if len_sq <= T::ZERO {
                    return Self::ZERO;
                }
                let inv = T::ONE / len_sq.sqrt();
                Self { $($fld: self.$fld * inv),+ }
            }

            /// Distance to another vector.
            #[inline]
            pub fn distance(&self, rhs: &Self) -> T {
                (*self - *rhs).length()
            }

            /// Component-wise absolute value.
            #[inline]
            pub fn abs(&self) -> Self {
                Self { $($fld: self.$fld.abs()),+ }
            }

            /// Component-wise minimum.
            #[inline]
            pub fn min(&self, rhs: &Self) -> Self {
                Self { $($fld: self.$fld.min(rhs.$fld)),+ }
            }

            /// Component-wise maximum.
            #[inline]
            pub fn max(&self, rhs: &Self) -> Self {
                Self { $($fld: self.$fld.max(rhs.$fld)),+ }
            }

            /// Approximate equality within `eps` (component-wise).
            #[inline]
            pub fn approx_eq(&self, rhs: &Self, eps: T) -> bool {
                $( if (self.$fld - rhs.$fld).abs() > eps { return false; } )+
                true
            }

            /// Whether every component is finite.
            #[inline]
            pub fn is_finite(&self) -> bool {
                $( if !self.$fld.is_finite() { return false; } )+
                true
            }
        }

        impl<T: ScalarNum> Add for $ty<T> {
            type Output = Self;
            #[inline]
            fn add(self, rhs: Self) -> Self {
                Self { $($fld: self.$fld + rhs.$fld),+ }
            }
        }

        impl<T: ScalarNum> Sub for $ty<T> {
            type Output = Self;
            #[inline]
            fn sub(self, rhs: Self) -> Self {
                Self { $($fld: self.$fld - rhs.$fld),+ }
            }
        }

        impl<T: ScalarNum> Mul for $ty<T> {
            type Output = Self;
            #[inline]
            fn mul(self, rhs: Self) -> Self {
                Self { $($fld: self.$fld * rhs.$fld),+ }
            }
        }

        impl<T: ScalarNum> Div for $ty<T> {
            type Output = Self;
            #[inline]
            fn div(self, rhs: Self) -> Self {
                Self { $($fld: self.$fld / rhs.$fld),+ }
            }
        }

        impl<T: ScalarNum> Mul<T> for $ty<T> {
            type Output = Self;
            #[inline]
            fn mul(self, s: T) -> Self {
                Self { $($fld: self.$fld * s),+ }
            }
        }

        impl<T: ScalarNum> Div<T> for $ty<T> {
            type Output = Self;
            #[inline]
            fn div(self, s: T) -> Self {
                Self { $($fld: self.$fld / s),+ }
            }
        }

        impl<T: ScalarNum> Neg for $ty<T> {
            type Output = Self;
            #[inline]
            fn neg(self) -> Self {
                Self { $($fld: -self.$fld),+ }
            }
        }

        impl<T: fmt::Debug> fmt::Display for $ty<T> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, stringify!($ty))?;
                write!(f, "(")?;
                let mut first = true;
                $(
                    if !first { write!(f, ", ")?; }
                    write!(f, "{:?}", self.$fld)?;
                    first = false;
                )+
                write!(f, ")")
            }
        }
    };
}

// ─── Vector2 ────────────────────────────────────────────────────────────────

/// A 2-component generic vector. Fixed at 8-byte alignment (not a SIMD hot path).
#[repr(C)]
#[repr(align(8))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector2<T> {
    pub x: T,
    pub y: T,
}

vector_impl!(Vector2, x, y);

impl<T: ScalarNum> Vector2<T> {
    /// Perpendicular vector `(-y, x)`.
    #[inline]
    pub fn perpendicular(&self) -> Self {
        Self {
            x: -self.y,
            y: self.x,
        }
    }
}

// ─── Vector3 ────────────────────────────────────────────────────────────────

/// A 3-component generic vector.
#[repr(C)]
#[cfg_attr(not(feature = "xmath_align64"), repr(align(16)))]
#[cfg_attr(feature = "xmath_align64", repr(align(64)))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector3<T> {
    pub x: T,
    pub y: T,
    pub z: T,
}

vector_impl!(Vector3, x, y, z);

impl<T: ScalarNum> Vector3<T> {
    /// Component-wise cross product.
    #[inline]
    pub fn cross(&self, rhs: &Self) -> Self {
        Self {
            x: self.y * rhs.z - self.z * rhs.y,
            y: self.z * rhs.x - self.x * rhs.z,
            z: self.x * rhs.y - self.y * rhs.x,
        }
    }
}

// ─── Vector4 ────────────────────────────────────────────────────────────────

/// A 4-component generic vector (`w` is the homogeneous/perspective component).
#[repr(C)]
#[cfg_attr(not(feature = "xmath_align64"), repr(align(16)))]
#[cfg_attr(feature = "xmath_align64", repr(align(64)))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vector4<T> {
    pub x: T,
    pub y: T,
    pub z: T,
    pub w: T,
}

vector_impl!(Vector4, x, y, z, w);

// ─── Aliases ────────────────────────────────────────────────────────────────

/// `f32` 2-component vector.
pub type Vector2F = Vector2<f32>;
/// `i32` 2-component vector.
pub type Vector2I = Vector2<i32>;
/// `f32` 3-component vector.
pub type Vector3F = Vector3<f32>;
/// `i32` 3-component vector.
pub type Vector3I = Vector3<i32>;
/// `f32` 4-component vector.
pub type Vector4F = Vector4<f32>;
/// `i32` 4-component vector.
pub type Vector4I = Vector4<i32>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_resolve_to_generic_instances() {
        use crate::scalar::IntNum;
        fn assert_float<T: FloatNum>() {}
        assert_float::<f32>();
        assert_float::<f64>();
        fn assert_int<T: IntNum>() {}
        assert_int::<i32>();
        assert_int::<i64>();
        // Float aliases are usable with float-only ops.
        let _: Vector2F = Vector2F::new(1.0, 2.0);
        let _: Vector3F = Vector3F::new(1.0, 2.0, 3.0);
        let _: Vector4F = Vector4F::new(1.0, 2.0, 3.0, 4.0);
        // Integer variants only support component + layout operations.
        let _: Vector2I = Vector2I::new(1, 2);
        let _: Vector3I = Vector3I::new(1, 2, 3);
        let _: Vector4I = Vector4I::new(1, 2, 3, 4);
    }

    #[test]
    fn vector3_component_and_dot_ops() {
        let a = Vector3F::new(1.0, 2.0, 3.0);
        let b = Vector3F::new(4.0, 5.0, 6.0);
        assert_eq!(a + b, Vector3F::new(5.0, 7.0, 9.0));
        assert_eq!(a - b, Vector3F::new(-3.0, -3.0, -3.0));
        assert_eq!(a * b, Vector3F::new(4.0, 10.0, 18.0));
        assert_eq!(
            a / Vector3F::new(2.0, 1.0, 3.0),
            Vector3F::new(0.5, 2.0, 1.0)
        );
        assert_eq!(a * 2.0, Vector3F::new(2.0, 4.0, 6.0));
        assert_eq!(a.dot(&b), 32.0);
        assert_eq!(a.length_sqr(), 14.0);
    }

    #[test]
    fn vector3_cross_handedness_left() {
        // Left-handed basis: forward = +Z (camera convention).
        let fwd = Vector3F::new(0.0, 0.0, 1.0);
        let up = Vector3F::new(0.0, 1.0, 0.0);
        // right = up × forward = +X in a left-handed (forward=+Z) frame.
        let right = up.cross(&fwd);
        assert!(right.approx_eq(&Vector3F::new(1.0, 0.0, 0.0), 1e-6));
        // A consistent left-handed frame: forward = right × up.
        let fwd2 = right.cross(&up);
        assert!(fwd2.approx_eq(&fwd, 1e-6));
    }

    #[test]
    fn normalize_or_zero_returns_zero_for_zero_input() {
        assert_eq!(
            Vector3F::ZERO.normalize_or_zero(),
            Vector3F::new(0.0, 0.0, 0.0)
        );
        let n = Vector3F::new(3.0, 0.0, 4.0).normalize_or_zero();
        assert!(n.approx_eq(&Vector3F::new(0.6, 0.0, 0.8), 1e-6));
        // length is exactly 1 for a unit vector (within f32 tolerance).
        assert!((n.length() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn length_distance_lerp_abs_min_max_finite() {
        assert_eq!(Vector3F::new(0.0, 3.0, 4.0).length(), 5.0);
        assert_eq!(
            Vector3F::new(0.0, 0.0, 0.0).distance(&Vector3F::new(0.0, 3.0, 4.0)),
            5.0
        );
        let a = Vector3F::new(0.0, 0.0, 0.0);
        let b = Vector3F::new(10.0, 20.0, 30.0);
        assert_eq!(a.lerp(&b, 0.5), Vector3F::new(5.0, 10.0, 15.0));
        assert_eq!(
            Vector3F::new(-1.0, 2.0, -3.0).abs(),
            Vector3F::new(1.0, 2.0, 3.0)
        );
        assert_eq!(
            Vector3F::new(1.0, 5.0, 3.0).min(&Vector3F::new(4.0, 2.0, 6.0)),
            Vector3F::new(1.0, 2.0, 3.0)
        );
        assert_eq!(
            Vector3F::new(1.0, 5.0, 3.0).max(&Vector3F::new(4.0, 2.0, 6.0)),
            Vector3F::new(4.0, 5.0, 6.0)
        );
        assert!(Vector3F::new(1.0, 2.0, 3.0).approx_eq(&Vector3F::new(1.0, 2.0, 3.0 + 1e-6), 1e-5));
        assert!(Vector3F::new(1.0, 2.0, 3.0).is_finite());
        assert!(!Vector3F::new(1.0, f32::NAN, 3.0).is_finite());
    }

    #[test]
    fn vector2_perpendicular() {
        let p = Vector2F::new(1.0, 2.0);
        let perp = p.perpendicular();
        // Dot with its perpendicular is always zero.
        assert_eq!(p.dot(&perp), 0.0);
        assert_eq!(perp, Vector2F::new(-2.0, 1.0));
    }

    #[test]
    fn integer_variant_supports_component_ops() {
        let a = Vector3I::new(1, 2, 3);
        let b = Vector3I::new(4, 5, 6);
        assert_eq!(a + b, Vector3I::new(5, 7, 9));
        assert_eq!(a.dot(&b), 32);
        assert_eq!(a.cross(&b), Vector3I::new(-3, 6, -3));
        assert_eq!(a.length_sqr(), 14);
    }

    #[test]
    fn display_debug_format() {
        let a = Vector3F::new(1.0, 2.0, 3.0);
        assert_eq!(a.to_string(), "Vector3(1.0, 2.0, 3.0)");
    }

    #[test]
    fn vector4_w_component() {
        let v = Vector4F::new(1.0, 2.0, 3.0, 4.0);
        let w = Vector4F::new(0.4, 0.5, 0.6, 0.0);
        assert_eq!(v.w, 4.0);
        // Dot: 1*0.4 + 2*0.5 + 3*0.6 + 4*0 = 3.2 (f32 rounded).
        assert!((v.dot(&w) - 3.2).abs() < 1e-6);
    }
}
