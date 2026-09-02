//! Lifecycle hooks for components (core-ecs extension).
//!
//! The core ECS stays decoupled from any game-object type (`Scene`): a
//! component type may carry a pair of type-erased hook functions that the
//! [`crate::world::World`] fires at well-defined lifecycle points. The
//! functions receive
//! * the live component data pointer (`*mut u8`), and
//! * an opaque single-threaded context pointer (`*mut ()`) bound via
//!   [`crate::world::World::bind_hook_context`].
//!
//! The GO layer funnels its [`Component`](crate::go::Component) hook methods
//! through this bridge so the core never names a `Scene`.

/// Type-erased component lifecycle hooks.
///
/// Both functions are `fn(*mut u8, *mut ())`:
/// * `*mut u8` — the address of the live component value inside a column.
/// * `*mut ()` — the bound context pointer (e.g. a `Scene`), or a "null"
///   marker. The world skips firing when no context is bound.
#[derive(Clone, Copy, Debug)]
pub struct ComponentHooks {
    /// Invoked once after a value is successfully added to an entity.
    pub on_add: Option<fn(*mut u8, *mut ())>,
    /// Invoked once immediately before a value is dropped (remove / destroy /
    /// clear). Never invoked for archetype-migrated (bitwise moved) values.
    pub on_remove: Option<fn(*mut u8, *mut ())>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_hooks_are_copy_and_nullable() {
        let none = ComponentHooks {
            on_add: None,
            on_remove: None,
        };
        assert!(none.on_add.is_none());
        assert!(none.on_remove.is_none());
        fn noop(_: *mut u8, _: *mut ()) {}
        let some = ComponentHooks {
            on_add: Some(noop),
            on_remove: None,
        };
        // Copy is preserved.
        let copy = some;
        assert!(copy.on_add.is_some() && copy.on_remove.is_none());
    }
}
