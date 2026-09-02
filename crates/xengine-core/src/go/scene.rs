//! `Scene`: the GO-layer runtime container for game objects.
//!
//! A `Scene` owns an ECS [`World`](crate::World), allocates a globally-unique
//! `scene_id`, and a scene-local monotonic `serial` sequence. Every GO-side
//! lifecycle / hierarchy / propagation / wrapper access goes through it. It is
//! driven from a single thread; `Scene::new` returns a `Pin<Box<Scene>>` so the
//! heap address is stable for the hook-context binding.

use std::collections::HashSet;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};

use xengine_math::{QuaternionF, Vector3F};

use crate::entity::Entity;
use crate::error::WorldResult;
use crate::world::World;

use super::component::Component;
use super::global_transform::TransformDirty;
use super::go_handle::{GoHandle, GoHandleError, GoLoc, GoView};
use super::hierarchy::{Children, HierarchyError, Parent};
use super::scene_ref::SceneRef;
use super::transform::Transform;

/// A game object is an [`Entity`]: no wrapper struct, an entity *is* a GO.
pub type GameObject = Entity;

/// Globally-unique scene-id allocator (the GO layer never reuses ids).
static NEXT_SCENE_ID: AtomicU32 = AtomicU32::new(1);

/// The GO-layer game-object container.
///
/// `!Unpin` by design: the scene's heap address (from `Pin<Box<Scene>>`) is
/// bound as the world's hook context, so the `Scene` must never move.
pub struct Scene {
    pub(crate) world: World,
    scene_id: u32,
    serial_counter: u64,
    _pin: std::marker::PhantomPinned,
}

impl Scene {
    /// Creates a new `Scene`, allocating a globally-unique `scene_id` and
    /// binding its own (heap-stable) address as the world's hook context.
    ///
    /// The scene is single-threaded: it must be driven from one thread. Its
    /// heap address (from the `Pin<Box<Scene>>`) is what component-lifecycle
    /// hooks receive as their context, so it must not move — the `!Unpin`
    /// marker enforces that `Pin::into_inner` is unavailable.
    pub fn new() -> Pin<Box<Scene>> {
        let scene_id = NEXT_SCENE_ID.fetch_add(1, Ordering::Relaxed);
        let mut boxed = Box::new(Scene {
            world: World::new(),
            scene_id,
            serial_counter: 0,
            _pin: std::marker::PhantomPinned,
        });
        let ctx = &mut *boxed as *mut Scene;
        // Safety: `boxed` is a `Box` whose address is stable for the box's
        // lifetime and the pointer is never invalidated before `Scene` drops
        // (the box is immediately pinned and never moved).
        unsafe { boxed.world.bind_hook_context(ctx as *mut ()) };
        Pin::from(boxed)
    }

    /// `&mut` access through a pinned box.
    ///
    /// The GO layer binds the world's hook-context pointer to this heap value,
    /// so the `Scene` must never move or be replaced; the pinned box (the only
    /// construction path) keeps the value's address structurally stable.
    ///
    /// # Safety
    /// The caller must keep the pinned box alive and must not replace or
    /// otherwise move the heap value for as long as the world lives.
    pub unsafe fn pinned_mut(pin: &mut Pin<Box<Scene>>) -> &mut Scene {
        // Safety: `get_unchecked_mut` is fine because a `Pin<Box>` keeps its
        // heap value structurally in place; the caller's contract covers the
        // "never replace the value" requirement.
        unsafe { pin.as_mut().get_unchecked_mut() }
    }

    /// Shared access to the ECS world.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Mutable access to the ECS world.
    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    /// The globally-unique id of this scene.
    pub fn scene_id(&self) -> u32 {
        self.scene_id
    }

    /// Registers a GO-layer `Component` type (with its lifecycle hooks).
    /// Duplicate registration errors and preserves the first entry.
    pub fn register_component<T: Component>(&mut self) -> WorldResult<()> {
        super::component::register_component::<T>(&mut self.world)
    }

    // ── lifecycle ─────────────────────────────────────────────────────────

    /// Creates a game object with the given `Transform`. The entity gains the
    /// component trio: `Transform`, `SceneRef` (auto-filled) and `Parent`
    /// (`None` = root). `Children` is maintained by the hierarchy system once
    /// the entity becomes a parent.
    pub fn create_go(&mut self, transform: Transform) -> WorldResult<Entity> {
        let e = self.world.create_empty();
        self.world.add(e, transform)?;
        self.serial_counter += 1;
        let scene_ref = SceneRef {
            scene_id: self.scene_id,
            serial: self.serial_counter,
            generation: e.generation(),
        };
        self.world.add(e, scene_ref)?;
        self.world.add(e, Parent { parent: None })?;
        Ok(e)
    }

