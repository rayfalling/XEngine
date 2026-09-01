//! The script-facing GO wrapper: a generational, location-cached handle that
//! validates before every access and never holds a bare pointer.

use std::fmt;

use crate::World;
use crate::entity::Entity;

use super::hierarchy::{Children, Parent};
use super::scene_ref::SceneRef;
use super::transform::Transform;

/// Cached data position of a game object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoLoc {
    /// Archetype id holding the entity.
    pub arch: usize,
    /// Row within the archetype.
    pub row: u32,
    /// Generation mirror, to detect slot reuse after destroy.
    pub generation: u32,
}

/// A script-identifiable handle to a game object.
///
/// Caches the data position so the stable access path is O(1); every access
/// is validated by generation + position and re-resolved on a mismatch. It
/// never holds a bare pointer (no cross-frame pointer caching).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GoHandle {
    pub(crate) entity: Entity,
    pub(crate) loc: Option<GoLoc>,
}

impl GoHandle {
    /// The underlying entity.
    pub fn entity(&self) -> Entity {
        self.entity
    }
}

impl fmt::Display for GoHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "GoHandle({}@{:?})",
            self.entity.index(),
            self.loc.as_ref().map(|l| (l.arch, l.row, l.generation))
        )
    }
}

/// Errors from [`crate::go::scene::Scene::go_view`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoHandleError {
    /// The entity was destroyed (or the handle's generation no longer matches).
    GoHandleStale,
}

impl fmt::Display for GoHandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GoHandleStale => write!(f, "go handle is stale (entity destroyed)"),
        }
    }
}

impl std::error::Error for GoHandleError {}

/// A borrowed view over a game object's component trio (no bare pointers).
///
/// Constructed by `Scene::go_view`: it validates the cached location and reuses
/// it for O(1) component access (or re-resolves on a stale location). Each
/// accessor re-derives the reference from the validated position.
pub struct GoView<'a> {
    pub(crate) world: &'a mut World,
    pub(crate) loc: GoLoc,
    pub(crate) entity: Entity,
}

impl<'a> GoView<'a> {
    /// The entity backing this view.
    pub fn entity(&self) -> Entity {
        self.entity
    }

    /// Mutable access to the local transform.
    pub fn transform(&mut self) -> &mut Transform {
        self.world
            .get_mut_at::<Transform>(self.loc.arch, self.loc.row as usize)
            .expect("go_view transform (validated position)")
    }

    /// Shared access to the scene reference relation.
    pub fn scene_ref(&self) -> &SceneRef {
        self.world
            .get_at::<SceneRef>(self.loc.arch, self.loc.row as usize)
            .expect("go_view scene_ref (validated position)")
    }

    /// Shared access to the parent edge.
    pub fn parent(&self) -> &Parent {
        self.world
            .get_at::<Parent>(self.loc.arch, self.loc.row as usize)
            .expect("go_view parent (validated position)")
    }

    /// Shared access to the children edge (may be absent for a leaf).
    pub fn children(&self) -> Option<&Children> {
        self.world
            .get_at::<Children>(self.loc.arch, self.loc.row as usize)
    }
}

#[cfg(test)]
mod tests {
    use crate::go::scene::Scene;
    use crate::go::transform::Transform;

    #[test]
    fn handle_display_and_entity() {
        let mut scene = Scene::new();
        let e = scene.create_go(Transform::default()).unwrap();
        let handle = scene.go_handle(e);
        assert_eq!(handle.entity(), e);
        let text = handle.to_string();
        assert!(text.starts_with("GoHandle("));
    }
}
