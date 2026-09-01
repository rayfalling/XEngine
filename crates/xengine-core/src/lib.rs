//! XEngine core library — the 100% Rust core layer.
//!
//! Data-oriented ECS primitives plus the Unity-style frame scheduling model.
//! This crate MUST stay free of any platform dependency (no graphics API,
//! no device-layer types); the device layer consumes the core interfaces
//! (e.g. [`render::RenderSnapshot`]) through the FFI boundary defined later.

pub mod archetype;
pub mod command;
pub mod component;
pub mod entity;
pub mod error;
pub mod frame;
pub mod go;
pub mod registry;
pub mod render;
pub mod schedule;
pub mod storage;
pub mod system;
pub mod world;

pub use component::ComponentHooks;
pub use entity::Entity;
pub use error::{WorldError, WorldResult};
pub use frame::{Engine, FrameMode, RunStats, TimeState};
pub use go::{
    Children, Component, GameObject, GlobalTransform, GoHandle, GoHandleError, GoLoc, GoView,
    HierarchyError, Parent, Scene, SceneRef, Transform, TransformDirty,
};
pub use render::{NullRenderSink, RenderSink, RenderSnapshot};
pub use schedule::{Schedule, ScheduleError};
pub use system::{AccessKind, Stage, System};
pub use world::World;

/// The engine's display name. Every core function must carry a unit test.
pub fn engine_name() -> &'static str {
    "XEngine"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_name_is_consistent() {
        assert_eq!(engine_name(), "XEngine");
    }
}
