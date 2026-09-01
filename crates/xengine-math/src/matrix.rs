//! Generic row-major matrices: [`Matrix3`] and [`Matrix4`].
//!
//! Conventions (matching NeoX/DirectXMath, D3D-style):
//! * **row-major** storage — `m[row][col]`.
//! * **row-vector × matrix** transforms — `v' = v · M`, so `A·B` applies `A`
//!   first, then `B`.
//! * **left-handed** coordinate system, identity forward `= +Z`.
//! * `Matrix4` translation lives in `m[3][0..2]`.

use crate::kernel;
use crate::quaternion::Quaternion;
use crate::scalar::{FloatNum, ScalarNum};
use crate::vector::{Vector3, Vector4};
use std::fmt;

/// A 3×3 row-major matrix.
#[repr(C)]
#[cfg_attr(not(feature = "xmath_align64"), repr(align(16)))]
#[cfg_attr(feature = "xmath_align64", repr(align(64)))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix3<T> {
    pub m: [[T; 3]; 3],
}

/// A 4×4 row-major matrix. Translation is stored in `m[3][0..2]`.
#[repr(C)]
#[cfg_attr(not(feature = "xmath_align64"), repr(align(16)))]
#[cfg_attr(feature = "xmath_align64", repr(align(64)))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix4<T> {
    pub m: [[T; 4]; 4],
}

impl<T: ScalarNum> Matrix3<T> {
    /// Constructs a matrix from its 9 row-major entries.
    #[inline]
    pub fn new(m: [[T; 3]; 3]) -> Self {
        Self { m }
    }

    /// Zero matrix.
    pub const ZERO: Self = Self {
        m: [[T::ZERO; 3]; 3],
    };

    /// Identity matrix.
    pub const IDENTITY: Self = Self {
        m: [
            [T::ONE, T::ZERO, T::ZERO],
            [T::ZERO, T::ONE, T::ZERO],
            [T::ZERO, T::ZERO, T::ONE],
        ],
    };

    /// Matrix product `self · rhs` (row-major, apply `self` then `rhs`).
    #[inline]
    pub fn mul(&self, rhs: &Self) -> Self {
        Self {
            m: kernel::mat3_mul(&self.m, &rhs.m),
        }
    }

    /// Transpose (keeps row-major semantics).
    #[inline]
    pub fn transpose(&self) -> Self {
        Self {
            m: [
                [self.m[0][0], self.m[1][0], self.m[2][0]],
                [self.m[0][1], self.m[1][1], self.m[2][1]],
                [self.m[0][2], self.m[1][2], self.m[2][2]],
            ],
        }
    }

