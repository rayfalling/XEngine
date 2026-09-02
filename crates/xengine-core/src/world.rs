//! The ECS world: entities, archetypes, lifecycle, queries and resources.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::mem::ManuallyDrop;

use crate::archetype::Archetype;
use crate::command::{CommandQueue, Commands};
use crate::component::ComponentHooks;
use crate::entity::{Entity, EntityAllocator};
use crate::error::{WorldError, WorldResult};
use crate::registry::{ComponentDescriptor, ComponentRegistry};

/// Per-entity slot: generation + archetype membership.
struct Slot {
    generation: u32,
    archetype: Option<usize>,
    row: u32,
}

/// The ECS world.
///
/// Owns the component registry, archetype storage, resources and the
/// deferred command queue. All lifecycle operations are O(1) amortized.
pub struct World {
    registry: ComponentRegistry,
    allocator: EntityAllocator,
    archetypes: Vec<Archetype>,
    archetype_ids: HashMap<Vec<TypeId>, usize>,
    slots: Vec<Slot>,
    resources: HashMap<TypeId, Box<dyn Any>>,
    queue: CommandQueue,
    /// Single-threaded lifecycle-context pointer forwarded to component
    /// hooks. `None` means "no context bound": hooks are skipped entirely.
    hook_context: Option<*mut ()>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    /// Creates an empty world.
    pub fn new() -> Self {
        Self {
            registry: ComponentRegistry::new(),
            allocator: EntityAllocator::new(),
            archetypes: Vec::new(),
            archetype_ids: HashMap::new(),
            slots: Vec::new(),
            resources: HashMap::new(),
            queue: CommandQueue::default(),
            hook_context: None,
        }
    }

