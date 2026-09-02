//! Local transform component (`Transform`).

use xengine_math::{QuaternionF, Vector3F};

use super::component::Component;

/// A game object's local TRS transform (relative to its parent).
///
/// The `rotate` field uses the crate-wide quaternion convention
/// (`xengine_math`, left-handed, `(x, y, z, w)` with `w` last). Negative
/// scale is allowed (mirroring). `Default` is position `ZERO`, rotation
/// `Identity`, scale `ONE`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Local translation.
    pub position: Vector3F,
    /// Local rotation (unit quaternion).
    pub rotate: QuaternionF,
    /// Local scale (may be negative for mirroring).
    pub scale: Vector3F,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Vector3F::ZERO,
            rotate: QuaternionF::IDENTITY,
            scale: Vector3F::ONE,
        }
    }
}

impl Component for Transform {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_zero_identity_one() {
        let t = Transform::default();
        assert_eq!(t.position, Vector3F::ZERO);
        assert_eq!(t.rotate, QuaternionF::IDENTITY);
        assert_eq!(t.scale, Vector3F::ONE);
    }
}
