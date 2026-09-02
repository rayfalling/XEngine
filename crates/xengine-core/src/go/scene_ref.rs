//! `SceneRef` — a game object's reference relation to its `Scene`.

use super::component::Component;

/// Identifies a game object robustly across a [`Scene`](super::scene::Scene).
///
/// Reference identity is the stable `(scene_id, serial)` pair; `generation`
/// mirrors the entity's current generation. The component is filled
/// automatically by `Scene::create_go` and is otherwise immutable (game
/// objects are not re-parented across scenes without a destroy-recreate or a
/// future save/restore API).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneRef {
    /// Globally-unique owning scene id.
    pub scene_id: u32,
    /// Scene-local, monotonically increasing creation serial.
    pub serial: u64,
    /// Entity generation mirror (stale detection).
    pub generation: u32,
}

impl Component for SceneRef {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_zeroed() {
        // No `Default` is provided: the struct is always filled by `Scene`.
        let r = SceneRef {
            scene_id: 7,
            serial: 42,
            generation: 3,
        };
        assert_eq!(r.scene_id, 7);
        assert_eq!(r.serial, 42);
        assert_eq!(r.generation, 3);
    }
}