    /// Adds a component to a game object (delegates to the ECS world).
    pub fn add_component<T: 'static>(&mut self, entity: Entity, value: T) -> WorldResult<()> {
        self.world.add(entity, value)
    }

    /// Removes a component type from a game object (delegates to the ECS).
    pub fn remove_component<T: 'static>(&mut self, entity: Entity) -> WorldResult<()> {
        self.world.remove::<T>(entity)
    }

    // ── hierarchy ─────────────────────────────────────────────────────────

    /// Sets (or clears) `child`'s parent. Maintains bidirectional consistency
    /// eagerly: the child leaves its old parent's `Children` and joins the new
    /// parent's, and a parent that would form a cycle is rejected
    /// (`Err(HierarchyCycle)`).
    pub fn set_parent(
        &mut self,
        child: Entity,
        parent: Option<Entity>,
    ) -> Result<(), HierarchyError> {
        if !self.world.contains_entity(child) {
            return Err(HierarchyError::StaleEntity);
        }
        if let Some(p) = parent {
            if !self.world.contains_entity(p) {
                return Err(HierarchyError::StaleEntity);
            }
            if p == child {
                return Err(HierarchyError::Cycle);
            }
            if super::hierarchy::would_cycle(&self.world, child, p) {
                return Err(HierarchyError::Cycle);
            }
        }
        self.ensure_parent_component(child)?;
        let old = self.parent_of(child);
        if let Some(old) = old
            && Some(old) != parent
        {
            self.remove_child_from_parent(old, child);
        }
        if let Ok(Some(p)) = self.world.get_mut::<Parent>(child) {
            p.parent = parent;
        }
        if let Some(p) = parent {
            self.ensure_children_component(p)?;
            if let Ok(Some(c)) = self.world.get_mut::<Children>(p)
                && !c.children.contains(&child)
            {
                c.children.push(child);
            }
        }
        Ok(())
    }

    /// Destroys the whole subtree rooted at `entity` (default cascade),
    /// depth-first, each entity exactly once. A game object is removed from its
    /// parent's `Children` before it is destroyed so no dangling edge remains.
    /// The ECS-level `World::destroy` keeps its single-entity semantics.
    pub fn destroy(&mut self, entity: Entity) -> WorldResult<()> {
        let subtree = self.collect_subtree(entity);
        // Pre-order collected; destroy leaves-first (reverse).
        for &e in subtree.iter().rev() {
            let parent = self.parent_of(e);
            if let Some(parent) = parent {
                self.remove_child_from_parent(parent, e);
            }
            self.world.destroy(e)?;
        }
        Ok(())
    }

    /// Detaches `entity` (and its subtree, which is preserved) to root: its
    /// `Parent` becomes `None` and it is removed from the old parent's
    /// `Children`.
    pub fn detach(&mut self, entity: Entity) -> WorldResult<()> {
        if !self.world.contains_entity(entity) {
            return Ok(()); // idempotent
        }
        let parent = self.parent_of(entity);
        if let Some(parent) = parent {
            self.remove_child_from_parent(parent, entity);
        }
        if let Ok(Some(p)) = self.world.get_mut::<Parent>(entity) {
            p.parent = None;
        }
        Ok(())
    }

    // ── transform set API (auto-mark dirty) ───────────────────────────────

    /// Sets the whole local transform and marks it dirty.
    pub fn set_go_transform(&mut self, entity: Entity, transform: Transform) -> WorldResult<()> {
        if let Ok(Some(t)) = self.world.get_mut::<Transform>(entity) {
            *t = transform;
        }
        self.mark_transform_dirty(entity)?;
        Ok(())
    }

    /// Sets the local position and marks the transform dirty.
    pub fn set_transform_position(
        &mut self,
        entity: Entity,
        position: Vector3F,
    ) -> WorldResult<()> {
        if let Ok(Some(t)) = self.world.get_mut::<Transform>(entity) {
            t.position = position;
        }
        self.mark_transform_dirty(entity)?;
        Ok(())
    }

    /// Sets the local rotation and marks the transform dirty.
    pub fn set_transform_rotation(
        &mut self,
        entity: Entity,
        rotate: QuaternionF,
    ) -> WorldResult<()> {
        if let Ok(Some(t)) = self.world.get_mut::<Transform>(entity) {
            t.rotate = rotate;
        }
        self.mark_transform_dirty(entity)?;
        Ok(())
    }

    /// Sets the local scale and marks the transform dirty.
    pub fn set_transform_scale(&mut self, entity: Entity, scale: Vector3F) -> WorldResult<()> {
        if let Ok(Some(t)) = self.world.get_mut::<Transform>(entity) {
            t.scale = scale;
        }
        self.mark_transform_dirty(entity)?;
        Ok(())
    }

    /// Marks a game object's transform dirty (needed after a direct public
    /// field write that bypasses the set APIs).
    pub fn mark_transform_dirty(&mut self, entity: Entity) -> WorldResult<()> {
        if self.world.contains_entity(entity)
            && !self
                .world
                .contains::<TransformDirty>(entity)
                .unwrap_or(false)
        {
            self.world.add(entity, TransformDirty)?;
        }
        Ok(())
    }

    // ── wrapper (GoHandle) ────────────────────────────────────────────────

    /// Builds a `GoHandle` for a live entity, caching its current location
    /// (or `None` when the entity is not live).
    pub fn go_handle(&self, entity: Entity) -> GoHandle {
        let loc = self.world.location_of(entity).map(|(arch, row)| GoLoc {
            arch,
            row,
            generation: self.world.live_generation(entity).unwrap_or(0),
        });
        GoHandle { entity, loc }
    }

    /// Validated access to a game object's component trio.
    ///
    /// O(1) when the cached location is fresh (generation + position match);
    /// on a stale location it re-resolves, and on a destroyed entity it
    /// returns `Err(GoHandleStale)` — never a bare pointer, never a panic.
    pub fn go_view(&mut self, handle: GoHandle) -> Result<GoView<'_>, GoHandleError> {
        let entity = handle.entity;
        if !self.world.contains_entity(entity) {
            return Err(GoHandleError::GoHandleStale);
        }
        let loc = match handle.loc {
            Some(loc) => {
                let gen_ok = self.world.live_generation(entity) == Some(loc.generation);
                let pos_ok = self.world.location_of(entity) == Some((loc.arch, loc.row));
                if gen_ok && pos_ok {
                    loc
                } else {
                    let (arch, row) = self
                        .world
                        .location_of(entity)
                        .ok_or(GoHandleError::GoHandleStale)?;
                    GoLoc {
                        arch,
                        row,
                        generation: self.world.live_generation(entity).unwrap_or(0),
                    }
                }
            }
            None => {
                let (arch, row) = self
                    .world
                    .location_of(entity)
                    .ok_or(GoHandleError::GoHandleStale)?;
                GoLoc {
                    arch,
                    row,
                    generation: self.world.live_generation(entity).unwrap_or(0),
                }
            }
        };
        // Validate the component trio at the resolved location before handing
        // out a view: raw ECS operations (e.g. `remove_component`) may have
        // stripped a trio member, and the view must never panic on access.
        for missing in [
            self.world
                .get_at::<Transform>(loc.arch, loc.row as usize)
                .is_none(),
            self.world
                .get_at::<SceneRef>(loc.arch, loc.row as usize)
                .is_none(),
            self.world
                .get_at::<Parent>(loc.arch, loc.row as usize)
                .is_none(),
        ] {
            if missing {
                return Err(GoHandleError::MissingComponent);
            }
        }
        Ok(GoView {
            world: &mut self.world,
            loc,
            entity,
        })
    }

    // ── internal helpers ──────────────────────────────────────────────────

    fn parent_of(&self, entity: Entity) -> Option<Entity> {
        match self.world.get::<Parent>(entity) {
            Ok(Some(p)) => p.parent,
            _ => None,
        }
    }

    fn ensure_parent_component(&mut self, entity: Entity) -> Result<(), HierarchyError> {
        if !self
            .world
            .contains::<Parent>(entity)
            .map_err(|_| HierarchyError::StaleEntity)?
        {
            self.world
                .add(entity, Parent { parent: None })
                .map_err(|_| HierarchyError::StaleEntity)?;
        }
        Ok(())
    }

    fn ensure_children_component(&mut self, entity: Entity) -> Result<(), HierarchyError> {
        if !self
            .world
            .contains::<Children>(entity)
            .map_err(|_| HierarchyError::StaleEntity)?
        {
            self.world
                .add(
                    entity,
                    Children {
                        children: Vec::new(),
                    },
                )
                .map_err(|_| HierarchyError::StaleEntity)?;
        }
        Ok(())
    }

    fn remove_child_from_parent(&mut self, parent: Entity, child: Entity) {
        if !self.world.contains_entity(parent) {
            return;
        }
        if let Ok(Some(c)) = self.world.get_mut::<Children>(parent) {
            c.children.retain(|x| *x != child);
        }
    }

    /// Depth-first collection of the subtree rooted at `entity` (pre-order),
    /// each entity exactly once, tolerant of malformed cycles.
    fn collect_subtree(&self, entity: Entity) -> Vec<Entity> {
        let mut out = Vec::new();
        let mut visited: HashSet<Entity> = HashSet::new();
        let mut stack = vec![entity];
        while let Some(e) = stack.pop() {
            if !self.world.contains_entity(e) {
                continue;
            }
            if !visited.insert(e) {
                continue;
            }
            out.push(e);
            if let Ok(Some(c)) = self.world.get::<Children>(e) {
                for &child in &c.children {
                    if child != e {
                        stack.push(child);
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_allocates_unique_id_and_binds_context() {
        let a = Scene::new();
        let b = Scene::new();
        assert_ne!(a.scene_id(), b.scene_id(), "scene ids are globally unique");
        // Global ids are strictly increasing; exact values depend on the test
        // run's parallel scene creation, so only their ordering is asserted.
        assert!(a.scene_id() < b.scene_id());
    }

    #[test]
    fn create_go_generates_the_trio() {
        let mut scene = Scene::new();
        let scene = unsafe { Scene::pinned_mut(&mut scene) };
        let e = scene.create_go(Transform::default()).unwrap();
        let w = scene.world();
        assert!(w.contains::<Transform>(e).unwrap());
        assert!(w.contains::<SceneRef>(e).unwrap());
        assert!(w.contains::<Parent>(e).unwrap());
        assert_eq!(w.get::<Parent>(e).unwrap().unwrap().parent, None);
    }

    #[test]
    fn scene_ref_auto_fill() {
        let mut scene = Scene::new();
        let scene = unsafe { Scene::pinned_mut(&mut scene) };
        let e1 = scene.create_go(Transform::default()).unwrap();
        let e2 = scene.create_go(Transform::default()).unwrap();
        let r1 = scene.world().get::<SceneRef>(e1).unwrap().unwrap();
        let r2 = scene.world().get::<SceneRef>(e2).unwrap().unwrap();
        assert_eq!(r1.scene_id, scene.scene_id());
        assert_eq!(r1.serial, 1);
        assert_eq!(r2.serial, 2, "serial is monotonic");
        assert_eq!(r1.generation, e1.generation());
        assert_eq!(r2.generation, e2.generation());
    }

    #[test]
    fn set_parent_bidirectional_and_reparent() {
        let mut scene = Scene::new();
        let scene = unsafe { Scene::pinned_mut(&mut scene) };
        let e1 = scene.create_go(Transform::default()).unwrap();
        let e2 = scene.create_go(Transform::default()).unwrap();
        let e3 = scene.create_go(Transform::default()).unwrap();
        scene.set_parent(e2, Some(e1)).unwrap();
        assert_eq!(
            scene.world().get::<Parent>(e2).unwrap().unwrap().parent,
            Some(e1)
        );
        assert!(
            scene
                .world()
                .get::<Children>(e1)
                .unwrap()
                .unwrap()
                .children
                .contains(&e2)
        );
        scene.set_parent(e2, Some(e3)).unwrap();
        assert_eq!(
            scene.world().get::<Parent>(e2).unwrap().unwrap().parent,
            Some(e3)
        );
        assert!(
            !scene
                .world()
                .get::<Children>(e1)
                .unwrap()
                .unwrap()
                .children
                .contains(&e2)
        );
        assert!(
            scene
                .world()
                .get::<Children>(e3)
                .unwrap()
                .unwrap()
                .children
                .contains(&e2)
        );
    }

    #[test]
    fn set_parent_cycle_is_rejected() {
        let mut scene = Scene::new();
        let scene = unsafe { Scene::pinned_mut(&mut scene) };
        let a = scene.create_go(Transform::default()).unwrap();
        let b = scene.create_go(Transform::default()).unwrap();
        scene.set_parent(b, Some(a)).unwrap();
        let err = scene.set_parent(a, Some(b)).unwrap_err();
        assert!(matches!(err, HierarchyError::Cycle));
        // State unchanged: a stays root, b stays under a.
        assert_eq!(
            scene.world().get::<Parent>(a).unwrap().unwrap().parent,
            None
        );
        assert_eq!(
            scene.world().get::<Parent>(b).unwrap().unwrap().parent,
            Some(a)
        );
    }

    #[test]
    fn destroy_cascades_depth_first() {
        let mut scene = Scene::new();
        let scene = unsafe { Scene::pinned_mut(&mut scene) };
        let root = scene.create_go(Transform::default()).unwrap();
        let mid = scene.create_go(Transform::default()).unwrap();
        let leaf = scene.create_go(Transform::default()).unwrap();
        scene.set_parent(mid, Some(root)).unwrap();
        scene.set_parent(leaf, Some(mid)).unwrap();
        scene.destroy(root).unwrap();
        assert!(!scene.world().contains_entity(root));
        assert!(!scene.world().contains_entity(mid));
        assert!(!scene.world().contains_entity(leaf));
        assert_eq!(scene.world().entity_count(), 0);
    }

    #[test]
    fn detach_keeps_subtree_and_roots_it() {
        let mut scene = Scene::new();
        let scene = unsafe { Scene::pinned_mut(&mut scene) };
        let root = scene.create_go(Transform::default()).unwrap();
        let mid = scene.create_go(Transform::default()).unwrap();
        let leaf = scene.create_go(Transform::default()).unwrap();
        scene.set_parent(mid, Some(root)).unwrap();
        scene.set_parent(leaf, Some(mid)).unwrap();
        scene.detach(mid).unwrap();
        assert!(scene.world().contains_entity(mid));
        assert!(scene.world().contains_entity(leaf));
        assert_eq!(
            scene.world().get::<Parent>(mid).unwrap().unwrap().parent,
            None
        );
        assert!(
            !scene
                .world()
                .get::<Children>(root)
                .unwrap()
                .unwrap()
                .children
                .contains(&mid)
        );
    }

    #[test]
    fn world_destroy_single_entity_is_not_cascade() {
        let mut scene = Scene::new();
        let scene = unsafe { Scene::pinned_mut(&mut scene) };
        let root = scene.create_go(Transform::default()).unwrap();
        let mid = scene.create_go(Transform::default()).unwrap();
        scene.set_parent(mid, Some(root)).unwrap();
        scene.world_mut().destroy(mid).unwrap(); // ECS-level: single entity only
        assert!(!scene.world().contains_entity(mid));
        assert!(scene.world().contains_entity(root));
    }

    #[test]
    fn set_transform_marks_dirty() {
        let mut scene = Scene::new();
        let scene = unsafe { Scene::pinned_mut(&mut scene) };
        let e = scene.create_go(Transform::default()).unwrap();
        assert!(!scene.world().contains::<TransformDirty>(e).unwrap());
        scene
            .set_transform_position(e, Vector3F::new(1.0, 2.0, 3.0))
            .unwrap();
        assert!(scene.world().contains::<TransformDirty>(e).unwrap());
    }

    #[test]
    fn go_handle_and_view_roundtrip() {
        let mut scene = Scene::new();
        let scene = unsafe { Scene::pinned_mut(&mut scene) };
        let e = scene
            .create_go(Transform {
                position: Vector3F::new(4.0, 5.0, 6.0),
                ..Transform::default()
            })
            .unwrap();
        let handle = scene.go_handle(e);
        {
            let mut view = scene.go_view(handle).unwrap();
            assert_eq!(view.scene_ref().serial, 1);
            assert_eq!(view.entity(), e);
            view.transform().position = Vector3F::new(7.0, 8.0, 9.0);
        }
        assert_eq!(
            scene.world().get::<Transform>(e).unwrap().unwrap().position,
            Vector3F::new(7.0, 8.0, 9.0)
        );
    }

    #[test]
    fn go_view_after_migration_re_resolves() {
        let mut scene = Scene::new();
        let scene = unsafe { Scene::pinned_mut(&mut scene) };
        let e = scene.create_go(Transform::default()).unwrap();
        let handle = scene.go_handle(e);
        // Migration: add a marker component -> the entity moves archetypes.
        scene.world_mut().add(e, DirtyMarker).unwrap();
        let mut view = scene.go_view(handle).unwrap();
        view.transform().position = Vector3F::new(0.0, 0.0, 1.0);
        assert_eq!(
            scene.world().get::<Transform>(e).unwrap().unwrap().position,
            Vector3F::new(0.0, 0.0, 1.0)
        );
    }

    #[test]
    fn go_view_after_destroy_is_stale() {
        let mut scene = Scene::new();
        let scene = unsafe { Scene::pinned_mut(&mut scene) };
        let e = scene.create_go(Transform::default()).unwrap();
        let handle = scene.go_handle(e);
        scene.destroy(e).unwrap();
        assert!(scene.go_view(handle).is_err());
    }

    #[derive(Clone, Copy, Debug)]
    struct DirtyMarker;
}
