//! Derived world transform cache + dirty-driven two-phase propagation.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use xengine_math::Matrix4F;

use crate::World;
use crate::entity::Entity;
use crate::parallel::JobSystem;
use crate::system::{AccessKind, Stage, System};
use crate::world::WorldReadView;

use super::component::Component;
use super::hierarchy::{Children, Parent};
use super::transform::Transform;

/// Derived cache component: the game object's world transform (local→world).
///
/// This is **not** part of the component trio and is optional — an entity
/// without a `GlobalTransform` is simply skipped by propagation. It is written
/// by the propagation system and read by the render collector.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlobalTransform {
    pub world: Matrix4F,
}

impl Component for GlobalTransform {}

/// Marker component set by the Scene transform set APIs, cleared by
/// propagation. Presence means "recompute this entity's world transform".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TransformDirty;

impl Component for TransformDirty {}

/// Hard bound on the ancestor chain walked per entity.
///
/// The hierarchy API rejects cycles, but a hand-built malformed cycle must not
/// hang propagation: the walk stops at this depth and treats that node as a
/// root (previously a per-entity `HashSet` did this job, at one allocation per
/// entity on the hot path). Realistic hierarchies are a few dozen levels deep,
/// so this bound is far beyond any legitimate tree while still terminating.
pub const MAX_HIERARCHY_DEPTH: usize = 16_384;

/// Scratch buffers owned by the phase-1 walk on the calling thread.
#[derive(Default)]
struct WalkScratch {
    stack: Vec<Entity>,
    children: Vec<Entity>,
}

thread_local! {
    /// Per-thread ancestor chain reused across entities (no per-entity heap
    /// allocation in the propagation hot path).
    static CHAIN_SCRATCH: RefCell<Vec<Entity>> = const { RefCell::new(Vec::new()) };
    /// Per-thread chunk result buffer, copied into the shared output under a
    /// short lock so the lock is never held while computing.
    static RESULT_SCRATCH: RefCell<Vec<Matrix4F>> = const { RefCell::new(Vec::new()) };
    /// Phase 1 walk buffers (stack + child-list copy), reused every run.
    static WALK_SCRATCH: RefCell<WalkScratch> = RefCell::new(WalkScratch::default());
    /// Timing of the most recent propagation on this thread.
    static LAST_TIMING: Cell<Option<PropagateTiming>> = const { Cell::new(None) };
}

/// Per-phase timing of one propagation run.
///
/// This exists so `cargo bench` (and tuning work) can see where propagation
/// spends its time: phase 2a is the parallel section, phase 2b the sequential
/// apply, `scan` the hierarchy marking of phase 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropagateTiming {
    /// Entities recomputed by this run.
    pub entities: usize,
    /// Phase 1 duration (dirty scan + descendant marking).
    pub scan: Duration,
    /// Phase 2a duration (world transform computation).
    pub compute: Duration,
    /// Phase 2b duration (writes, dirty clearing, callbacks).
    pub apply: Duration,
    /// Whether phase 2a ran on a job system.
    pub parallel: bool,
}

/// Timing of the most recent propagation executed **on this thread**.
pub fn last_propagate_timing() -> Option<PropagateTiming> {
    LAST_TIMING.with(|cell| cell.get())
}

/// Recomputes the world transform for `entity` by independently walking its own
/// ancestor chain: `world(e) = trs(e) · trs(parent) · … · trs(root)` under the
/// crate row-vector `mul` convention (the rightmost factor is applied first, so
/// an e-local point is pushed up through each ancestor's local transform).
///
/// The computation is order-independent — it never reads an ancestor's
/// `GlobalTransform`, only each ancestor's local `Transform` — which is exactly
/// what makes phase 2a parallel. `chain` is caller-owned scratch (see
/// `CHAIN_SCRATCH`), so the hot path stays allocation free.
fn compute_world(view: &WorldReadView<'_>, entity: Entity, chain: &mut Vec<Entity>) -> Matrix4F {
    chain.clear();
    let mut node = entity;
    for _ in 0..MAX_HIERARCHY_DEPTH {
        chain.push(node);
        match view.get::<Parent>(node).and_then(|parent| parent.parent) {
            Some(parent) => node = parent,
            None => break,
        }
    }
    let mut acc = local_trs(view, chain[0]);
    for &ancestor in &chain[1..] {
        acc = acc.mul(&local_trs(view, ancestor));
    }
    acc
}

