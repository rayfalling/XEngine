//! The GO (game-object) layer.
//!
//! A `Scene` is the runtime container for game objects. It owns an ECS
//! [`World`](crate::World), allocates a globally-unique `scene_id` and a
//! scene-local `serial` sequence, and drives everything in this layer: the
//! component trio (`Transform` / `SceneRef` / `Parent`-`Children`), hierarchy
//! maintenance, dirty-driven transform propagation and the script-facing
//! `GoHandle` wrapper.
//!
//! The GO layer lives in the core crate so it stays a pure-Rust, platform-free
//! data source for the render layer. It is single-threaded: a `Scene` must be
//! driven from one thread (documented on [`scene::Scene`]).
//!
//! `GameObject` is an alias for [`Entity`](crate::Entity): there is no extra
//! wrapper struct, so an entity *is* a game object.

pub mod component;
pub mod global_transform;
pub mod go_handle;
pub mod hierarchy;
pub mod scene;
pub mod scene_ref;
pub mod transform;

pub use component::Component;
pub use global_transform::{GlobalTransform, TransformDirty};
pub use go_handle::{GoHandle, GoHandleError, GoLoc, GoView};
pub use hierarchy::{Children, HierarchyError, Parent};
pub use scene::{GameObject, Scene, SceneHandle};
pub use scene_ref::SceneRef;
pub use transform::Transform;
