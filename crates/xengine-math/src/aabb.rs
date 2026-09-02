//! Generic axis-aligned bounding box primitive.

use crate::scalar::{FloatNum, ScalarNum};
use crate::vector::Vector3;
use std::fmt;

/// An axis-aligned bounding box defined by its `min` and `max` corners.
///
/// A box with any `min > max` component is *degenerate* (empty); all queries
/// treat it as empty and never panic.
#[repr(C)]
#[cfg_attr(not(feature = "xmath_align64"), repr(align(16)))]
#[cfg_attr(feature = "xmath_align64", repr(align(64)))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AABB<T> {
    pub min: Vector3<T>,
    pub max: Vector3<T>,
}

impl<T: ScalarNum> AABB<T> {
    /// Builds a box from its min and max corners.
    #[inline]
    pub fn new(min: Vector3<T>, max: Vector3<T>) -> Self {
        Self { min, max }
    }

    /// Alias for [`new`](Self::new), matching the spec operation name.
    #[inline]
    pub fn from_min_max(min: Vector3<T>, max: Vector3<T>) -> Self {
        Self { min, max }
    }

    /// Whether the box is degenerate (empty): any component has `min > max`.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    /// The union of two boxes (the smallest box containing both).
    #[inline]
    pub fn union(&self, other: &Self) -> Self {
        Self::new(
            Vector3::new(
                if self.min.x < other.min.x {
                    self.min.x
                } else {
                    other.min.x
                },
                if self.min.y < other.min.y {
                    self.min.y
                } else {
                    other.min.y
                },
                if self.min.z < other.min.z {
                    self.min.z
                } else {
                    other.min.z
                },
            ),
            Vector3::new(
                if self.max.x > other.max.x {
                    self.max.x
                } else {
                    other.max.x
                },
                if self.max.y > other.max.y {
                    self.max.y
                } else {
                    other.max.y
                },
                if self.max.z > other.max.z {
                    self.max.z
                } else {
                    other.max.z
                },
            ),
        )
    }

    /// Whether this box and `other` overlap on every axis.
    ///
    /// Degenerate (empty) boxes never intersect.
    #[inline]
    pub fn intersects(&self, other: &Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }
        self.min.x <= other.max.x
            && other.min.x <= self.max.x
            && self.min.y <= other.max.y
            && other.min.y <= self.max.y
            && self.min.z <= other.max.z
            && other.min.z <= self.max.z
    }

    /// Whether the box contains a point (inclusive).
    ///
    /// Degenerate (empty) boxes never contain a point.
    #[inline]
    pub fn contains(&self, p: Vector3<T>) -> bool {
        if self.is_empty() {
            return false;
        }
        self.min.x <= p.x
            && p.x <= self.max.x
            && self.min.y <= p.y
            && p.y <= self.max.y
            && self.min.z <= p.z
            && p.z <= self.max.z
    }
}

impl<T: FloatNum> AABB<T> {
    /// Approximate equality within `eps`.
    #[inline]
    pub fn approx_eq(&self, rhs: &Self, eps: T) -> bool {
        self.min.approx_eq(&rhs.min, eps) && self.max.approx_eq(&rhs.max, eps)
    }
}

/// `f32` axis-aligned bounding box.
#[allow(clippy::upper_case_acronyms)]
pub type AABBF = AABB<f32>;

impl<T: fmt::Debug> fmt::Display for AABB<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AABB({:?}, {:?})", self.min, self.max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_intersect_contain() {
        let a = AABBF::from_min_max(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0));
        let b = AABBF::from_min_max(Vector3::new(0.5, 0.5, 0.5), Vector3::new(2.0, 2.0, 2.0));
        assert!(a.intersects(&b));
        let u = a.union(&b);
        assert_eq!(u.min, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(u.max, Vector3::new(2.0, 2.0, 2.0));
        assert!(a.contains(Vector3::new(0.5, 0.5, 0.5)));
        assert!(!a.contains(Vector3::new(1.5, 0.5, 0.5)));
        // Disjoint boxes do not intersect.
        let c = AABBF::from_min_max(Vector3::new(5.0, 5.0, 5.0), Vector3::new(6.0, 6.0, 6.0));
        assert!(!a.intersects(&c));
    }

    #[test]
    fn degenerate_and_empty_semantics() {
        // min > max is empty.
        let empty = AABBF::from_min_max(Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 1.0));
        assert!(empty.is_empty());
        assert!(!empty.intersects(&AABBF::from_min_max(
            Vector3::new(0.0, 0.0, 0.0),
            Vector3::new(2.0, 2.0, 2.0)
        )));
        assert!(!empty.contains(Vector3::new(0.5, 0.5, 0.5)));
        // Union with an empty box returns the other box.
        let a = AABBF::from_min_max(Vector3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 1.0, 1.0));
        let u = a.union(&empty);
        assert_eq!(u.min, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(u.max, Vector3::new(1.0, 1.0, 1.0));
    }
}