fn local_trs(view: &WorldReadView<'_>, entity: Entity) -> Matrix4F {
    match view.get::<Transform>(entity) {
        Some(t) => Matrix4F::from_trs(t.position, &t.rotate, t.scale),
        None => Matrix4F::IDENTITY,
    }
}

/// Number of entities computed per parallel work item of phase 2a.
///
/// Every chunk writes disjoint slots of the shared result buffer, so the
/// parallel phase is data-race free without any raw-pointer column splitting.
pub const PROPAGATE_CHUNK: usize = 64;

/// Two-phase propagation.
///
/// **Phase 1 (sequential, reads hierarchy edges):** from every dirty entity,
/// walk the `Children` subtree and mark every descendant *to recompute* (parent
/// movement must reach the whole subtree, even if a child's local transform did
/// not change).
///
/// **Phase 2a (parallel compute, strictly read-only):** every marked entity
/// recomputes independently from its own ancestor chain through the shared
/// [`WorldReadView`] and stores the result in a pre-allocated buffer. Nothing is
/// written into the world and no structure changes here.
///
/// **Phase 2b (sequential apply):** the buffered matrices are written into each
/// entity's `GlobalTransform`, the `TransformDirty` markers are cleared (the
/// only structural change of propagation, hence sequential) and `on_recompute`
/// fires once per entity, in the deterministic order of the to-recompute set.
pub fn propagate(world: &mut World) {
    propagate_inner(world, None, |_| {});
}

/// Parallel propagation entry point: identical results to [`propagate`], with
/// phase 2a computed on `jobs`.
pub fn propagate_with_jobs(world: &mut World, jobs: &JobSystem) {
    propagate_inner(world, Some(jobs), |_| {});
}

