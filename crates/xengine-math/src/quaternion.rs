//! Generic quaternion primitives.
//!
//! A quaternion is stored as `(x, y, z, w)` with `w` **last**.
//! convention). All rotation APIs assume the left-handed, `forward = +Z`
//! convention of the crate. The public fields are the FFI layout contract.

use crate::matrix::{Matrix3, Matrix4};
use crate::scalar::{FloatNum, ScalarNum};
use crate::vector::Vector3;
use std::fmt;
use std::ops::{Add, Mul, Neg};

/// A generic quaternion `(x, y, z, w)`.
#[repr(C)]
#[cfg_attr(not(feature = "xmath_align64"), repr(align(16)))]
#[cfg_attr(feature = "xmath_align64", repr(align(64)))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quaternion<T> {
    pub x: T,
    pub y: T,
    pub z: T,
    pub w: T,
}

impl<T: ScalarNum> Quaternion<T> {
    /// Constructs a quaternion from its `(x, y, z, w)` components.
    #[inline]
    pub fn new(x: T, y: T, z: T, w: T) -> Self {
        Self { x, y, z, w }
    }

    /// Identity quaternion (no rotation).
    pub const IDENTITY: Self = Self {
        x: T::ZERO,
        y: T::ZERO,
        z: T::ZERO,
        w: T::ONE,
    };

    /// Extracts the vector (imaginary) part as a [`Vector3`].
    #[inline]
    pub fn xyz(&self) -> Vector3<T> {
        Vector3::new(self.x, self.y, self.z)
    }

    /// Hamilton product `self * rhs`.
    #[inline]
    pub fn mul(&self, rhs: &Self) -> Self {
        Self {
            x: self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            y: self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            z: self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
            w: self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
        }
    }

    /// Component-wise dot product.
    #[inline]
    pub fn dot(&self, rhs: &Self) -> T {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z + self.w * rhs.w
    }

    /// Squared length.
    #[inline]
    pub fn length_sqr(&self) -> T {
        self.dot(self)
    }

    /// Conjugate `(-x, -y, -z, w)`.
    #[inline]
    pub fn conjugate(&self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: self.w,
        }
    }
}

impl<T: FloatNum> Quaternion<T> {
    /// Builds a rotation quaternion from an axis (unit) and an angle (radians).
    #[inline]
    pub fn from_axis_angle(axis: Vector3<T>, angle: T) -> Self {
        let half = angle / (T::ONE + T::ONE);
        let s = half.sin();
        let c = half.cos();
        Self::new(axis.x * s, axis.y * s, axis.z * s, c)
    }

    /// Builds a quaternion from euler angles in **YXZ** order
    /// `(pitch, yaw, roll)`.
    ///
    /// Euler order is (pitch, yaw, roll) (YXZ, D3D-style).
    #[inline]
    pub fn from_euler_yxz(pitch: T, yaw: T, roll: T) -> Self {
        let half_p = pitch / (T::ONE + T::ONE);
        let half_y = yaw / (T::ONE + T::ONE);
        let half_r = roll / (T::ONE + T::ONE);
        let (sp, cp) = (half_p.sin(), half_p.cos());
        let (sy, cy) = (half_y.sin(), half_y.cos());
        let (sr, cr) = (half_r.sin(), half_r.cos());
        Self::new(
            sp * cy * cr + cp * sy * sr,
            cp * sy * cr - sp * cy * sr,
            cp * cy * sr - sp * sy * cr,
            sp * sy * sr + cp * cy * cr,
        )
    }

    /// Extracts euler angles in **YXZ** order `(pitch, yaw, roll)`.
    ///
    /// The inverse of [`from_euler_yxz`](Self::from_euler_yxz); matches the
    /// scalar path.
    #[inline]
    pub fn to_euler_yxz(&self) -> Vector3<T> {
        let singularity_test = self.y * self.z - self.x * self.w;
        let z1 = (T::ONE + T::ONE) * (self.x * self.y + self.z * self.w);
        let z2 = self.y * self.y - self.z * self.z - self.x * self.x + self.w * self.w;
        let x2 = (T::ONE + T::ONE) * singularity_test;
        let neg_one = -T::ONE;
        let cutoff = T::from_f32(0.499_999);

        let (pitch, yaw, roll);
        if singularity_test.abs() < cutoff {
            let y1 = (T::ONE + T::ONE) * (self.x * self.z + self.y * self.w);
            let y2 = self.z * self.z - self.x * self.x - self.y * self.y + self.w * self.w;
            pitch = neg_one * clamp(x2, neg_one, T::ONE).asin();
            yaw = y1.atan2(y2);
            roll = z1.atan2(z2);
        } else {
            // Gimbal-lock fallback (yaw locked to 0), matching the reference.
            let a = self.x * self.y + self.z * self.w;
            let b = -self.y * self.z + self.x * self.w;
            let c = self.x * self.y - self.z * self.w;
            let e = self.y * self.z + self.x * self.w;
            let y1 = a * e + b * c;
            let y2 = b * e - a * c;
            pitch = neg_one * clamp(x2, neg_one, T::ONE).asin();
            yaw = y1.atan2(y2);
            roll = T::ZERO;
        }
        // Contract: the returned components follow the same (pitch, yaw, roll)
        // order as [`from_euler_yxz`](Self::from_euler_yxz); YXZ rotation order.
        Vector3::new(pitch, yaw, roll)
    }

