//! Deferred world mutations: `Commands`.

use crate::entity::Entity;
use crate::world::World;

/// One deferred world mutation closure.
pub type WorldCommand = Box<dyn FnOnce(&mut World)>;

/// Queue of deferred closures applied at the next flush point.
#[derive(Default)]
pub struct CommandQueue(pub(crate) Vec<WorldCommand>);

impl CommandQueue {
    pub(crate) fn push(&mut self, f: WorldCommand) {
        self.0.push(f);
    }

    pub(crate) fn take(&mut self) -> Vec<WorldCommand> {
        std::mem::take(&mut self.0)
    }
}

/// Deferred operations buffered during a system and flushed at the system
/// boundary. Ordering: FIFO; semantics identical to the synchronous API.
pub struct Commands<'a> {
    world: &'a mut World,
}

impl<'a> Commands<'a> {
    pub(crate) fn new(world: &'a mut World) -> Self {
        Self { world }
    }

    /// Queues entity creation with one initial component. The returned
    /// entity handle is reserved immediately and becomes valid at flush.
    pub fn create1<A: 'static>(&mut self, a: A) -> Entity {
        let e = self.world.reserve_entity();
        self.world.queue().push(Box::new(move |world| {
            world.create_into(e);
            let _ = world.add(e, a);
        }));
        e
    }

    /// Queues entity creation with two initial components.
    pub fn create2<A: 'static, B: 'static>(&mut self, a: A, b: B) -> Entity {
        let e = self.world.reserve_entity();
        self.world.queue().push(Box::new(move |world| {
            world.create_into(e);
            let _ = world.add(e, a);
            let _ = world.add(e, b);
        }));
        e
    }

    /// Queues entity creation with three initial components.
    pub fn create3<A: 'static, B: 'static, C: 'static>(&mut self, a: A, b: B, c: C) -> Entity {
        let e = self.world.reserve_entity();
        self.world.queue().push(Box::new(move |world| {
            world.create_into(e);
            let _ = world.add(e, a);
            let _ = world.add(e, b);
            let _ = world.add(e, c);
        }));
        e
    }

    /// Queues entity creation with four initial components.
    pub fn create4<A: 'static, B: 'static, C: 'static, D: 'static>(
        &mut self,
        a: A,
        b: B,
        c: C,
        d: D,
    ) -> Entity {
        let e = self.world.reserve_entity();
        self.world.queue().push(Box::new(move |world| {
            world.create_into(e);
            let _ = world.add(e, a);
            let _ = world.add(e, b);
            let _ = world.add(e, c);
            let _ = world.add(e, d);
        }));
        e
    }

    /// Queues adding a component.
    pub fn add<T: 'static>(&mut self, entity: Entity, value: T) {
        self.world.queue().push(Box::new(move |world| {
            let _ = world.add(entity, value);
        }));
    }

    /// Queues removing a component type.
    pub fn remove<T: 'static>(&mut self, entity: Entity) {
        self.world.queue().push(Box::new(move |world| {
            let _ = world.remove::<T>(entity);
        }));
    }

    /// Queues entity destruction (idempotent).
    pub fn destroy(&mut self, entity: Entity) {
        self.world.queue().push(Box::new(move |world| {
            let _ = world.destroy(entity);
        }));
    }

    /// Queues inserting a resource (replaces an existing one).
    pub fn insert_resource<T: 'static>(&mut self, value: T) {
        self.world.queue().push(Box::new(move |world| {
            world.insert_resource(value);
        }));
    }

    /// Queues an arbitrary deferred closure.
    pub fn push(&mut self, f: impl FnOnce(&mut World) + 'static) {
        self.world.queue().push(Box::new(f));
    }
}