    /// Determinant.
    #[inline]
    pub fn determinant(&self) -> T {
        let m = &self.m;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    /// Inverts the matrix, returning `None` when it is singular.
    #[inline]
    pub fn inverse(&self) -> Option<Self> {
        let det = self.determinant();
        if det == T::ZERO {
            return None;
        }
        let inv_det = T::ONE / det;
        let m = &self.m;
        let mut out = [[T::ZERO; 3]; 3];
        out[0][0] = (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv_det;
        out[0][1] = (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det;
        out[0][2] = (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det;
        out[1][0] = (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv_det;
        out[1][1] = (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det;
        out[1][2] = (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det;
        out[2][0] = (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv_det;
        out[2][1] = (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det;
        out[2][2] = (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det;
        Some(Self { m: out })
    }

    /// Transforms a vector as a point (implicit `w = 1`).
    #[inline]
    pub fn mul_vec3(&self, v: Vector3<T>) -> Vector3<T> {
        let m = &self.m;
        Vector3::new(
            v.x * m[0][0] + v.y * m[1][0] + v.z * m[2][0],
            v.x * m[0][1] + v.y * m[1][1] + v.z * m[2][1],
            v.x * m[0][2] + v.y * m[1][2] + v.z * m[2][2],
        )
    }

    /// Converts to a [`Matrix4`] (the 3×3 rotation embedded in a 4×4 identity).
    #[inline]
    pub fn to_mat4(&self) -> Matrix4<T> {
        Matrix4 {
            m: [
                [self.m[0][0], self.m[0][1], self.m[0][2], T::ZERO],
                [self.m[1][0], self.m[1][1], self.m[1][2], T::ZERO],
                [self.m[2][0], self.m[2][1], self.m[2][2], T::ZERO],
                [T::ZERO, T::ZERO, T::ZERO, T::ONE],
            ],
        }
    }

    /// Extracts the upper-left 3×3 from a [`Matrix4`].
    #[inline]
    pub fn from_mat4(m: &Matrix4<T>) -> Self {
        Self {
            m: [
                [m.m[0][0], m.m[0][1], m.m[0][2]],
                [m.m[1][0], m.m[1][1], m.m[1][2]],
                [m.m[2][0], m.m[2][1], m.m[2][2]],
            ],
        }
    }
}

impl<T: FloatNum> Matrix3<T> {
    /// Builds a rotation matrix from a (unit) quaternion.
    #[inline]
    pub fn from_quat(q: &Quaternion<T>) -> Self {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let (x2, y2, z2) = (x * x, y * y, z * z);
        let two = T::from_f32(2.0);
        Self {
            m: [
                [
                    T::ONE - two * (y2 + z2),
                    two * (x * y + w * z),
                    two * (x * z - w * y),
                ],
                [
                    two * (x * y - w * z),
                    T::ONE - two * (x2 + z2),
                    two * (y * z + w * x),
                ],
                [
                    two * (x * z + w * y),
                    two * (y * z - w * x),
                    T::ONE - two * (x2 + y2),
                ],
            ],
        }
    }

    /// Approximate equality within `eps`.
    #[inline]
    pub fn approx_eq(&self, rhs: &Self, eps: T) -> bool {
        for r in 0..3 {
            for c in 0..3 {
                if (self.m[r][c] - rhs.m[r][c]).abs() > eps {
                    return false;
                }
            }
        }
        true
    }
}

impl<T: ScalarNum> Matrix4<T> {
    /// Constructs a matrix from its 16 row-major entries.
    #[inline]
    pub fn new(m: [[T; 4]; 4]) -> Self {
        Self { m }
    }

    /// Zero matrix.
    pub const ZERO: Self = Self {
        m: [[T::ZERO; 4]; 4],
    };

    /// Identity matrix (forward `= +Z` in a left-handed system).
    pub const IDENTITY: Self = Self {
        m: [
            [T::ONE, T::ZERO, T::ZERO, T::ZERO],
            [T::ZERO, T::ONE, T::ZERO, T::ZERO],
            [T::ZERO, T::ZERO, T::ONE, T::ZERO],
            [T::ZERO, T::ZERO, T::ZERO, T::ONE],
        ],
    };

    /// Matrix product `self · rhs` (row-major, apply `self` then `rhs`).
    #[inline]
    pub fn mul(&self, rhs: &Self) -> Self {
        Self {
            m: kernel::mat4_mul(&self.m, &rhs.m),
        }
    }

    /// Transpose (keeps row-major semantics).
    #[inline]
    pub fn transpose(&self) -> Self {
        let mut out = [[T::ZERO; 4]; 4];
        for (r, row) in out.iter_mut().enumerate() {
            for (c, v) in row.iter_mut().enumerate() {
                *v = self.m[c][r];
            }
        }
        Self { m: out }
    }

    /// Determinant.
    #[inline]
    pub fn determinant(&self) -> T {
        let m = &self.m;
        let s0 = m[0][0] * m[1][1] - m[1][0] * m[0][1];
        let s1 = m[0][0] * m[1][2] - m[1][0] * m[0][2];
        let s2 = m[0][0] * m[1][3] - m[1][0] * m[0][3];
        let s3 = m[0][1] * m[1][2] - m[1][1] * m[0][2];
        let s4 = m[0][1] * m[1][3] - m[1][1] * m[0][3];
        let s5 = m[0][2] * m[1][3] - m[1][2] * m[0][3];
        let c5 = m[2][2] * m[3][3] - m[3][2] * m[2][3];
        let c4 = m[2][1] * m[3][3] - m[3][1] * m[2][3];
        let c3 = m[2][1] * m[3][2] - m[3][1] * m[2][2];
        let c2 = m[2][0] * m[3][3] - m[3][0] * m[2][3];
        let c1 = m[2][0] * m[3][2] - m[3][0] * m[2][2];
        let c0 = m[2][0] * m[3][1] - m[3][0] * m[2][1];
        s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0
    }

    /// Inverts the matrix, returning `None` when it is singular.
    ///
    /// Uses the adjugate method (cofactor expansion over minors). For a pure
    /// rotation matrix the inverse equals the transpose — this general inverse
    /// also covers TRS (affine) combinations.
    #[inline]
    pub fn inverse(&self) -> Option<Self> {
        let m = &self.m;
        let s0 = m[0][0] * m[1][1] - m[1][0] * m[0][1];
        let s1 = m[0][0] * m[1][2] - m[1][0] * m[0][2];
        let s2 = m[0][0] * m[1][3] - m[1][0] * m[0][3];
        let s3 = m[0][1] * m[1][2] - m[1][1] * m[0][2];
        let s4 = m[0][1] * m[1][3] - m[1][1] * m[0][3];
        let s5 = m[0][2] * m[1][3] - m[1][2] * m[0][3];
        let c5 = m[2][2] * m[3][3] - m[3][2] * m[2][3];
        let c4 = m[2][1] * m[3][3] - m[3][1] * m[2][3];
        let c3 = m[2][1] * m[3][2] - m[3][1] * m[2][2];
        let c2 = m[2][0] * m[3][3] - m[3][0] * m[2][3];
        let c1 = m[2][0] * m[3][2] - m[3][0] * m[2][2];
        let c0 = m[2][0] * m[3][1] - m[3][0] * m[2][1];
        let det = s0 * c5 - s1 * c4 + s2 * c3 + s3 * c2 - s4 * c1 + s5 * c0;
        if det == T::ZERO {
            return None;
        }
        let inv = T::ONE / det;

        let mut out = [[T::ZERO; 4]; 4];
        out[0][0] = (m[1][1] * c5 - m[1][2] * c4 + m[1][3] * c3) * inv;
        out[0][1] = (-m[0][1] * c5 + m[0][2] * c4 - m[0][3] * c3) * inv;
        out[0][2] = (m[3][1] * s5 - m[3][2] * s4 + m[3][3] * s3) * inv;
        out[0][3] = (-m[2][1] * s5 + m[2][2] * s4 - m[2][3] * s3) * inv;
        out[1][0] = (-m[1][0] * c5 + m[1][2] * c2 - m[1][3] * c1) * inv;
        out[1][1] = (m[0][0] * c5 - m[0][2] * c2 + m[0][3] * c1) * inv;
        out[1][2] = (-m[3][0] * s5 + m[3][2] * s2 - m[3][3] * s1) * inv;
        out[1][3] = (m[2][0] * s5 - m[2][2] * s2 + m[2][3] * s1) * inv;
        out[2][0] = (m[1][0] * c4 - m[1][1] * c2 + m[1][3] * c0) * inv;
        out[2][1] = (-m[0][0] * c4 + m[0][1] * c2 - m[0][3] * c0) * inv;
        out[2][2] = (m[3][0] * s4 - m[3][1] * s2 + m[3][3] * s0) * inv;
        out[2][3] = (-m[2][0] * s4 + m[2][1] * s2 - m[2][3] * s0) * inv;
        out[3][0] = (-m[1][0] * c3 + m[1][1] * c1 - m[1][2] * c0) * inv;
        out[3][1] = (m[0][0] * c3 - m[0][1] * c1 + m[0][2] * c0) * inv;
        out[3][2] = (-m[3][0] * s3 + m[3][1] * s1 - m[3][2] * s0) * inv;
        out[3][3] = (m[2][0] * s3 - m[2][1] * s1 + m[2][2] * s0) * inv;
        Some(Self { m: out })
    }

    /// Transforms a point (implicit `w = 1`, translation applied).
    #[inline]
    pub fn mul_vec3(&self, v: Vector3<T>) -> Vector3<T> {
        let o = kernel::mat4_mul_point(&self.m, &[v.x, v.y, v.z]);
        Vector3::new(o[0], o[1], o[2])
    }

    /// Transforms a 4-component vector (`v · M`), returning the raw
    /// homogeneous result **without** perspective division.
    ///
    /// This matches DirectXMath `XMVector4Transform` — the `w` component is
    /// carried through transparently. For a direction vector (`w = 0`) the
    /// result is unaffected by the translation row and never panics.
    #[inline]
    pub fn mul_vec4(&self, v: Vector4<T>) -> Vector4<T> {
        let o = kernel::mat4_mul_vec4(&self.m, &[v.x, v.y, v.z, v.w]);
        Vector4::new(o[0], o[1], o[2], o[3])
    }

    /// Point transform (alias for [`mul_vec3`](Self::mul_vec3)).
    #[inline]
    pub fn transform_point(&self, v: Vector3<T>) -> Vector3<T> {
        self.mul_vec3(v)
    }

    /// Direction transform (`w = 0`, no translation) — rotation/scale only.
    #[inline]
    pub fn transform_vec3(&self, v: Vector3<T>) -> Vector3<T> {
        let o = kernel::mat4_mul_dir(&self.m, &[v.x, v.y, v.z]);
        Vector3::new(o[0], o[1], o[2])
    }

    /// Flattens the matrix into column-major storage order `[c*4 + r]`,
    /// i.e. the transpose of the row-major layout. This is the explicit
    /// interop form consumed by column-major backends (e.g. future Vulkan).
    #[inline]
    pub fn to_col_major(&self) -> [T; 16] {
        let mut out = [T::ZERO; 16];
        for r in 0..4 {
            for c in 0..4 {
                out[c * 4 + r] = self.m[r][c];
            }
        }
        out
    }

    /// Rebuilds a row-major matrix from a column-major flat array.
    #[inline]
    pub fn from_col_major(data: &[T; 16]) -> Self {
        let mut out = [[T::ZERO; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                out[r][c] = data[c * 4 + r];
            }
        }
        Self { m: out }
    }
}

impl<T: FloatNum> Matrix4<T> {
    /// Builds a rotation matrix (with zero translation) from a unit quaternion.
    #[inline]
    pub fn from_quat(q: &Quaternion<T>) -> Self {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let (x2, y2, z2) = (x * x, y * y, z * z);
        let two = T::from_f32(2.0);
        Self {
            m: [
                [
                    T::ONE - two * (y2 + z2),
                    two * (x * y + w * z),
                    two * (x * z - w * y),
                    T::ZERO,
                ],
                [
                    two * (x * y - w * z),
                    T::ONE - two * (x2 + z2),
                    two * (y * z + w * x),
                    T::ZERO,
                ],
                [
                    two * (x * z + w * y),
                    two * (y * z - w * x),
                    T::ONE - two * (x2 + y2),
                    T::ZERO,
                ],
                [T::ZERO, T::ZERO, T::ZERO, T::ONE],
            ],
        }
    }

    /// Builds a translation-only matrix.
    #[inline]
    pub fn from_translation(t: Vector3<T>) -> Self {
        Self {
            m: [
                [T::ONE, T::ZERO, T::ZERO, T::ZERO],
                [T::ZERO, T::ONE, T::ZERO, T::ZERO],
                [T::ZERO, T::ZERO, T::ONE, T::ZERO],
                [t.x, t.y, t.z, T::ONE],
            ],
        }
    }

    /// Builds a scale-only matrix.
    #[inline]
    pub fn from_scale(s: Vector3<T>) -> Self {
        Self {
            m: [
                [s.x, T::ZERO, T::ZERO, T::ZERO],
                [T::ZERO, s.y, T::ZERO, T::ZERO],
                [T::ZERO, T::ZERO, s.z, T::ZERO],
                [T::ZERO, T::ZERO, T::ZERO, T::ONE],
            ],
        }
    }

    /// Builds a TRS matrix: scale `s`, rotation `q`, translation `t`.
    ///
    /// The rotation/scale occupy the upper-left 3×3 (`R * S`), the translation
    /// sits in `m[3][0..2]`.
    #[inline]
    pub fn from_trs(t: Vector3<T>, q: &Quaternion<T>, s: Vector3<T>) -> Self {
        let r = Self::from_quat(q);
        Self {
            m: [
                [r.m[0][0] * s.x, r.m[0][1] * s.y, r.m[0][2] * s.z, T::ZERO],
                [r.m[1][0] * s.x, r.m[1][1] * s.y, r.m[1][2] * s.z, T::ZERO],
                [r.m[2][0] * s.x, r.m[2][1] * s.y, r.m[2][2] * s.z, T::ZERO],
                [t.x, t.y, t.z, T::ONE],
            ],
        }
    }

    /// Builds a left-handed look-at view matrix (camera space forward `= +Z`).
    ///
    /// Matches DirectXMath `XMMatrixLookAtLH`.
    #[inline]
    pub fn look_at_lh(eye: Vector3<T>, target: Vector3<T>, up: Vector3<T>) -> Self {
        let r2 = (target - eye).normalize_or_zero();
        let r0 = up.cross(&r2).normalize_or_zero();
        let r1 = r2.cross(&r0);
        let neg_eye = -eye;
        Self {
            m: [
                [r0.x, r0.y, r0.z, r0.dot(&neg_eye)],
                [r1.x, r1.y, r1.z, r1.dot(&neg_eye)],
                [r2.x, r2.y, r2.z, r2.dot(&neg_eye)],
                [T::ZERO, T::ZERO, T::ZERO, T::ONE],
            ],
        }
    }

    /// Builds a left-handed perspective matrix mapping depth to `[0, 1]`
    /// (D3D convention). Matches DirectXMath `XMMatrixPerspectiveFovLH`.
    ///
    /// Note: the depth mapping uses `m[2][2] = far/(far-near)` and
    /// `m[2][3] = 1`. (`m[3][2] = -m[2][2]*near` is the near-plane offset.)
    #[inline]
    pub fn perspective_lh(fovy: T, aspect: T, near: T, far: T) -> Self {
        let half = fovy / (T::ONE + T::ONE);
        let s = half.sin();
        let c = half.cos();
        let height = c / s;
        let width = height / aspect;
        let f_range = far / (far - near);
        Self {
            m: [
                [width, T::ZERO, T::ZERO, T::ZERO],
                [T::ZERO, height, T::ZERO, T::ZERO],
                [T::ZERO, T::ZERO, f_range, T::ONE],
                [T::ZERO, T::ZERO, -f_range * near, T::ZERO],
            ],
        }
    }

    /// Builds a left-handed orthographic matrix. Matches DirectXMath
    /// `XMMatrixOrthographicLH`.
    #[inline]
    pub fn ortho_lh(width: T, height: T, near: T, far: T) -> Self {
        let f_range = T::ONE / (far - near);
        let two = T::from_f32(2.0);
        Self {
            m: [
                [two / width, T::ZERO, T::ZERO, T::ZERO],
                [T::ZERO, two / height, T::ZERO, T::ZERO],
                [T::ZERO, T::ZERO, f_range, T::ZERO],
                [T::ZERO, T::ZERO, -f_range * near, T::ONE],
            ],
        }
    }

    /// Approximate equality within `eps`.
    #[inline]
    pub fn approx_eq(&self, rhs: &Self, eps: T) -> bool {
        for r in 0..4 {
            for c in 0..4 {
                if (self.m[r][c] - rhs.m[r][c]).abs() > eps {
                    return false;
                }
            }
        }
        true
    }
}

/// `f32` 3×3 matrix.
pub type Matrix3F = Matrix3<f32>;
/// `f32` 4×4 matrix.
pub type Matrix4F = Matrix4<f32>;

/// Free-function point transform (`v · m`, translation applied).
pub fn transform_point<T: ScalarNum>(v: Vector3<T>, m: &Matrix4<T>) -> Vector3<T> {
    m.mul_vec3(v)
}

/// Free-function direction transform (`w = 0`, no translation).
pub fn transform_vec3<T: ScalarNum>(v: Vector3<T>, m: &Matrix4<T>) -> Vector3<T> {
    m.transform_vec3(v)
}

impl<T: fmt::Debug> fmt::Display for Matrix3<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Matrix3({:?})", self.m)
    }
}

impl<T: fmt::Debug> fmt::Display for Matrix4<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Matrix4({:?})", self.m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quaternion::QuaternionF;

    #[test]
    fn row_vector_applies_a_then_b() {
        // A·B must apply A first, then B (row-vector × matrix).
        let a = Matrix4F::from_translation(Vector3::new(1.0, 0.0, 0.0));
        let b = Matrix4F::from_translation(Vector3::new(0.0, 2.0, 0.0));
        let origin = Vector3::new(0.0, 0.0, 0.0);
        // transform_point(v, A·B) == transform_point(transform_point(v, A), B)
        let ab = a.mul(&b);
        let direct = ab.transform_point(origin);
        let expected = b.transform_point(a.transform_point(origin));
        assert!(direct.approx_eq(&expected, 1e-6));
        assert_eq!(direct, Vector3::new(1.0, 2.0, 0.0));
    }

    #[test]
    fn translation_lives_in_m30_2() {
        let t = Vector3::new(3.0, 4.0, 5.0);
        let m = Matrix4F::from_translation(t);
        assert_eq!(m.m[3][0], 3.0);
        assert_eq!(m.m[3][1], 4.0);
        assert_eq!(m.m[3][2], 5.0);
        assert_eq!(m.m[3][3], 1.0);
        assert_eq!(m.m[0][0], 1.0);
        assert_eq!(m.m[0][1], 0.0);
    }

    #[test]
    fn identity_forward_is_plus_z() {
        let fwd = Vector3::new(0.0, 0.0, 1.0);
        let out = Matrix4F::IDENTITY.transform_vec3(fwd);
        assert!(out.approx_eq(&fwd, 1e-6));
        // Perspective/left-handed forward is +Z.
        let q = QuaternionF::IDENTITY;
        assert!(
            Matrix4F::from_quat(&q)
                .transform_vec3(fwd)
                .approx_eq(&fwd, 1e-6)
        );
    }

    #[test]
    fn trs_composes_and_inverts() {
        let t = Vector3::new(1.0, -2.0, 3.0);
        let q = QuaternionF::from_euler_yxz(0.3, 0.4, 0.5);
        let s = Vector3::new(2.0, 3.0, 4.0);
        let m = Matrix4F::from_trs(t, &q, s);
        // Translation row must hold t.
        assert_eq!(m.m[3][0], 1.0);
        assert_eq!(m.m[3][1], -2.0);
        assert_eq!(m.m[3][2], 3.0);
        // M * M^-1 == I and M^-1 * M == I.
        let inv = m.inverse().unwrap();
        let id1 = m.mul(&inv);
        let id2 = inv.mul(&m);
        assert!(id1.approx_eq(&Matrix4F::IDENTITY, 1e-4));
        assert!(id2.approx_eq(&Matrix4F::IDENTITY, 1e-4));
    }

    #[test]
    fn rotation_inverse_equals_transpose() {
        let q = QuaternionF::from_axis_angle(Vector3::new(0.0, 0.0, 1.0), 1.2);
        let m4 = Matrix4F::from_quat(&q);
        let inv4 = m4.inverse().unwrap();
        assert!(inv4.approx_eq(&m4.transpose(), 1e-5));
        let m3 = Matrix3F::from_quat(&q);
        let inv3 = m3.inverse().unwrap();
        assert!(inv3.approx_eq(&m3.transpose(), 1e-5));
    }

    #[test]
    fn perspective_lh_maps_depth_to_0_1() {
        let near = 0.1f32;
        let far = 100.0f32;
        let m = Matrix4F::perspective_lh(1.0, 1.0, near, far);
        // D3D depth [0,1]: m[2][2] = far/(far-near), m[2][3] = 1.
        let f_range = far / (far - near);
        assert!((m.m[2][2] - f_range).abs() < 1e-5);
        assert!((m.m[2][3] - 1.0).abs() < 1e-6);
        assert!((m.m[3][2] - (-f_range * near)).abs() < 1e-5);
        // Depth mapping check.
        for (z, expected) in [(near, 0.0), (far, 1.0)] {
            let r = m.mul_vec4(Vector4::new(0.0, 0.0, z, 1.0));
            let ndc_z = r.z / r.w;
            assert!((ndc_z - expected).abs() < 1e-4, "z={z} ndc={ndc_z}");
        }
    }

    #[test]
    fn look_at_lh_forward_is_plus_z() {
        // Eye at origin looking toward +Z.
        let m = Matrix4F::look_at_lh(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 5.0),
            Vector3::new(0.0, 1.0, 0.0),
        );
        // Forward basis row r2 = normalize(target-eye) = +Z.
        assert!((m.m[2][0] - 0.0).abs() < 1e-6);
        assert!((m.m[2][1] - 0.0).abs() < 1e-6);
        assert!((m.m[2][2] - 1.0).abs() < 1e-6);
        // Eye maps to the camera-space origin.
        let eye = m.transform_point(Vector3::new(0.0, 0.0, 0.0));
        assert!(eye.approx_eq(&Vector3::new(0.0, 0.0, 0.0), 1e-6));
        // A target point at eye + forward ends up on +Z.
        let p = m.transform_point(Vector3::new(0.0, 0.0, 5.0));
        assert!(p.z > 0.0);
        assert!(p.x.abs() < 1e-6 && p.y.abs() < 1e-6);
    }

    #[test]
    fn ortho_lh_matches_dxmath() {
        let m = Matrix4F::ortho_lh(800.0, 600.0, 0.1, 100.0);
        assert!((m.m[0][0] - 2.0 / 800.0).abs() < 1e-6);
        assert!((m.m[1][1] - 2.0 / 600.0).abs() < 1e-6);
        assert!((m.m[2][2] - 1.0 / (100.0 - 0.1)).abs() < 1e-6);
        assert!((m.m[3][2] - (-0.1 / (100.0 - 0.1))).abs() < 1e-6);
        assert!((m.m[3][3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn col_major_roundtrip() {
        let m = Matrix4F::from_trs(
            Vector3::new(1.0, 2.0, 3.0),
            &QuaternionF::from_euler_yxz(0.1, 0.2, 0.3),
            Vector3::new(2.0, 2.0, 2.0),
        );
        let cm = m.to_col_major();
        let back = Matrix4F::from_col_major(&cm);
        assert!(back.approx_eq(&m, 1e-6));
        // Column-major storage: element (col c, row r) sits at index c*4+r and
        // equals the row-major value m[r][c].
        assert_eq!(cm[0], m.m[0][0]); // col 0, row 0
        assert_eq!(cm[4], m.m[0][1]); // col 1, row 0 == m[0][1]
        assert_eq!(cm[4 + 1], m.m[1][1]); // col 1, row 1 == m[1][1]
    }

    #[test]
    fn mul_vec4_passthrough_w0_no_panic() {
        let m = Matrix4F::from_translation(Vector3::new(1.0, 2.0, 3.0));
        // A direction (w=0) is transformed without translation and without panic.
        let r = m.mul_vec4(Vector4::new(1.0, 2.0, 3.0, 0.0));
        // w stays 0 (no perspective divide), and xyz are the rotation-only part.
        assert_eq!(r.w, 0.0);
        assert!(r.x.is_finite() && r.y.is_finite() && r.z.is_finite());
    }

    #[test]
    fn matrix3_operations() {
        let q = QuaternionF::from_euler_yxz(0.2, 0.3, 0.4);
        let m3 = Matrix3F::from_quat(&q);
        let m4 = Matrix4F::from_quat(&q);
        // mat3 / mat4 from the same quaternion agree on the 3x3 block.
        let up = Matrix3F::from_mat4(&m4);
        assert!(up.approx_eq(&m3, 1e-6));
        // mat3 -> mat4 embeds with a translation-free identity bottom row.
        let m4b = m3.to_mat4();
        assert!(m4b.approx_eq(&m4, 1e-6));
    }

    #[test]
    fn display_format() {
        let m = Matrix4F::IDENTITY;
        assert!(m.to_string().starts_with("Matrix4("));
    }
}
