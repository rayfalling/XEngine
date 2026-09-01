//! Runtime component type registry (type-id based, script-component ready).

use std::any::TypeId;
use std::collections::HashMap;

use crate::component::ComponentHooks;
use crate::error::{WorldError, WorldResult};

/// Layout and lifecycle descriptor of one registered component type.
///
/// Components live in type-erased columns; the descriptor is the single
/// source of truth for size, alignment, drop behavior and the scriptable
/// marker. Runtime registration (including script-registered component
/// types) uses the same descriptor language as native components.
#[derive(Clone, Copy, Debug)]
pub struct ComponentDescriptor {
    /// Size in bytes of the component type.
    pub size: usize,
    /// Alignment in bytes of the component type.
    pub align: usize,
    /// Whether the component was registered as scriptable (script payload).
    pub scriptable: bool,
    /// Drop function called for each live instance (None for no-op drops).
    pub drop_fn: Option<fn(*mut u8)>,
    /// Optional lifecycle hooks (None for the default registration path).
    pub hooks: Option<ComponentHooks>,
}

impl ComponentDescriptor {
    /// Builds the descriptor for a concrete native type (no hooks).
    pub fn of<T>(scriptable: bool) -> Self {
        Self {
            size: std::mem::size_of::<T>(),
            // The true alignment is always used: ZSTs with an explicit
            // `#[repr(align(N))]` must keep their alignment so references
            // created from column pointers stay valid.
            align: std::mem::align_of::<T>(),
            scriptable,
            drop_fn: Some(needs_drop::<T>()),
            hooks: None,
        }
    }

    /// Builds the descriptor for a concrete native type carrying hooks.
    pub fn of_with_hooks<T>(hooks: ComponentHooks) -> Self {
        Self {
            size: std::mem::size_of::<T>(),
            align: std::mem::align_of::<T>(),
            scriptable: false,
            drop_fn: Some(needs_drop::<T>()),
            hooks: Some(hooks),
        }
    }

    /// Descriptor for a raw (type-erased) component instance; used by the
    /// runtime registration path (script components). Layout is supplied by
    /// the script runtime at registration time in a later change.
    pub fn of_raw(scriptable: bool) -> Self {
        Self {
            size: 0,
            align: 1,
            scriptable,
            drop_fn: None,
            hooks: None,
        }
    }
}

/// The typed drop function for `T`.
fn needs_drop<T>() -> fn(*mut u8) {
    fn drop_value<T>(ptr: *mut u8) {
        // Safety: the column contract guarantees `ptr` points at a live T.
        unsafe {
            std::ptr::drop_in_place(ptr as *mut T);
        }
    }
    drop_value::<T>
}

/// Registry of component types keyed by `TypeId`, plus the global
/// registration order (deterministic drop order).
#[derive(Default)]
pub struct ComponentRegistry {
    descriptors: HashMap<TypeId, ComponentDescriptor>,
    order: Vec<TypeId>,
}

impl ComponentRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a concrete component type. Duplicate registration returns
    /// an error and leaves the existing entry untouched.
    pub fn register<T: 'static>(&mut self, scriptable: bool) -> WorldResult<()> {
        self.insert(
            TypeId::of::<T>(),
            std::any::type_name::<T>(),
            ComponentDescriptor::of::<T>(scriptable),
        )
    }

    /// Registers a concrete component type together with lifecycle hooks.
    /// Duplicate registration returns an error and leaves the existing entry
    /// (and its hooks) untouched.
    pub fn register_component_meta<T: 'static>(
        &mut self,
        hooks: ComponentHooks,
    ) -> WorldResult<()> {
        self.insert(
            TypeId::of::<T>(),
            std::any::type_name::<T>(),
            ComponentDescriptor::of_with_hooks::<T>(hooks),
        )
    }

    /// Runtime (raw) registration path: script components register through
    /// a self-describing descriptor without a Rust type.
    pub fn register_raw(
        &mut self,
        id: TypeId,
        name: &'static str,
        desc: ComponentDescriptor,
    ) -> WorldResult<()> {
        self.insert(id, name, desc)
    }

    fn insert(
        &mut self,
        id: TypeId,
        name: &'static str,
        desc: ComponentDescriptor,
    ) -> WorldResult<()> {
        if self.descriptors.contains_key(&id) {
            return Err(WorldError::DuplicateRegistration(name));
        }
        self.descriptors.insert(id, desc);
        self.order.push(id);
        Ok(())
    }

    /// Idempotent auto-registration used by world operations.
    pub fn ensure_registered<T: 'static>(&mut self) {
        if !self.descriptors.contains_key(&TypeId::of::<T>()) {
            self.register::<T>(false)
                .expect("uncontended auto-register");
        }
    }

    /// Returns the descriptor for an id, if registered.
    pub fn descriptor(&self, id: &TypeId) -> Option<ComponentDescriptor> {
        self.descriptors.get(id).copied()
    }

    /// Returns the descriptor for a concrete type, if registered.
    pub fn descriptor_of<T: 'static>(&self) -> Option<ComponentDescriptor> {
        self.descriptor(&TypeId::of::<T>())
    }

    /// Registration order as TypeIds (deterministic drop order).
    pub fn order(&self) -> &[TypeId] {
        &self.order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    struct Health(i32);
    struct Transform;
    #[allow(dead_code)]
    struct ScriptPayload;

    #[test]
    fn register_and_describe() {
        let mut reg = ComponentRegistry::new();
        reg.register::<Health>(false).unwrap();
        let desc = reg.descriptor_of::<Health>().unwrap();
        assert_eq!(desc.size, std::mem::size_of::<Health>());
        assert_eq!(desc.align, std::mem::align_of::<Health>());
        assert!(desc.drop_fn.is_some());
        assert!(!desc.scriptable);
    }

    #[test]
    fn duplicate_registration_errors_and_preserves_first() {
        let mut reg = ComponentRegistry::new();
        reg.register::<Transform>(false).unwrap();
        let err = reg.register::<Transform>(false).unwrap_err();
        assert!(matches!(err, WorldError::DuplicateRegistration(_)));
        assert!(reg.descriptor_of::<Transform>().is_some());
    }

    #[test]
    fn scriptable_marker_and_runtime_path() {
        let mut reg = ComponentRegistry::new();
        reg.register::<ScriptPayload>(true).unwrap();
        let desc = reg.descriptor_of::<ScriptPayload>().unwrap();
        assert!(desc.scriptable);
        // Runtime raw path: self-describing descriptor.
        let id = TypeId::of::<ScriptPayload>();
        let raw = ComponentDescriptor::of_raw(true);
        let err = reg.register_raw(id, "script:p", raw).unwrap_err();
        assert!(matches!(err, WorldError::DuplicateRegistration(_)));
    }

    #[test]
    fn registration_order_is_stable() {
        let mut reg = ComponentRegistry::new();
        reg.register::<Health>(false).unwrap();
        reg.register::<Transform>(false).unwrap();
        assert_eq!(reg.order().len(), 2);
        assert_eq!(reg.order()[0], TypeId::of::<Health>());
        assert_eq!(reg.order()[1], TypeId::of::<Transform>());
    }

    #[test]
    fn zst_keeps_explicit_alignment() {
        #[repr(align(16))]
        struct AlignedZst;
        let desc = ComponentDescriptor::of::<AlignedZst>(false);
        assert_eq!(desc.size, 0, "ZST has zero size");
        assert_eq!(desc.align, 16, "ZST keeps its explicit alignment");
    }
}