/// Propagate with an explicit job system (parallel phase 2a) or without one
/// (serial, equivalent results).
///
/// `on_recompute` is a test/observability hook invoked once per recomputed
/// entity.
pub(crate) fn propagate_inner(
    world: &mut World,
    jobs: Option<&JobSystem>,
    mut on_recompute: impl FnMut(Entity),
) {
    // Phase 1: gather initially-dirty roots and mark all descendants.
    let scan_start = Instant::now();
    let mut roots: Vec<Entity> = Vec::new();
    world.iterate::<TransformDirty>(|e, _| roots.push(e));
    let mut visited: HashSet<Entity> = roots.iter().copied().collect();
    WALK_SCRATCH.with(|cell| {
        let mut scratch = cell.borrow_mut();
        let mut stack = std::mem::take(&mut scratch.stack);
        let mut children = std::mem::take(&mut scratch.children);
        stack.clear();
        stack.extend_from_slice(&roots);
        while let Some(e) = stack.pop() {
            children.clear();
            if let Ok(Some(list)) = world.get::<Children>(e) {
                children.extend_from_slice(&list.children);
            }
            for &child in &children {
                if visited.insert(child) {
                    if !world.contains::<TransformDirty>(child).unwrap_or(false) {
                        let _ = world.add(child, TransformDirty);
                    }
                    stack.push(child);
                }
            }
        }
        scratch.stack = stack;
        scratch.children = children;
    });
    let scan = scan_start.elapsed();

    // The to-recompute set is collected in iteration order, which makes both the
    // result layout and the callback order deterministic.
    let mut to_recompute: Vec<Entity> = Vec::new();
    world.iterate::<TransformDirty>(|e, _| to_recompute.push(e));
    if to_recompute.is_empty() {
        LAST_TIMING.with(|cell| {
            cell.set(Some(PropagateTiming {
                entities: 0,
                scan,
                compute: Duration::ZERO,
                apply: Duration::ZERO,
                parallel: false,
            }))
        });
        return;
    }
    let job_system = jobs.filter(|jobs| jobs.worker_count() > 0);
    let parallel = job_system.is_some();

    // Phase 2a: compute world transforms into `results` (read-only world access).
    // One buffer per run; per-entity work stays allocation free (thread-local
    // chain + chunk scratch), a reusable run buffer is a follow-up optimisation.
    let mut results: Vec<Matrix4F> = vec![Matrix4F::IDENTITY; to_recompute.len()];
    let compute_start = Instant::now();
    {
        let view = world.read_view();
        let entities: &[Entity] = &to_recompute;
        match job_system {
            Some(jobs) => {
                // Each work item owns a whole chunk of the result buffer, so the
                // lock is only held for the copy, never while computing.
                let out = Mutex::new(&mut results[..]);
                let chunks = entities.len().div_ceil(PROPAGATE_CHUNK);
                jobs.parallel_for(0..chunks as u32, 1, |first, last| {
                    for chunk in first..last {
                        let lo = chunk as usize * PROPAGATE_CHUNK;
                        let hi = (lo + PROPAGATE_CHUNK).min(entities.len());
                        let len = hi - lo;
                        CHAIN_SCRATCH.with(|chain| {
                            let mut chain = chain.borrow_mut();
                            RESULT_SCRATCH.with(|local| {
                                let mut local = local.borrow_mut();
                                local.clear();
                                local.resize(len, Matrix4F::IDENTITY);
                                for (slot, entity) in entities[lo..hi].iter().enumerate() {
                                    local[slot] = compute_world(&view, *entity, &mut chain);
                                }
                                let mut guard = out.lock().unwrap_or_else(|e| e.into_inner());
                                guard[lo..hi].copy_from_slice(&local);
                            });
                        });
                    }
                });
            }
            None => CHAIN_SCRATCH.with(|chain| {
                let mut chain = chain.borrow_mut();
                for (slot, entity) in entities.iter().enumerate() {
                    results[slot] = compute_world(&view, *entity, &mut chain);
                }
            }),
        }
    }
    let compute = compute_start.elapsed();

    // Phase 2b: sequential apply. Clearing `TransformDirty` migrates entities
    // between archetypes, which is why every structural change stays here.
    let apply_start = Instant::now();
    for (slot, entity) in to_recompute.iter().enumerate() {
        let matrix = results[slot];
        if world.contains::<GlobalTransform>(*entity).unwrap_or(false)
            && let Ok(Some(gt)) = world.get_mut::<GlobalTransform>(*entity)
        {
            gt.world = matrix;
        }
        if world.contains::<TransformDirty>(*entity).unwrap_or(false) {
            let _ = world.remove::<TransformDirty>(*entity);
        }
        on_recompute(*entity);
    }
    LAST_TIMING.with(|cell| {
        cell.set(Some(PropagateTiming {
            entities: to_recompute.len(),
            scan,
            compute,
            apply: apply_start.elapsed(),
            parallel,
        }))
    });
}

/// Builds the parallel `transform_propagate` post-update system, ordered after
/// `hierarchy_maintain` so it sees a consistent hierarchy.
///
/// Phase 2a runs on `jobs`; the results are byte-identical to
/// [`transform_propagate_system_single_threaded`] whatever the worker count.
pub fn transform_propagate_system(jobs: Arc<JobSystem>) -> System {
    System::with_spec(
        "transform_propagate",
        Stage::PostUpdate,
        &[
            ("Transform", AccessKind::Read),
            ("Parent", AccessKind::Read),
            ("Children", AccessKind::Read),
            ("TransformDirty", AccessKind::Write),
            ("GlobalTransform", AccessKind::Write),
        ],
        None,
        Some("hierarchy_maintain"),
        move |world| propagate_inner(world, Some(&jobs), |_| {}),
    )
}

