//! The layer-level `Component` trait for the GO layer.
//!
//! The core ECS stays decoupled from `Scene`: it stores type-erased hooks
//! ([`ComponentHooks`](crate::component::ComponentHooks)). This module is the
//! *bridge*: it defines the typed trait (whose hook methods receive
//! `&mut Scene`) and generates the type-erased dispatch functions that the
//! world calls, casting the opaque context pointer back to a `&mut Scene`.

use crate::component::ComponentHooks;
use crate::error::WorldResult;
use crate::world::World;

use super::scene::Scene;

/// A component that may observe its own lifecycle with a `Scene` context.
///
/// Both hooks default to no-ops. They fire at the points documented by
/// [`crate::component::ComponentHooks`]:
/// * `on_add` — once, after the value is added to an entity.
/// * `on_remove` — once, immediately before the value is dropped
///   (remove / destroy / clear). Never fired for archetype-migrated values.
pub trait Component: 'static {
    /// Called once after the component value is added to an entity.
    fn on_add(&mut self, _scene: &mut Scene) {}
    /// Called once immediately before the component value is dropped.
    fn on_remove(&mut self, _scene: &mut Scene) {}
}

/// Builds the type-erased hooks for a `Component` type.
///
/// The dispatched functions cast the component data pointer back to `&mut T`
/// and the bound context pointer back to `&mut Scene`.
///
/// # Safety
///
/// * The core world guarantees `data` points at a live `T` inside a column
///   and `ctx` is the pointer bound by `World::bind_hook_context`.
/// * The GO layer guarantees that context is a stable `SceneHandle` (an
///   `Scene::new`, driven from a single thread, and that the hook does not
///   structurally mutate the same world's layout re-entrantly during its own
///   call (single-threaded contract, documented on [`Scene`]).
///
/// Reborrowing `data`/`ctx` as `&mut` while the world is mid-mutation is the
/// only intentionally-unsafe convergence point of the hook bridge. It is kept
/// off the public API surface (the world only ever hands out raw pointers).
pub(crate) fn component_hooks<T: Component>() -> ComponentHooks {
    // Safety: see the module-level `# Safety` note; both `data` and `ctx` are
    // guaranteed live for the duration of the call by the core + go layers.
    fn on_add_dispatch<T: Component>(data: *mut u8, ctx: *mut ()) {
        let comp = unsafe { &mut *(data as *mut T) };
        let scene = unsafe { &mut *(ctx as *mut Scene) };
        comp.on_add(scene);
    }
    // Safety: same contract as `on_add_dispatch`.
    fn on_remove_dispatch<T: Component>(data: *mut u8, ctx: *mut ()) {
        let comp = unsafe { &mut *(data as *mut T) };
        let scene = unsafe { &mut *(ctx as *mut Scene) };
        comp.on_remove(scene);
    }
    ComponentHooks {
        on_add: Some(on_add_dispatch::<T>),
        on_remove: Some(on_remove_dispatch::<T>),
    }
}

/// Registers a `Component` type (with its hooks) into a world.
///
/// Duplicate registration returns an error and preserves the first entry.
pub(crate) fn register_component<T: Component>(world: &mut World) -> WorldResult<()> {
    world.register_component_meta::<T>(component_hooks::<T>())
}