    /// Length.
    #[inline]
    pub fn length(&self) -> T {
        self.length_sqr().sqrt()
    }

    /// Normalizes to unit length, returning the identity when the input is
    /// a zero quaternion (no NaN).
    #[inline]
    pub fn normalize_or_zero(&self) -> Self {
        let len_sq = self.length_sqr();
        if len_sq <= T::ZERO {
            return Self::IDENTITY;
        }
        let inv = T::ONE / len_sq.sqrt();
        Self::new(self.x * inv, self.y * inv, self.z * inv, self.w * inv)
    }

    /// Inverse rotation. For a unit quaternion this equals the conjugate.
    #[inline]
    pub fn inverse(&self) -> Self {
        let len_sq = self.length_sqr();
        let inv = if len_sq > T::ZERO {
            T::ONE / len_sq
        } else {
            T::ONE
        };
        Self::new(-self.x * inv, -self.y * inv, -self.z * inv, self.w * inv)
    }

    /// Whether every component is finite.
    #[inline]
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite() && self.w.is_finite()
    }

    /// Approximate equality within `eps`.
    #[inline]
    pub fn approx_eq(&self, rhs: &Self, eps: T) -> bool {
        (self.x - rhs.x).abs() <= eps
            && (self.y - rhs.y).abs() <= eps
            && (self.z - rhs.z).abs() <= eps
            && (self.w - rhs.w).abs() <= eps
    }

    /// Rotates a vector by this (unit) quaternion.
    ///
    /// Equivalent to `Matrix4::from_quat(q).transform_vec3(v)`; assumes `q` is
    /// unit-length.
    #[inline]
    pub fn rotate_vec3(&self, v: Vector3<T>) -> Vector3<T> {
        let qv = self.xyz();
        let uv = qv.cross(&v) * (T::ONE + T::ONE);
        let uuv = qv.cross(&uv);
        v + uv * self.w + uuv
    }

    /// Spherical linear interpolation between two unit quaternions.
    #[inline]
    pub fn slerp(&self, other: &Self, t: T) -> Self {
        let cos_theta = self.dot(other);
        let mut other = *other;
        let mut cos_theta = cos_theta;
        // Take the shorter arc.
        if cos_theta < T::ZERO {
            cos_theta = -cos_theta;
            other = -other;
        }
        // Nearly parallel: fall back to nlerp to avoid acos(≈1) instability.
        if cos_theta > T::ONE - T::from_f32(1e-6) {
            return self.nlerp(&other, t);
        }
        let theta = cos_theta.acos();
        let sin_theta = theta.sin();
        let w1 = ((T::ONE - t) * theta).sin() / sin_theta;
        let w2 = (t * theta).sin() / sin_theta;
        (*self * w1) + (other * w2)
    }

    /// Normalized linear interpolation between two quaternions.
    #[inline]
    pub fn nlerp(&self, other: &Self, t: T) -> Self {
        let a = *self * (T::ONE - t);
        let b = *other * t;
        (a + b).normalize_or_zero()
    }

    /// Builds the quaternion that rotates `from` to `to` (both directions).
    ///
    /// Rotation-between-vectors semantics, including the
    /// opposite-vector and near-parallel edge cases.
    #[inline]
    pub fn from_to(from: Vector3<T>, to: Vector3<T>) -> Self {
        let f = from.normalize_or_zero();
        let t = to.normalize_or_zero();
        let d = f.dot(&t);
        let one = T::ONE;
        let eps = T::from_f32(1e-6);
        if d < -one + eps {
            // Opposite: rotate 180° about any axis perpendicular to `from`.
            let axis = if f.z.abs() < one - eps {
                Vector3::new(T::ZERO, T::ZERO, T::ONE).cross(&f)
            } else {
                Vector3::new(T::ONE, T::ZERO, T::ZERO).cross(&f)
            };
            return Self::from_axis_angle(
                axis.normalize_or_zero(),
                T::from_f32(std::f32::consts::PI),
            );
        }
        if d > one - eps {
            return Self::IDENTITY;
        }
        let axis = f.cross(&t);
        Self::new(axis.x, axis.y, axis.z, one + d).normalize_or_zero()
    }

    /// Converts to a [`Matrix3`] rotation matrix.
    #[inline]
    pub fn to_mat3(&self) -> Matrix3<T> {
        Matrix3::from_quat(self)
    }

    /// Converts to a [`Matrix4`] rotation matrix (zero translation).
    #[inline]
    pub fn to_mat4(&self) -> Matrix4<T> {
        Matrix4::from_quat(self)
    }

    /// Extracts a rotation quaternion from a [`Matrix4`].
    ///
    /// Uses only the upper-left 3×3 rotation part; matches the standard
    /// matrix extraction.
    #[inline]
    pub fn from_mat4(m: &Matrix4<T>) -> Self {
        let m00 = m.m[0][0];
        let m01 = m.m[0][1];
        let m02 = m.m[0][2];
        let m10 = m.m[1][0];
        let m11 = m.m[1][1];
        let m12 = m.m[1][2];
        let m20 = m.m[2][0];
        let m21 = m.m[2][1];
        let m22 = m.m[2][2];
        let trace = m00 + m11 + m22;
        let two = T::ONE + T::ONE;
        let four = two + two;

        if trace > T::ZERO {
            let s = (trace + T::ONE).sqrt() * two;
            // w known largest & positive; recover x, y, z from the w cross terms.
            let w = s / four;
            let x = (m12 - m21) / s;
            let y = (m20 - m02) / s;
            let z = (m01 - m10) / s;
            Self::new(x, y, z, w)
        } else if m00 > m11 && m00 > m22 {
            let s = (T::ONE + m00 - m11 - m22).sqrt() * two;
            // x largest; recover y, z from x terms and w from the x cross term.
            let x = s / four;
            let y = (m01 + m10) / s;
            let z = (m02 + m20) / s;
            let w = (m12 - m21) / s;
            Self::new(x, y, z, w)
        } else if m11 > m22 {
            let s = (T::ONE + m11 - m00 - m22).sqrt() * two;
            // y largest; recover x, z from y terms and w from the y cross term.
            let y = s / four;
            let x = (m01 + m10) / s;
            let z = (m12 + m21) / s;
            let w = (m20 - m02) / s;
            Self::new(x, y, z, w)
        } else {
            let s = (T::ONE + m22 - m00 - m11).sqrt() * two;
            // z largest; recover x, y from z terms and w from the z cross term.
            let z = s / four;
            let x = (m02 + m20) / s;
            let y = (m12 + m21) / s;
            let w = (m01 - m10) / s;
            Self::new(x, y, z, w)
        }
    }
}