    /// Explicitly registers a component type. Duplicate registration errors.
    pub fn register<T: 'static>(&mut self, scriptable: bool) -> WorldResult<()> {
        self.registry.register::<T>(scriptable)
    }

    /// Explicitly registers a component type together with lifecycle hooks.
    /// Duplicate registration errors and leaves the first registration (and
    /// its hooks) untouched.
    pub fn register_component_meta<T: 'static>(
        &mut self,
        hooks: ComponentHooks,
    ) -> WorldResult<()> {
        self.registry.register_component_meta::<T>(hooks)
    }

    /// Whether a component type is registered in the registry.
    pub fn is_registered<T: 'static>(&self) -> bool {
        self.registry.descriptor_of::<T>().is_some()
    }

    /// Binds the single-threaded lifecycle-context pointer handed verbatim to
    /// every registered component hook (`on_add` / `on_remove`).
    ///
    /// The context must point at a stable, non-moving object (e.g. a
    /// `Pin<Box<Scene>>` heap allocation) that stays valid for the whole world
    /// lifetime and is only mutated from the single thread driving the world.
    /// When no context is bound the hooks are skipped (so the GO layer can
    /// guarantee hooks only fire once a `Scene` exists and is bound).
    ///
    /// # Safety
    /// `ctx` must be a pointer to an object that (a) remains valid for the
    /// entire lifetime of this world, (b) never moves while bound, and (c)
    /// matches the type the registered hooks will cast it to (the GO layer
    /// binds `&mut Scene` via `Pin<Box<Scene>>`). A dangling or moving pointer
    /// results in undefined behavior when a hook fires. Hooks run single-threaded.
    pub unsafe fn bind_hook_context(&mut self, ctx: *mut ()) {
        self.hook_context = Some(ctx);
    }

    // ── entity validity ──────────────────────────────────────────────────

    fn valid(&self, e: Entity) -> bool {
        let i = e.index() as usize;
        i < self.slots.len()
            && self.slots[i].generation == e.generation()
            && self.slots[i].archetype.is_some()
    }

    fn ensure_slot(&mut self, e: Entity) {
        let i = e.index() as usize;
        if i >= self.slots.len() {
            self.slots.push(Slot {
                generation: e.generation(),
                archetype: None,
                row: 0,
            });
        } else {
            self.slots[i].generation = e.generation();
            self.slots[i].archetype = None;
        }
    }

    pub(crate) fn reserve_entity(&mut self) -> Entity {
        let e = self.allocator.allocate();
        self.ensure_slot(e);
        e
    }

    // ── archetype helpers ────────────────────────────────────────────────

    fn reg_index(&self, t: &TypeId) -> usize {
        self.registry
            .order()
            .iter()
            .position(|x| x == t)
            .unwrap_or(usize::MAX)
    }

    /// Computes the ordered type set for a migration.
    fn ordered_types(
        &self,
        types: &[TypeId],
        add: Option<TypeId>,
        remove: Option<TypeId>,
    ) -> Vec<TypeId> {
        let mut set: Vec<TypeId> = types
            .iter()
            .filter(|t| Some(**t) != remove)
            .cloned()
            .collect();
        if let Some(add) = add
            && !set.contains(&add)
        {
            set.push(add);
        }
        set.sort_by_key(|t| self.reg_index(t));
        set
    }

    fn archetype_id_for(&mut self, types: &[TypeId]) -> usize {
        if let Some(&id) = self.archetype_ids.get(types) {
            return id;
        }
        let descs: Vec<ComponentDescriptor> = types
            .iter()
            .map(|t| {
                self.registry
                    .descriptor(t)
                    .expect("archetype types must be registered")
            })
            .collect();
        let id = self.archetypes.len();
        self.archetypes
            .push(Archetype::new(id, types.to_vec(), &descs));
        self.archetype_ids.insert(types.to_vec(), id);
        id
    }

    // ── lifecycle ────────────────────────────────────────────────────────

    /// Creates an empty entity.
    pub fn create_empty(&mut self) -> Entity {
        let e = self.reserve_entity();
        self.create_into(e);
        e
    }

    /// Finalizes a previously reserved entity into the world (empty archetype).
    /// No-op when the entity is already live.
    pub fn create_into(&mut self, e: Entity) {
        if self.valid(e) {
            return;
        }
        let id = self.archetype_id_for(&[]);
        let row = self.archetypes[id].len() as u32;
        // Safety: empty sources for an empty archetype.
        unsafe { self.archetypes[id].push_row(e.index(), &[]) };
        let idx = e.index() as usize;
        self.slots[idx].archetype = Some(id);
        self.slots[idx].row = row;
    }

    /// Creates an entity with one initial component.
    pub fn create1<A: 'static>(&mut self, a: A) -> WorldResult<Entity> {
        let e = self.create_empty();
        self.add(e, a)?;
        Ok(e)
    }

    /// Creates an entity with two initial components.
    pub fn create2<A: 'static, B: 'static>(&mut self, a: A, b: B) -> WorldResult<Entity> {
        let e = self.create_empty();
        self.add(e, a)?;
        self.add(e, b)?;
        Ok(e)
    }

    /// Creates an entity with three initial components.
    pub fn create3<A: 'static, B: 'static, C: 'static>(
        &mut self,
        a: A,
        b: B,
        c: C,
    ) -> WorldResult<Entity> {
        let e = self.create_empty();
        self.add(e, a)?;
        self.add(e, b)?;
        self.add(e, c)?;
        Ok(e)
    }

    /// Creates an entity with four initial components.
    pub fn create4<A: 'static, B: 'static, C: 'static, D: 'static>(
        &mut self,
        a: A,
        b: B,
        c: C,
        d: D,
    ) -> WorldResult<Entity> {
        let e = self.create_empty();
        self.add(e, a)?;
        self.add(e, b)?;
        self.add(e, c)?;
        self.add(e, d)?;
        Ok(e)
    }

    /// Adds a component (batch via repeated calls or `add_bundle`).
    pub fn add<T: 'static>(&mut self, entity: Entity, value: T) -> WorldResult<()> {
        self.registry.ensure_registered::<T>();
        if !self.valid(entity) {
            return Err(WorldError::StaleEntity);
        }
        let t = TypeId::of::<T>();
        let idx = entity.index() as usize;
        let at = self.slots[idx].archetype.unwrap();
        let row = self.slots[idx].row as usize;
        if self.archetypes[at].has(&t) {
            return Err(WorldError::InsertAlreadyExists(std::any::type_name::<T>()));
        }
        let new_types = self.ordered_types(self.archetypes[at].types(), Some(t), None);
        let nid = self.archetype_id_for(&new_types);
        let value = ManuallyDrop::new(value);
        let desc = self.registry.descriptor_of::<T>();
        let ctx = self.hook_context;
        let (old, new) = get_two_mut(&mut self.archetypes, at, nid);
        let mut sources: Vec<Vec<u8>> = Vec::with_capacity(new_types.len());
        for ty in &new_types {
            if *ty == t {
                // Safety: ManuallyDrop keeps the value alive through the copy;
                // the column takes ownership afterwards.
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        &*value as *const T as *const u8,
                        std::mem::size_of::<T>(),
                    )
                };
                sources.push(bytes.to_vec());
            } else {
                let c = old.column_index(ty).unwrap();
                // Safety: row is live in the old archetype.
                sources.push(unsafe { old.columns[c].take_bytes(row) });
            }
        }
        let ptrs: Vec<*const u8> = sources.iter().map(|b| b.as_ptr()).collect();
        let target_row = new.len() as u32;
        // Safety: every source matches the target column's descriptor.
        unsafe { new.push_row(entity.index(), &ptrs) };
        let moved = unsafe { old.remove_row_migrate(row, None) };
        if let Some(m) = moved {
            self.slots[m as usize].row = row as u32;
        }
        self.slots[idx].archetype = Some(nid);
        self.slots[idx].row = target_row;
        // Fire on_add now that the value is live in the (new) archetype.
        if let (Some(desc), Some(ctx)) = (desc, ctx)
            && let Some(on_add) = desc.hooks.and_then(|h| h.on_add)
        {
            let arch_ref = &self.archetypes[nid];
            if let Some(c) = arch_ref.column_index(&t) {
                let ptr = arch_ref.columns[c].get_ptr(target_row as usize);
                on_add(ptr as *mut u8, ctx);
            }
        }
        Ok(())
    }

    /// Adds two components in one call.
    pub fn add_bundle2<A: 'static, B: 'static>(
        &mut self,
        entity: Entity,
        a: A,
        b: B,
    ) -> WorldResult<()> {
        self.add(entity, a)?;
        self.add(entity, b)
    }

    /// Removes a component type. Missing component is a no-op (idempotent);
    /// stale entities error.
    pub fn remove<T: 'static>(&mut self, entity: Entity) -> WorldResult<()> {
        if self.registry.descriptor_of::<T>().is_none() {
            return Ok(()); // never registered: cannot exist
        }
        if !self.valid(entity) {
            return Err(WorldError::StaleEntity);
        }
        let t = TypeId::of::<T>();
        let idx = entity.index() as usize;
        let at = self.slots[idx].archetype.unwrap();
        let row = self.slots[idx].row as usize;
        if !self.archetypes[at].has(&t) {
            return Ok(()); // idempotent no-op
        }
        let new_types = self.ordered_types(self.archetypes[at].types(), None, Some(t));
        let nid = self.archetype_id_for(&new_types);
        let desc = self.registry.descriptor_of::<T>();
        let ctx = self.hook_context;
        let (old, new) = get_two_mut(&mut self.archetypes, at, nid);
        let mut sources: Vec<Vec<u8>> = Vec::with_capacity(new_types.len());
        for ty in &new_types {
            let c = old.column_index(ty).unwrap();
            // Safety: row is live in the old archetype.
            sources.push(unsafe { old.columns[c].take_bytes(row) });
        }
        let ptrs: Vec<*const u8> = sources.iter().map(|b| b.as_ptr()).collect();
        let target_row = new.len() as u32;
        // Safety: every source matches the target column's descriptor.
        unsafe { new.push_row(entity.index(), &ptrs) };
        let drop_col = old.column_index(&t);
        // Fire on_remove BEFORE the value is dropped inside remove_row_migrate.
        if let (Some(desc), Some(ctx)) = (desc, ctx)
            && let Some(on_remove) = desc.hooks.and_then(|h| h.on_remove)
        {
            let ptr = old.columns[drop_col.unwrap()].get_ptr(row);
            on_remove(ptr as *mut u8, ctx);
        }
        let moved = unsafe { old.remove_row_migrate(row, drop_col) };
        if let Some(m) = moved {
            self.slots[m as usize].row = row as u32;
        }
        self.slots[idx].archetype = Some(nid);
        self.slots[idx].row = target_row;
        Ok(())
    }

    /// Destroys an entity: drops components in registration order, frees
    /// the slot for reuse. Idempotent for stale/destroyed handles.
    pub fn destroy(&mut self, entity: Entity) -> WorldResult<()> {
        if !self.valid(entity) {
            return Ok(()); // idempotent
        }
        let ctx = self.hook_context;
        let idx = entity.index() as usize;
        let at = self.slots[idx].archetype.unwrap();
        let row = self.slots[idx].row as usize;
        // Fire on_remove once per component, before remove_row drops them.
        if let Some(ctx) = ctx {
            let arch_ref = &self.archetypes[at];
            for ci in 0..arch_ref.columns.len() {
                if let Some(on_remove) = arch_ref.columns[ci]
                    .descriptor()
                    .hooks
                    .and_then(|h| h.on_remove)
                {
                    let ptr = arch_ref.columns[ci].get_ptr(row);
                    on_remove(ptr as *mut u8, ctx);
                }
            }
        }
        let moved = self.archetypes[at].remove_row(row);
        if let Some(m) = moved {
            self.slots[m as usize].row = row as u32;
        }
        self.allocator.release(idx as u32);
        self.slots[idx].archetype = None;
        Ok(())
    }

    /// Destroys every entity, retaining the archetype structure and registry.
    pub fn clear(&mut self) {
        let released: Vec<u32> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.archetype.is_some())
            .map(|(i, _)| i as u32)
            .collect();
        let ctx = self.hook_context;
        for arch in &mut self.archetypes {
            // Fire on_remove once per live value, before drop_all drops them.
            if let Some(ctx) = ctx {
                for ci in 0..arch.columns.len() {
                    if let Some(on_remove) = arch.columns[ci]
                        .descriptor()
                        .hooks
                        .and_then(|h| h.on_remove)
                    {
                        let n = arch.len();
                        for row in 0..n {
                            let ptr = arch.columns[ci].get_ptr(row);
                            on_remove(ptr as *mut u8, ctx);
                        }
                    }
                }
            }
            arch.drop_all();
        }
        for slot in &mut self.slots {
            slot.archetype = None;
        }
        for i in released {
            self.allocator.release(i);
        }
    }

    // ── access ───────────────────────────────────────────────────────────

    /// Returns a shared reference to the component, or None when missing.
    pub fn get<T: 'static>(&self, entity: Entity) -> WorldResult<Option<&T>> {
        if !self.valid(entity) {
            return Err(WorldError::StaleEntity);
        }
        let idx = entity.index() as usize;
        let row = self.slots[idx].row as usize;
        let arch = &self.archetypes[self.slots[idx].archetype.unwrap()];
        let Some(c) = arch.column_index(&TypeId::of::<T>()) else {
            return Ok(None);
        };
        // Safety: the column is immutably borrowed for the lifetime of the
        // returned reference, and the archetype may not mutate concurrently.
        let ptr = arch.columns[c].get_ptr(row);
        Ok(Some(unsafe { &*(ptr as *const T) }))
    }

    /// Returns a mutable reference to the component, or None when missing.
    pub fn get_mut<T: 'static>(&mut self, entity: Entity) -> WorldResult<Option<&mut T>> {
        if !self.valid(entity) {
            return Err(WorldError::StaleEntity);
        }
        let idx = entity.index() as usize;
        let row = self.slots[idx].row as usize;
        let at = self.slots[idx].archetype.unwrap();
        let arch = &mut self.archetypes[at];
        let Some(c) = arch.column_index(&TypeId::of::<T>()) else {
            return Ok(None);
        };
        // Safety: &mut self gives exclusivity for the returned reference.
        let ptr = arch.columns[c].get_mut_ptr(row);
        Ok(Some(unsafe { &mut *(ptr as *mut T) }))
    }

    /// Whether an entity is alive.
    pub fn contains_entity(&self, entity: Entity) -> bool {
        self.valid(entity)
    }

    /// Whether an entity has a component type.
    pub fn contains<T: 'static>(&self, entity: Entity) -> WorldResult<bool> {
        if !self.valid(entity) {
            return Err(WorldError::StaleEntity);
        }
        let idx = entity.index() as usize;
        let arch = &self.archetypes[self.slots[idx].archetype.unwrap()];
        Ok(arch.has(&TypeId::of::<T>()))
    }

    // ── location accessors (GoHandle O(1) path, crate-internal) ────────────

    /// The `(archetype_id, row)` locating the live entity, or `None` when the
    /// handle is stale. O(1).
    pub(crate) fn location_of(&self, entity: Entity) -> Option<(usize, u32)> {
        if !self.valid(entity) {
            return None;
        }
        let idx = entity.index() as usize;
        Some((self.slots[idx].archetype.unwrap(), self.slots[idx].row))
    }

    /// The current generation of the entity's slot, or `None` once the slot is
    /// unwritten. O(1).
    pub(crate) fn live_generation(&self, entity: Entity) -> Option<u32> {
        let idx = entity.index() as usize;
        if idx >= self.slots.len() {
            return None;
        }
        Some(self.slots[idx].generation)
    }

    /// Shared access to a component at an exact `(arch, row)` position, without
    /// a slot lookup. O(1).
    pub(crate) fn get_at<T: 'static>(&self, arch: usize, row: usize) -> Option<&T> {
        let arch_ref = self.archetypes.get(arch)?;
        let c = arch_ref.column_index(&TypeId::of::<T>())?;
        if row >= arch_ref.len() {
            return None;
        }
        // Safety: `row` is a live position in the same archetype; the column
        // value is a `T` and the borrow is shared for the returned reference.
        Some(unsafe { &*(arch_ref.columns[c].get_ptr(row) as *const T) })
    }

    /// Mutable access to a component at an exact `(arch, row)` position,
    /// without a slot lookup. O(1).
    pub(crate) fn get_mut_at<T: 'static>(&mut self, arch: usize, row: usize) -> Option<&mut T> {
        let arch_ref = self.archetypes.get_mut(arch)?;
        let c = arch_ref.column_index(&TypeId::of::<T>())?;
        if row >= arch_ref.len() {
            return None;
        }
        // Safety: `&mut self` gives exclusivity; the column value is a `T`.
        Some(unsafe { &mut *(arch_ref.columns[c].get_mut_ptr(row) as *mut T) })
    }

    /// Number of live entities.
    pub fn entity_count(&self) -> usize {
        self.slots.iter().filter(|s| s.archetype.is_some()).count()
    }

    /// Iterates all live entity handles.
    pub fn entities(&self) -> Vec<Entity> {
        let mut out = Vec::new();
        for arch in &self.archetypes {
            for row in 0..arch.len() {
                let eidx = arch.entity_at(row);
                out.push(Entity::from_parts(
                    eidx,
                    self.slots[eidx as usize].generation,
                ));
            }
        }
        out
    }

    // ── iteration / query ────────────────────────────────────────────────

    /// Iterates all entities holding component T (single).
    pub fn iterate<T: 'static>(&self, mut f: impl FnMut(Entity, &T)) {
        let t = TypeId::of::<T>();
        for arch in &self.archetypes {
            if let Some(c) = arch.column_index(&t) {
                for row in 0..arch.len() {
                    let eidx = arch.entity_at(row);
                    let e = Entity::from_parts(eidx, self.slots[eidx as usize].generation);
                    // Safety: immutable borrow of the column; no mutation path.
                    let ptr = arch.columns[c].get_ptr(row);
                    f(e, unsafe { &*(ptr as *const T) });
                }
            }
        }
    }

    /// Iterates all entities holding component T mutably (single).
    pub fn iterate_mut<T: 'static>(&mut self, mut f: impl FnMut(Entity, &mut T)) {
        let t = TypeId::of::<T>();
        let arch_ids: Vec<usize> = self
            .archetypes
            .iter()
            .enumerate()
            .filter(|(_, a)| a.has(&t))
            .map(|(i, _)| i)
            .collect();
        for aid in arch_ids {
            let arch = &mut self.archetypes[aid];
            let c = arch.column_index(&t).unwrap();
            for row in 0..arch.len() {
                let eidx = arch.entity_at(row);
                let e = Entity::from_parts(eidx, self.slots[eidx as usize].generation);
                // Safety: &mut self gives exclusivity for the returned ref.
                let ptr = arch.columns[c].get_mut_ptr(row);
                f(e, unsafe { &mut *(ptr as *mut T) });
            }
        }
    }

    /// Joins two components (intersection iteration).
    ///
    /// Returns `Err(BorrowConflict)` when both parameters name the same
    /// component type (duplicate mutable access is rejected deterministically).
    pub fn query2<A: 'static, B: 'static>(
        &mut self,
        mut f: impl FnMut(Entity, &mut A, &mut B),
    ) -> WorldResult<()> {
        let ta = TypeId::of::<A>();
        let tb = TypeId::of::<B>();
        if ta == tb {
            return Err(WorldError::BorrowConflict(std::any::type_name::<A>()));
        }
        let arch_ids: Vec<usize> = self
            .archetypes
            .iter()
            .enumerate()
            .filter(|(_, a)| a.has(&ta) && a.has(&tb))
            .map(|(i, _)| i)
            .collect();
        for aid in arch_ids {
            let arch = &mut self.archetypes[aid];
            let ca = arch.column_index(&ta).unwrap();
            let cb = arch.column_index(&tb).unwrap();
            let n = arch.len();
            let ents: Vec<u32> = (0..n).map(|r| arch.entity_at(r)).collect();
            let (col_a, col_b) = get_two_mut(&mut arch.columns, ca, cb);
            for (row, &eidx) in ents.iter().enumerate() {
                let e = Entity::from_parts(eidx, self.slots[eidx as usize].generation);
                // Safety: &mut self gives exclusivity for both refs.
                let pa = col_a.get_mut_ptr(row);
                let pb = col_b.get_mut_ptr(row);
                f(e, unsafe { &mut *(pa as *mut A) }, unsafe {
                    &mut *(pb as *mut B)
                });
            }
        }
        Ok(())
    }

    /// Joins three components (intersection iteration, <= 3 requirement).
    ///
    /// Returns `Err(BorrowConflict)` when any two parameters name the same
    /// component type (duplicate mutable access is rejected deterministically).
    pub fn query3<A: 'static, B: 'static, C: 'static>(
        &mut self,
        mut f: impl FnMut(Entity, &mut A, &mut B, &mut C),
    ) -> WorldResult<()> {
        let ta = TypeId::of::<A>();
        let tb = TypeId::of::<B>();
        let tc = TypeId::of::<C>();
        if ta == tb || ta == tc {
            return Err(WorldError::BorrowConflict(std::any::type_name::<A>()));
        }
        if tb == tc {
            return Err(WorldError::BorrowConflict(std::any::type_name::<B>()));
        }
        let arch_ids: Vec<usize> = self
            .archetypes
            .iter()
            .enumerate()
            .filter(|(_, a)| a.has(&ta) && a.has(&tb) && a.has(&tc))
            .map(|(i, _)| i)
            .collect();
        for aid in arch_ids {
            let arch = &mut self.archetypes[aid];
            let ca = arch.column_index(&ta).unwrap();
            let cb = arch.column_index(&tb).unwrap();
            let cc = arch.column_index(&tc).unwrap();
            let n = arch.len();
            let ents: Vec<u32> = (0..n).map(|r| arch.entity_at(r)).collect();
            // Sort indices ascending, split the slice three ways.
            let mut cols = [ca, cb, cc];
            cols.sort_unstable();
            let (lo, mid, hi) = (cols[0], cols[1], cols[2]);
            let (l, r) = arch.columns.split_at_mut(mid);
            let (r_l, r_r) = r.split_at_mut(hi - mid);
            let col_mid = &mut r_l[0];
            let col_hi = &mut r_r[0];
            let (_, l_r) = l.split_at_mut(lo);
            let col_lo = &mut l_r[0];
            for (row, &eidx) in ents.iter().enumerate() {
                let e = Entity::from_parts(eidx, self.slots[eidx as usize].generation);
                // Safety: &mut self gives exclusivity for all three refs.
                let (pa, pb, pc) = if ca == lo && cb == mid {
                    (
                        col_lo.get_mut_ptr(row),
                        col_mid.get_mut_ptr(row),
                        col_hi.get_mut_ptr(row),
                    )
                } else if ca == lo && cc == mid {
                    (
                        col_lo.get_mut_ptr(row),
                        col_hi.get_mut_ptr(row),
                        col_mid.get_mut_ptr(row),
                    )
                } else if cb == lo && ca == mid {
                    (
                        col_mid.get_mut_ptr(row),
                        col_lo.get_mut_ptr(row),
                        col_hi.get_mut_ptr(row),
                    )
                } else if cb == lo && cc == mid {
                    (
                        col_hi.get_mut_ptr(row),
                        col_lo.get_mut_ptr(row),
                        col_mid.get_mut_ptr(row),
                    )
                } else if cc == lo && ca == mid {
                    (
                        col_mid.get_mut_ptr(row),
                        col_hi.get_mut_ptr(row),
                        col_lo.get_mut_ptr(row),
                    )
                } else {
                    (
                        col_hi.get_mut_ptr(row),
                        col_mid.get_mut_ptr(row),
                        col_lo.get_mut_ptr(row),
                    )
                };
                f(
                    e,
                    unsafe { &mut *(pa as *mut A) },
                    unsafe { &mut *(pb as *mut B) },
                    unsafe { &mut *(pc as *mut C) },
                );
            }
        }
        Ok(())
    }

    // ── resources ────────────────────────────────────────────────────────

    /// Inserts (or replaces) a singleton resource.
    pub fn insert_resource<R: 'static>(&mut self, value: R) {
        self.resources.insert(TypeId::of::<R>(), Box::new(value));
    }

    /// Shared access to a resource; errors when absent.
    pub fn get_resource<R: 'static>(&self) -> WorldResult<Option<&R>> {
        match self.resources.get(&TypeId::of::<R>()) {
            Some(b) => Ok(Some(b.downcast_ref::<R>().expect("resource type tag"))),
            None => Ok(None),
        }
    }

    /// Mutable access to a resource; errors when absent.
    pub fn get_resource_mut<R: 'static>(&mut self) -> WorldResult<Option<&mut R>> {
        match self.resources.get_mut(&TypeId::of::<R>()) {
            Some(b) => Ok(Some(b.downcast_mut::<R>().expect("resource type tag"))),
            None => Ok(None),
        }
    }

    /// Removes a resource.
    pub fn remove_resource<R: 'static>(&mut self) {
        self.resources.remove(&TypeId::of::<R>());
    }

    // ── commands ─────────────────────────────────────────────────────────

    /// Returns a deferred command builder for the current system.
    pub fn commands(&mut self) -> Commands<'_> {
        Commands::new(self)
    }

    /// Applies all queued commands in order (called at system boundaries).
    ///
    /// Every queued command runs (order preserved); the FIRST error raised
    /// by any command is returned, matching the synchronous API semantics.
    pub fn flush_commands(&mut self) -> WorldResult<()> {
        let pending = self.queue.take();
        let mut first_error = None;
        for f in pending {
            if let Err(e) = f(self)
                && first_error.is_none()
            {
                first_error = Some(e);
            }
        }
        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    pub(crate) fn queue(&mut self) -> &mut CommandQueue {
        &mut self.queue
    }
}