/// Single-threaded equivalent of [`transform_propagate_system`], for hosts that
/// do not want a worker pool involved.
pub fn transform_propagate_system_single_threaded() -> System {
    System::with_spec(
        "transform_propagate",
        Stage::PostUpdate,
        &[
            ("Transform", AccessKind::Read),
            ("Parent", AccessKind::Read),
            ("Children", AccessKind::Read),
            ("TransformDirty", AccessKind::Write),
            ("GlobalTransform", AccessKind::Write),
        ],
        None,
        Some("hierarchy_maintain"),
        |world| propagate_inner(world, None, |_| {}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::go::scene::{Scene, SceneHandle};
    use xengine_math::{QuaternionF, Vector3F};

    /// Builds a chain root -> mid -> leaf via the Scene API and returns the
    /// entities in that order.
    fn build_chain(scene: &mut Scene, root_pos: Vector3F) -> [crate::Entity; 3] {
        let root = scene
            .create_go(Transform {
                position: root_pos,
                ..Transform::default()
            })
            .unwrap();
        let mid = scene.create_go(Transform::default()).unwrap();
        let leaf = scene.create_go(Transform::default()).unwrap();
        scene.set_parent(mid, Some(root)).unwrap();
        scene.set_parent(leaf, Some(mid)).unwrap();
        [root, mid, leaf]
    }

    #[test]
    fn root_rotation_moves_child() {
        let mut scene = SceneHandle::new();
        let [root, _, leaf] = build_chain(&mut scene, Vector3F::ZERO);
        // Give the leaf a local position (1,0,0).
        scene
            .set_transform_position(leaf, Vector3F::new(1.0, 0.0, 0.0))
            .unwrap();
        // Rotate the root by 90° about Z.
        let q =
            QuaternionF::from_axis_angle(Vector3F::new(0.0, 0.0, 1.0), std::f32::consts::FRAC_PI_2);
        scene.set_transform_rotation(root, q).unwrap();
        // Attach a GlobalTransform to root + leaf, then propagate.
        scene
            .world_mut()
            .add(
                root,
                GlobalTransform {
                    world: Matrix4F::IDENTITY,
                },
            )
            .unwrap();
        scene
            .world_mut()
            .add(
                leaf,
                GlobalTransform {
                    world: Matrix4F::IDENTITY,
                },
            )
            .unwrap();
        propagate(scene.world_mut());
        // The leaf's local (1,0,0) rotated by root's 90° about Z lands at (0,1,0).
        let w = scene
            .world()
            .get::<GlobalTransform>(leaf)
            .unwrap()
            .unwrap()
            .world;
        let p = w.transform_point(Vector3F::new(0.0, 0.0, 0.0));
        assert!(
            p.approx_eq(&Vector3F::new(0.0, 1.0, 0.0), 1e-4),
            "leaf world got {:?}",
            p
        );
    }

    #[test]
    fn dirty_subtree_cascades_and_resets() {
        let mut scene = SceneHandle::new();
        let [root, mid, leaf] = build_chain(&mut scene, Vector3F::ZERO);
        for e in [root, mid, leaf] {
            scene
                .world_mut()
                .add(
                    e,
                    GlobalTransform {
                        world: Matrix4F::IDENTITY,
                    },
                )
                .unwrap();
        }
        // Mark only root dirty via a position change (which also marks it).
        scene
            .set_transform_position(root, Vector3F::new(5.0, 0.0, 0.0))
            .unwrap();
        assert!(scene.world().contains::<TransformDirty>(root).unwrap());
        assert!(!scene.world().contains::<TransformDirty>(mid).unwrap());
        assert!(!scene.world().contains::<TransformDirty>(leaf).unwrap());
        propagate(scene.world_mut());
        // All subtree entities recomputed and reset.
        for e in [root, mid, leaf] {
            assert!(
                !scene.world().contains::<TransformDirty>(e).unwrap(),
                "dirty reset for {:?}",
                e
            );
        }
        // Root's world translation is (5,0,0); mid's is (5,0,0); leaf's too.
        for e in [root, mid, leaf] {
            let w = scene
                .world()
                .get::<GlobalTransform>(e)
                .unwrap()
                .unwrap()
                .world;
            let p = w.transform_point(Vector3F::ZERO);
            assert!(
                p.approx_eq(&Vector3F::new(5.0, 0.0, 0.0), 1e-4),
                "entity {:?} got {:?}",
                e.index(),
                p
            );
        }
    }

    #[test]
    fn leaf_change_does_not_touch_ancestors() {
        let mut scene = SceneHandle::new();
        let [root, mid, leaf] = build_chain(&mut scene, Vector3F::ZERO);
        for e in [root, mid, leaf] {
            scene
                .world_mut()
                .add(
                    e,
                    GlobalTransform {
                        world: Matrix4F::IDENTITY,
                    },
                )
                .unwrap();
        }
        // Set a sentinel on root/mid so an accidental recompute would overwrite it.
        sentinel_world(&mut scene, root, Vector3F::new(99.0, 0.0, 0.0));
        sentinel_world(&mut scene, mid, Vector3F::new(88.0, 0.0, 0.0));
        // Mark only the leaf dirty.
        scene
            .set_transform_position(leaf, Vector3F::new(0.0, 0.0, 0.0))
            .unwrap();
        assert!(scene.world().contains::<TransformDirty>(leaf).unwrap());
        assert!(!scene.world().contains::<TransformDirty>(root).unwrap());
        assert!(!scene.world().contains::<TransformDirty>(mid).unwrap());
        propagate(scene.world_mut());
        // Root/mid not recomputed: sentinel preserved.
        assert_eq!(
            scene
                .world()
                .get::<GlobalTransform>(root)
                .unwrap()
                .unwrap()
                .world
                .transform_point(Vector3F::ZERO),
            Vector3F::new(99.0, 0.0, 0.0)
        );
        assert_eq!(
            scene
                .world()
                .get::<GlobalTransform>(mid)
                .unwrap()
                .unwrap()
                .world
                .transform_point(Vector3F::ZERO),
            Vector3F::new(88.0, 0.0, 0.0)
        );
        // Leaf recomputed (dirty cleared).
        assert!(!scene.world().contains::<TransformDirty>(leaf).unwrap());
    }

    #[test]
    fn ancestor_independent_computation() {
        let mut scene = SceneHandle::new();
        let [root, mid, leaf] = build_chain(&mut scene, Vector3F::ZERO);
        for e in [root, mid, leaf] {
            scene
                .world_mut()
                .add(
                    e,
                    GlobalTransform {
                        world: Matrix4F::IDENTITY,
                    },
                )
                .unwrap();
        }
        // Move root by (1,0,0), mid by (0,2,0) — mark both dirty.
        scene
            .set_transform_position(root, Vector3F::new(1.0, 0.0, 0.0))
            .unwrap();
        scene
            .set_transform_position(mid, Vector3F::new(0.0, 2.0, 0.0))
            .unwrap();
        propagate(scene.world_mut());
        // leaf world = trs(leaf)·trs(mid)·trs(root). leaf local is zero, so
        // leaf origin = (1,0,0)+(0,2,0) applied up to world = (1,2,0).
        let w = scene
            .world()
            .get::<GlobalTransform>(leaf)
            .unwrap()
            .unwrap()
            .world;
        let p = w.transform_point(Vector3F::ZERO);
        assert!(
            p.approx_eq(&Vector3F::new(1.0, 2.0, 0.0), 1e-4),
            "leaf got {:?}",
            p
        );
    }

    #[test]
    fn unmarked_direct_write_is_not_propagated() {
        let mut scene = SceneHandle::new();
        let [_, _, leaf] = build_chain(&mut scene, Vector3F::ZERO);
        scene
            .world_mut()
            .add(
                leaf,
                GlobalTransform {
                    world: Matrix4F::IDENTITY,
                },
            )
            .unwrap();
        // Direct field write without the Scene set API (no dirty mark).
        if let Ok(Some(t)) = scene.world_mut().get_mut::<Transform>(leaf) {
            t.position = Vector3F::new(7.0, 0.0, 0.0);
        }
        propagate(scene.world_mut());
        // Not recomputed: GlobalTransform stays identity.
        let w = scene
            .world()
            .get::<GlobalTransform>(leaf)
            .unwrap()
            .unwrap()
            .world;
        assert!(w.approx_eq(&Matrix4F::IDENTITY, 1e-6));
    }

    #[test]
    fn no_global_transform_entity_is_skipped() {
        let mut scene = SceneHandle::new();
        let [root, _, leaf] = build_chain(&mut scene, Vector3F::ZERO);
        // root has GlobalTransform; mid/leaf do not.
        scene
            .world_mut()
            .add(
                root,
                GlobalTransform {
                    world: Matrix4F::IDENTITY,
                },
            )
            .unwrap();
        scene
            .set_transform_position(root, Vector3F::new(3.0, 0.0, 0.0))
            .unwrap();
        // Must not panic despite mid/leaf lacking GlobalTransform.
        propagate(scene.world_mut());
        let w = scene
            .world()
            .get::<GlobalTransform>(root)
            .unwrap()
            .unwrap()
            .world;
        let p = w.transform_point(Vector3F::ZERO);
        assert!(p.approx_eq(&Vector3F::new(3.0, 0.0, 0.0), 1e-4));
        assert!(!scene.world().contains::<GlobalTransform>(leaf).unwrap());
    }

    fn sentinel_world(scene: &mut Scene, e: crate::Entity, t: Vector3F) {
        if let Ok(Some(g)) = scene.world_mut().get_mut::<GlobalTransform>(e) {
            g.world = Matrix4F::from_translation(t);
        }
    }

    #[test]
    fn go_systems_coexist_in_postupdate_schedule() {
        use crate::go::hierarchy::hierarchy_maintain_system;
        use crate::schedule::Schedule;
        use crate::system::Stage;

        let mut scene = SceneHandle::new();
        let root = scene.create_go(Transform::default()).unwrap();
        let mid = scene.create_go(Transform::default()).unwrap();
        let _leaf = scene.create_go(Transform::default()).unwrap();
        scene.set_parent(mid, Some(root)).unwrap();
        // The two systems declare ordering (maintain before propagate), so the
        // schedule builds without an unordered conflict on Parent/Children.
        let schedule = Schedule::build(vec![
            hierarchy_maintain_system(),
            transform_propagate_system(Arc::new(JobSystem::single_threaded())),
        ])
        .expect("go systems must be ordered without conflict");
        let mut schedule = schedule;
        scene
            .set_transform_position(root, Vector3F::new(2.0, 0.0, 0.0))
            .unwrap();
        schedule.run_stage(scene.world_mut(), Stage::PostUpdate);
        assert!(!scene.world().contains::<TransformDirty>(root).unwrap());
    }

    // ── parallel phase 2a ──────────────────────────────────────────────────

    /// Root -> (child_a -> grandchild_a), child_b: a mix of depth and breadth
    /// with a `GlobalTransform` on every entity.
    fn build_propagation_tree(scene: &mut Scene) -> Vec<crate::Entity> {
        let mut entities = Vec::new();
        let root = scene
            .create_go(Transform {
                position: Vector3F::new(1.0, 2.0, 3.0),
                ..Transform::default()
            })
            .unwrap();
        entities.push(root);
        for index in 0..17 {
            let child = scene
                .create_go(Transform {
                    position: Vector3F::new(index as f32, 0.5, -1.0),
                    ..Transform::default()
                })
                .unwrap();
            scene.set_parent(child, Some(root)).unwrap();
            entities.push(child);
            if index % 4 == 0 {
                let grandchild = scene
                    .create_go(Transform {
                        position: Vector3F::new(0.0, 1.0, index as f32),
                        ..Transform::default()
                    })
                    .unwrap();
                scene.set_parent(grandchild, Some(child)).unwrap();
                entities.push(grandchild);
            }
        }
        for entity in &entities {
            scene
                .world_mut()
                .add(
                    *entity,
                    GlobalTransform {
                        world: Matrix4F::IDENTITY,
                    },
                )
                .unwrap();
        }
        entities
    }

    /// Marks every entity dirty so the next propagation recomputes all of them.
    fn mark_all_dirty(scene: &mut Scene, entities: &[crate::Entity]) {
        for entity in entities {
            scene.mark_transform_dirty(*entity).unwrap();
        }
    }

    fn world_origins(scene: &Scene, entities: &[crate::Entity]) -> Vec<Vector3F> {
        entities
            .iter()
            .map(|entity| {
                scene
                    .world()
                    .get::<GlobalTransform>(*entity)
                    .unwrap()
                    .unwrap()
                    .world
                    .transform_point(Vector3F::ZERO)
            })
            .collect()
    }

    #[test]
    fn parallel_phase_matches_serial_for_every_worker_count() {
        use crate::parallel::{JobConfig, JobSystem};

        let mut scene = SceneHandle::new();
        let entities = build_propagation_tree(&mut scene);

        mark_all_dirty(&mut scene, &entities);
        propagate(scene.world_mut());
        let serial = world_origins(&scene, &entities);

        for workers in [1.0f32, 2.0, 4.0] {
            mark_all_dirty(&mut scene, &entities);
            let jobs = JobSystem::new(JobConfig {
                hint: workers,
                reserve: 0,
                min: workers as u32,
                max: workers as u32,
            });
            propagate_inner(scene.world_mut(), Some(&jobs), |_| {});
            assert_eq!(
                world_origins(&scene, &entities),
                serial,
                "worker count {workers} must not change results"
            );
            for entity in &entities {
                assert!(
                    !scene.world().contains::<TransformDirty>(*entity).unwrap(),
                    "dirty flags are cleared"
                );
            }
        }
    }

    #[test]
    fn on_recompute_fires_once_per_entity_in_the_serial_order() {
        use crate::parallel::{JobConfig, JobSystem};

        let mut scene = SceneHandle::new();
        let entities = build_propagation_tree(&mut scene);

        mark_all_dirty(&mut scene, &entities);
        let mut serial_calls = Vec::new();
        propagate_inner(scene.world_mut(), None, |entity| serial_calls.push(entity));
        assert_eq!(serial_calls.len(), entities.len());

        mark_all_dirty(&mut scene, &entities);
        let jobs = JobSystem::new(JobConfig {
            hint: 4.0,
            reserve: 0,
            min: 4,
            max: 4,
        });
        let mut parallel_calls = Vec::new();
        propagate_inner(scene.world_mut(), Some(&jobs), |entity| {
            parallel_calls.push(entity)
        });
        assert_eq!(parallel_calls, serial_calls, "deterministic callback order");

        let mut unique = serial_calls.clone();
        unique.sort_by_key(|entity| (entity.index(), entity.generation()));
        unique.dedup();
        assert_eq!(unique.len(), serial_calls.len(), "exactly once per entity");
    }

    #[test]
    fn malformed_cycle_terminates_within_the_depth_limit() {
        use crate::go::Parent;

        let mut scene = SceneHandle::new();
        let first = scene.create_go(Transform::default()).unwrap();
        let second = scene.create_go(Transform::default()).unwrap();
        // Hand-built malformed cycle (bypasses the hierarchy API on purpose).
        if let Ok(Some(parent)) = scene.world_mut().get_mut::<Parent>(first) {
            *parent = Parent {
                parent: Some(second),
            };
        }
        if let Ok(Some(parent)) = scene.world_mut().get_mut::<Parent>(second) {
            *parent = Parent {
                parent: Some(first),
            };
        }
        scene
            .world_mut()
            .add(
                first,
                GlobalTransform {
                    world: Matrix4F::IDENTITY,
                },
            )
            .unwrap();
        mark_all_dirty(&mut scene, &[first]);

        propagate(scene.world_mut());
        assert!(
            !scene.world().contains::<TransformDirty>(first).unwrap(),
            "the walk must terminate and clear the flag"
        );
        assert_eq!(scene.world().entity_count(), 2);
    }
}