fn clamp<T: ScalarNum>(v: T, lo: T, hi: T) -> T {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

impl<T: ScalarNum> Add for Quaternion<T> {
    type Output = Self;
    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
            w: self.w + rhs.w,
        }
    }
}

impl<T: ScalarNum> Mul<T> for Quaternion<T> {
    type Output = Self;
    #[inline]
    fn mul(self, s: T) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
            w: self.w * s,
        }
    }
}

impl<T: ScalarNum> Mul<Self> for Quaternion<T> {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        Self::mul(&self, &rhs)
    }
}

impl<T: ScalarNum> Neg for Quaternion<T> {
    type Output = Self;
    #[inline]
    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
            z: -self.z,
            w: -self.w,
        }
    }
}

/// `f32` quaternion.
pub type QuaternionF = Quaternion<f32>;

impl<T: fmt::Debug> fmt::Display for Quaternion<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Quaternion({:?}, {:?}, {:?}, {:?})",
            self.x, self.y, self.z, self.w
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matrix::{Matrix3, Matrix4F};
    use crate::vector::{Vector3, Vector3F};

    fn euler(deg: f32) -> f32 {
        deg * std::f32::consts::PI / 180.0
    }

    #[test]
    fn identity_and_axis_angle() {
        assert_eq!(QuaternionF::IDENTITY, QuaternionF::new(0.0, 0.0, 0.0, 1.0));
        let q = QuaternionF::from_axis_angle(Vector3::new(0.0, 0.0, 1.0), euler(90.0));
        // Rotating +X about +Z by 90° gives +Y.
        let v = q.rotate_vec3(Vector3::new(1.0, 0.0, 0.0));
        assert!(v.approx_eq(&Vector3::new(0.0, 1.0, 0.0), 1e-5));
    }

    #[test]
    fn euler_yxz_matches_matrix() {
        // Reference computed independently (f64): YXZ (0.3, 0.4, 0.5) -> standard
        // row-vector quaternion-to-matrix.
        let p = 0.3f64;
        let y = 0.4f64;
        let r = 0.5f64;
        let q = Quaternion::<f64>::from_euler_yxz(p, y, r);
        let m = Matrix3::<f64>::from_quat(&q);
        let expect = [
            [
                0.863_479_831_907_225_3,
                0.458_012_710_847_291_9,
                -0.211_250_885_422_487_2,
            ],
            [
                -0.340_587_093_988_493_6,
                0.838_386_643_594_203_6,
                0.425_568_169_922_656_17,
            ],
            [
                0.372_025_551_942_259_45,
                -0.295_520_206_661_339_44,
                0.879_923_176_281_257_2,
            ],
        ];
        for (i, row) in expect.iter().enumerate() {
            for (j, val) in row.iter().enumerate() {
                assert!(
                    (m.m[i][j] - val).abs() < 1e-9,
                    "mat[{i}][{j}] got {} want {}",
                    m.m[i][j],
                    val
                );
            }
        }
    }

    #[test]
    fn quat_mat_euler_roundtrip() {
        let angles = [
            (0.3f32, 0.4f32, 0.5f32),
            (euler(20.0), euler(35.0), euler(-15.0)),
        ];
        for (p, y, r) in angles {
            let q = QuaternionF::from_euler_yxz(p, y, r);
            let m = q.to_mat4();
            let q2 = QuaternionF::from_mat4(&m);
            let back = q2.to_euler_yxz();
            assert!(
                back.approx_eq(&Vector3::new(p, y, r), 1e-5),
                "roundtrip got {:?} want ({p},{y},{r})",
                back
            );
        }
    }

    #[test]
    fn rotate_vec3_matches_matrix() {
        let q = QuaternionF::from_euler_yxz(0.5, 0.2, 1.1);
        let v = Vector3::new(1.0, 2.0, 3.0);
        let a = q.rotate_vec3(v);
        let b = q.to_mat4().transform_vec3(v);
        assert!(a.approx_eq(&b, 1e-6));
        // Also matches the 3x3 matrix path.
        let c = q.to_mat3().mul_vec3(v);
        assert!(a.approx_eq(&c, 1e-6));
    }

    #[test]
    fn conjugate_inverse_and_mul() {
        let q = QuaternionF::from_euler_yxz(0.2, 0.4, 0.6).normalize_or_zero();
        let c = q.conjugate();
        assert!(c.approx_eq(&QuaternionF::new(-q.x, -q.y, -q.z, q.w), 1e-6));
        let inv = q.inverse();
        // q * q^-1 = identity (unit quaternion, so inverse = conjugate).
        let prod = q * inv;
        assert!(prod.approx_eq(&QuaternionF::IDENTITY, 1e-5));
        // dot of unit q with itself is 1.
        assert!((q.dot(&q) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn slerp_endpoints_and_nlerp() {
        let q0 = QuaternionF::from_euler_yxz(0.0, 0.0, 0.0);
        let q1 = QuaternionF::from_euler_yxz(0.6, 0.3, 0.9);
        let s0 = q0.slerp(&q1, 0.0);
        assert!(s0.approx_eq(&q0, 1e-5));
        let s1 = q0.slerp(&q1, 1.0);
        // slerp may snap to the equivalent -q1 on the short arc, so compare up
        // to sign.
        assert!(s1.approx_eq(&q1, 1e-5) || s1.approx_eq(&-q1, 1e-5));
        // nlerp midpoint is normalized and lies between.
        let m = q0.nlerp(&q1, 0.5);
        assert!((m.length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn from_to_builds_rotation() {
        let from = Vector3F::new(1.0, 0.0, 0.0);
        let to = Vector3F::new(0.0, 1.0, 0.0);
        let q = QuaternionF::from_to(from, to);
        let v = q.rotate_vec3(from.normalize_or_zero());
        assert!(v.approx_eq(&to, 1e-5), "got {:?}", v);
        // Opposite vectors must not produce NaN / zero.
        let q = QuaternionF::from_to(Vector3F::new(1.0, 0.0, 0.0), Vector3F::new(-1.0, 0.0, 0.0));
        let v = q.rotate_vec3(Vector3F::new(1.0, 0.0, 0.0));
        assert!(v.approx_eq(&Vector3F::new(-1.0, 0.0, 0.0), 1e-4) || v.length() > 0.5);
    }

    #[test]
    fn from_mat4_roundtrip_rotation() {
        let q = QuaternionF::from_euler_yxz(0.3, 0.7, -0.2);
        let m = Matrix4F::from_quat(&q);
        let q2 = QuaternionF::from_mat4(&m);
        // Same rotation (q or -q has the same matrix).
        let m2 = Matrix4F::from_quat(&q2);
        assert!(m2.approx_eq(&m, 1e-5));
    }

    #[test]
    fn display_and_is_finite() {
        let q = QuaternionF::IDENTITY;
        assert!(q.to_string().starts_with("Quaternion("));
        assert!(q.is_finite());
        assert!(!QuaternionF::new(0.0, f32::NAN, 0.0, 1.0).is_finite());
    }
}