/// Borrows two distinct elements of a slice mutably.
fn get_two_mut<T>(v: &mut [T], a: usize, b: usize) -> (&mut T, &mut T) {
    assert_ne!(a, b, "distinct indices required");
    if a < b {
        let (left, right) = v.split_at_mut(b);
        (&mut left[a], &mut right[0])
    } else {
        let (left, right) = v.split_at_mut(a);
        (&mut right[0], &mut left[b])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Debug, PartialEq)]
    struct Position(f32, f32);
    #[derive(Debug, PartialEq)]
    struct Velocity(f32, f32);
    #[derive(Debug, PartialEq)]
    struct Health(i32);
    #[derive(Debug, PartialEq)]
    struct Script(i32);

    #[test]
    fn create_contains_get() {
        let mut w = World::new();
        let e = w.create2(Position(1.0, 2.0), Velocity(3.0, 4.0)).unwrap();
        assert!(w.contains_entity(e));
        assert!(w.contains::<Position>(e).unwrap());
        assert!(!w.contains::<Health>(e).unwrap());
        assert_eq!(w.get::<Position>(e).unwrap(), Some(&Position(1.0, 2.0)));
        assert_eq!(w.entity_count(), 1);
    }

    #[test]
    fn stale_handle_access_errors_and_destroy_idempotent() {
        let mut w = World::new();
        let e = w.create1(Position(0.0, 0.0)).unwrap();
        w.destroy(e).unwrap();
        assert!(!w.contains_entity(e));
        assert!(matches!(w.get::<Position>(e), Err(WorldError::StaleEntity)));
        // Idempotent destroy.
        w.destroy(e).unwrap();
        // Reuse bumps generation: old handle stays stale.
        let e2 = w.create_empty();
        assert_ne!(e, e2);
        assert!(matches!(w.get::<Position>(e), Err(WorldError::StaleEntity)));
    }

    #[test]
    fn add_duplicate_errors_and_state_unchanged() {
        let mut w = World::new();
        let e = w.create1(Position(1.0, 0.0)).unwrap();
        let err = w.add(e, Position(9.0, 9.0)).unwrap_err();
        assert!(matches!(err, WorldError::InsertAlreadyExists(_)));
        assert_eq!(w.get::<Position>(e).unwrap(), Some(&Position(1.0, 0.0)));
    }

    #[test]
    fn remove_missing_is_noop() {
        let mut w = World::new();
        let e = w.create1(Position(1.0, 0.0)).unwrap();
        w.remove::<Health>(e).unwrap(); // never registered -> Ok
        w.remove::<Velocity>(e).unwrap(); // registered? auto-registered by create2? Position only; remove Velocity no-op Ok.
        assert!(w.contains::<Position>(e).unwrap());
    }

    #[test]
    fn remove_present_migrates_archetype() {
        let mut w = World::new();
        let e = w.create2(Position(1.0, 2.0), Velocity(3.0, 4.0)).unwrap();
        w.remove::<Velocity>(e).unwrap();
        assert!(w.contains::<Position>(e).unwrap());
        assert!(!w.contains::<Velocity>(e).unwrap());
        assert_eq!(w.get::<Position>(e).unwrap(), Some(&Position(1.0, 2.0)));
        // Re-add works after removal.
        w.add(e, Velocity(5.0, 6.0)).unwrap();
        assert_eq!(w.get::<Velocity>(e).unwrap(), Some(&Velocity(5.0, 6.0)));
    }

    #[test]
    fn drop_order_follows_registration_order() {
        struct A(Rc<RefCell<Vec<&'static str>>>);
        struct B(Rc<RefCell<Vec<&'static str>>>);
        struct C(Rc<RefCell<Vec<&'static str>>>);
        impl Drop for A {
            fn drop(&mut self) {
                self.0.borrow_mut().push("A");
            }
        }
        impl Drop for B {
            fn drop(&mut self) {
                self.0.borrow_mut().push("B");
            }
        }
        impl Drop for C {
            fn drop(&mut self) {
                self.0.borrow_mut().push("C");
            }
        }
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut w = World::new();
        // Explicit registration order A, B, C:
        w.register::<A>(false).unwrap();
        w.register::<B>(false).unwrap();
        w.register::<C>(false).unwrap();
        let e = w
            .create3(A(log.clone()), B(log.clone()), C(log.clone()))
            .unwrap();
        w.destroy(e).unwrap();
        assert_eq!(*log.borrow(), vec!["A", "B", "C"]);
    }

    #[test]
    fn drop_runs_once_per_component() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Tracked(Arc<AtomicUsize>);
        impl Drop for Tracked {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let c = Arc::new(AtomicUsize::new(0));
        let mut w = World::new();
        let e = w.create1(Tracked(c.clone())).unwrap();
        w.destroy(e).unwrap();
        assert_eq!(c.load(Ordering::SeqCst), 1);
        // clear(): all components dropped exactly once.
        let _e2 = w.create1(Tracked(c.clone())).unwrap();
        let _e3 = w.create1(Tracked(c.clone())).unwrap();
        w.clear();
        assert_eq!(c.load(Ordering::SeqCst), 3);
        // world continues to work after clear
        let e4 = w.create1(Tracked(c.clone())).unwrap();
        assert!(w.contains::<Tracked>(e4).unwrap());
    }

    #[test]
    fn iterate_single_and_query2_intersection() {
        let mut w = World::new();
        let a = w.create2(Position(0.0, 0.0), Velocity(1.0, 1.0)).unwrap();
        let b = w.create1(Position(9.0, 9.0)).unwrap();
        let c = w.create2(Position(3.0, 3.0), Velocity(2.0, 2.0)).unwrap();
        let mut seen = Vec::new();
        w.iterate::<Position>(|e, p| seen.push((e, p.0)));
        assert_eq!(seen.len(), 3);
        let mut join = Vec::new();
        w.query2::<Position, Velocity>(|e, p, v| join.push((e, p.0, v.0)))
            .unwrap();
        assert_eq!(join.len(), 2);
        assert!(join.iter().any(|(e, _, _)| *e == a));
        assert!(join.iter().any(|(e, _, _)| *e == c));
        // remove A from a -> a disappears from join
        w.remove::<Velocity>(a).unwrap();
        let mut join2 = Vec::new();
        w.query2::<Position, Velocity>(|e, _, _| join2.push(e))
            .unwrap();
        assert_eq!(join2.len(), 1);
        assert_eq!(join2[0], c);
        let _ = b;
    }

    #[test]
    fn query3_and_archetype_migration() {
        let mut w = World::new();
        let e = w
            .create3(Position(1.0, 1.0), Velocity(1.0, 1.0), Health(10))
            .unwrap();
        let mut count = 0;
        w.query3::<Position, Velocity, Health>(|_, _, _, _| count += 1)
            .unwrap();
        assert_eq!(count, 1);
        w.remove::<Health>(e).unwrap();
        let mut count2 = 0;
        w.query3::<Position, Velocity, Health>(|_, _, _, _| count2 += 1)
            .unwrap();
        assert_eq!(count2, 0);
        // add it back via bundle2
        w.add_bundle2(e, Health(5), Script(0)).unwrap();
        let mut count3 = 0;
        w.query3::<Position, Velocity, Health>(|_, _, _, _| count3 += 1)
            .unwrap();
        assert_eq!(count3, 1);
    }

    #[test]
    fn resources_lifecycle() {
        let mut w = World::new();
        assert!(matches!(w.get_resource::<Health>(), Ok(None)));
        w.insert_resource(Health(77));
        assert_eq!(w.get_resource::<Health>().unwrap().unwrap().0, 77);
        {
            let r = w.get_resource_mut::<Health>().unwrap().unwrap();
            r.0 += 1;
        }
        assert_eq!(w.get_resource::<Health>().unwrap().unwrap().0, 78);
        w.remove_resource::<Health>();
        assert!(w.get_resource::<Health>().unwrap().is_none());
    }

    #[test]
    fn commands_flush_in_order_with_sync_semantics() {
        let mut w = World::new();
        let (first, second) = {
            let mut cmds = w.commands();
            let first = cmds.create1(Position(1.0, 1.0));
            let second = cmds.create2(Position(2.0, 2.0), Velocity(9.0, 9.0));
            cmds.add(first, Velocity(3.0, 3.0));
            cmds.destroy(second);
            (first, second)
        };
        // Nothing applied yet.
        assert_eq!(w.entity_count(), 0);
        w.flush_commands().unwrap();
        assert_eq!(w.entity_count(), 1);
        assert!(w.contains::<Position>(first).unwrap());
        assert!(w.contains::<Velocity>(first).unwrap());
        assert!(!w.contains_entity(second));
    }

    #[test]
    fn commands_propagate_errors_on_flush() {
        let mut w = World::new();
        let stale = {
            let e = w.create1(Position(1.0, 1.0)).unwrap();
            w.destroy(e).unwrap();
            e
        };
        {
            let mut cmds = w.commands();
            cmds.remove::<Position>(stale); // stale -> Err(StaleEntity)
        }
        let err = w.flush_commands().unwrap_err();
        assert!(matches!(err, WorldError::StaleEntity));
    }

    #[test]
    fn query_same_type_is_rejected_deterministically() {
        let mut w = World::new();
        w.create1(Position(0.0, 0.0)).unwrap();
        let err = w.query2::<Position, Position>(|_, _, _| {}).unwrap_err();
        assert!(matches!(err, WorldError::BorrowConflict(_)));
        let err = w
            .query3::<Position, Velocity, Position>(|_, _, _, _| {})
            .unwrap_err();
        assert!(matches!(err, WorldError::BorrowConflict(_)));
    }

    #[test]
    fn scriptable_registration_path() {
        let mut w = World::new();
        w.register::<Script>(true).unwrap();
        let e = w.create1(Script(5)).unwrap();
        // Same lifecycle semantics as native components.
        assert_eq!(w.get::<Script>(e).unwrap(), Some(&Script(5)));
        w.destroy(e).unwrap();
    }

    // ── component hook lifecycle (core-ecs) ────────────────────────────────

    mod hooks {
        use super::*;
        use crate::component::ComponentHooks;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Debug, PartialEq)]
        struct Tracked(u32);
        #[derive(Debug, PartialEq)]
        struct Other(u32);

        /// Per-test hook state carried through the bound context pointer.
        struct HookCounter {
            add: AtomicUsize,
            remove: AtomicUsize,
        }
        impl HookCounter {
            fn new() -> Self {
                Self {
                    add: AtomicUsize::new(0),
                    remove: AtomicUsize::new(0),
                }
            }
            /// A stable context pointer used only to route the counters.
            fn ctx(&self) -> *mut () {
                self as *const Self as *mut ()
            }
        }

        fn make_hooks() -> ComponentHooks {
            // Safety: the ctx pointer bound by each test points at a live
            // `HookCounter`; the hooks read its atomics.
            fn on_add(_: *mut u8, ctx: *mut ()) {
                let c = unsafe { &*(ctx as *const HookCounter) };
                c.add.fetch_add(1, Ordering::SeqCst);
            }
            fn on_remove(_: *mut u8, ctx: *mut ()) {
                let c = unsafe { &*(ctx as *const HookCounter) };
                c.remove.fetch_add(1, Ordering::SeqCst);
            }
            ComponentHooks {
                on_add: Some(on_add),
                on_remove: Some(on_remove),
            }
        }

        #[test]
        fn add_remove_destroy_fire_once() {
            let counter = HookCounter::new();
            let mut w = World::new();
            w.register_component_meta::<Tracked>(make_hooks()).unwrap();
            unsafe { w.bind_hook_context(counter.ctx()) };
            let e = w.create1(Tracked(1)).unwrap();
            assert_eq!(counter.add.load(Ordering::SeqCst), 1, "on_add fires once");
            assert_eq!(counter.remove.load(Ordering::SeqCst), 0);
            w.remove::<Tracked>(e).unwrap();
            assert_eq!(
                counter.remove.load(Ordering::SeqCst),
                1,
                "on_remove fires once"
            );
            assert_eq!(counter.add.load(Ordering::SeqCst), 1);
            w.add(e, Tracked(2)).unwrap();
            assert_eq!(counter.add.load(Ordering::SeqCst), 2, "re-add fires on_add");
            w.destroy(e).unwrap();
            assert_eq!(
                counter.remove.load(Ordering::SeqCst),
                2,
                "destroy fires on_remove once"
            );
        }

        #[test]
        fn migration_does_not_fire_for_moved_components() {
            let counter = HookCounter::new();
            let mut w = World::new();
            w.register_component_meta::<Tracked>(make_hooks()).unwrap();
            unsafe { w.bind_hook_context(counter.ctx()) };
            let e = w.create1(Tracked(1)).unwrap();
            assert_eq!(counter.add.load(Ordering::SeqCst), 1);
            // Adding a second (hook-less) component migrates Tracked bitwise.
            w.add(e, Other(9)).unwrap();
            assert_eq!(
                counter.add.load(Ordering::SeqCst),
                1,
                "adding Other must not re-fire Tracked's on_add"
            );
            assert_eq!(
                counter.remove.load(Ordering::SeqCst),
                0,
                "migrated (kept) component must not fire on_remove"
            );
            // Removing Other migrates Tracked again.
            w.remove::<Other>(e).unwrap();
            assert_eq!(counter.add.load(Ordering::SeqCst), 1);
            assert_eq!(counter.remove.load(Ordering::SeqCst), 0);
            // Removing the tracked component itself fires on_remove exactly once.
            w.remove::<Tracked>(e).unwrap();
            assert_eq!(counter.remove.load(Ordering::SeqCst), 1);
        }

        #[test]
        fn clear_fires_on_remove_for_every_live_value() {
            let counter = HookCounter::new();
            let mut w = World::new();
            w.register_component_meta::<Tracked>(make_hooks()).unwrap();
            unsafe { w.bind_hook_context(counter.ctx()) };
            let _ = w.create1(Tracked(1)).unwrap();
            let _ = w.create1(Tracked(2)).unwrap();
            let _ = w.create1(Tracked(3)).unwrap();
            assert_eq!(counter.add.load(Ordering::SeqCst), 3);
            assert_eq!(counter.remove.load(Ordering::SeqCst), 0);
            w.clear();
            assert_eq!(
                counter.remove.load(Ordering::SeqCst),
                3,
                "clear fires on_remove per value"
            );
        }

        #[test]
        fn commands_path_fires_identically() {
            let counter = HookCounter::new();
            let mut w = World::new();
            w.register_component_meta::<Tracked>(make_hooks()).unwrap();
            unsafe { w.bind_hook_context(counter.ctx()) };
            let e = {
                let mut cmds = w.commands();
                cmds.create1(Tracked(1))
            };
            assert_eq!(counter.add.load(Ordering::SeqCst), 0, "not yet flushed");
            w.flush_commands().unwrap();
            assert_eq!(counter.add.load(Ordering::SeqCst), 1);
            {
                let mut cmds = w.commands();
                cmds.destroy(e);
            }
            assert_eq!(counter.remove.load(Ordering::SeqCst), 0);
            w.flush_commands().unwrap();
            assert_eq!(counter.remove.load(Ordering::SeqCst), 1);
        }

        #[test]
        fn unbound_context_skips_hooks() {
            let counter = HookCounter::new();
            let mut w = World::new();
            w.register_component_meta::<Tracked>(make_hooks()).unwrap();
            // No bind_hook_context -> hooks must be skipped.
            let _ = w.create1(Tracked(1)).unwrap();
            assert_eq!(counter.add.load(Ordering::SeqCst), 0);
            assert_eq!(counter.remove.load(Ordering::SeqCst), 0);
        }

        #[test]
        fn duplicate_registration_preserves_first_hooks() {
            let counter = HookCounter::new();
            let mut w = World::new();
            w.register_component_meta::<Tracked>(make_hooks()).unwrap();
            let err = w.register::<Tracked>(false).unwrap_err();
            assert!(matches!(err, WorldError::DuplicateRegistration(_)));
            unsafe { w.bind_hook_context(counter.ctx()) };
            let e = w.create1(Tracked(1)).unwrap();
            assert_eq!(
                counter.add.load(Ordering::SeqCst),
                1,
                "first registration's hooks must survive"
            );
            let _ = e;
        }

        #[test]
        fn hookless_type_zero_impact() {
            let counter = HookCounter::new();
            let mut w = World::new();
            unsafe { w.bind_hook_context(counter.ctx()) };
            // Other is auto-registered without hooks; context is bound but the
            // descriptor has no hooks so nothing may fire.
            let e = w.create1(Other(5)).unwrap();
            assert_eq!(counter.add.load(Ordering::SeqCst), 0);
            w.destroy(e).unwrap();
            assert_eq!(counter.remove.load(Ordering::SeqCst), 0);
        }
    }
}
