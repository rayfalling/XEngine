//! Derived world transform cache + dirty-driven two-phase propagation.

use std::collections::HashSet;

use xengine_math::Matrix4F;

use crate::World;
use crate::entity::Entity;
use crate::system::{AccessKind, Stage, System};

use super::component::Component;
use super::hierarchy::Children;
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

/// Recomputes the world transform for `entity` by independently walking its own
/// ancestor chain: `world(e) = trs(e) · trs(parent) · … · trs(root)` under the
/// crate row-vector `mul` convention (the rightmost factor is applied first, so
/// an e-local point is pushed up through each ancestor's local transform).
///
/// The computation is order-independent — it never reads an ancestor's
/// `GlobalTransform`, only each ancestor's local `Transform`.
fn compute_world(world: &World, entity: Entity) -> Matrix4F {
    // Build [e, parent, ..., root].
    let mut chain: Vec<Entity> = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    let mut node = entity;
    loop {
        if !seen.insert(node.index()) {
            break; // malformed cycle: stop to avoid looping forever
        }
        chain.push(node);
        let parent = match world.get::<super::hierarchy::Parent>(node) {
            Ok(Some(p)) => p.parent,
            _ => None,
        };
        match parent {
            Some(p) => node = p,
            None => break,
        }
    }
    let mut acc = local_trs(world, chain[0]);
    for &ancestor in &chain[1..] {
        acc = acc.mul(&local_trs(world, ancestor));
    }
    acc
}

fn local_trs(world: &World, entity: Entity) -> Matrix4F {
    match world.get::<Transform>(entity) {
        Ok(Some(t)) => Matrix4F::from_trs(t.position, &t.rotate, t.scale),
        _ => Matrix4F::IDENTITY,
    }
}

/// Number of entities processed per `recompute_chunk` call (the documented
/// parallel insertion point: a chunk is the unit the scheduler can hand to a
/// worker; every chunk writes disjoint entity rows so it is data-race safe).
pub const PROPAGATE_CHUNK: usize = 64;

/// Recomputes one chunk of to-recompute entities (serial in the first
/// implementation; a per-chunk worker is the documented parallel insertion
/// point).
fn recompute_chunk(world: &mut World, chunk: &[Entity], on_recompute: &mut impl FnMut(Entity)) {
    for &e in chunk {
        let world_mat = compute_world(world, e);
        let has_gt = world.contains::<GlobalTransform>(e).unwrap_or(false);
        if has_gt && let Ok(Some(gt)) = world.get_mut::<GlobalTransform>(e) {
            gt.world = world_mat;
        }
        if world.contains::<TransformDirty>(e).unwrap_or(false) {
            let _ = world.remove::<TransformDirty>(e);
        }
        on_recompute(e);
    }
}

/// Two-phase propagation.
///
/// **Phase 1 (sequential, reads hierarchy edges):** from every dirty entity,
/// walk the `Children` subtree and mark every descendant *to recompute* (parent
/// movement must reach the whole subtree, even if a child's local transform did
/// not change).
///
/// **Phase 2 (per-entity parallel insertion point; serial here):** each marked
/// entity recomputes independently from its own ancestor chain and writes its
/// own `GlobalTransform`, then clears its own `TransformDirty`. Chunking the
/// marked set is the documented scheduler hook (`PROPAGATE_CHUNK`).
pub fn propagate(world: &mut World) {
    propagate_inner(world, |_| {});
}

/// Propagate with an optional `on_recompute` callback (test/observability
/// hook, invoked once per recomputed entity).
pub(crate) fn propagate_inner(world: &mut World, mut on_recompute: impl FnMut(Entity)) {
    // Phase 1: gather initially-dirty roots and mark all descendants.
    let mut roots: Vec<Entity> = Vec::new();
    world.iterate::<TransformDirty>(|e, _| roots.push(e));
    let mut visited: HashSet<u32> = roots.iter().map(|e| e.index()).collect();
    let mut stack: Vec<Entity> = roots.clone();
    while let Some(e) = stack.pop() {
        let children = match world.get::<Children>(e) {
            Ok(Some(c)) => c.children.clone(),
            _ => Vec::new(),
        };
        for child in children {
            if visited.insert(child.index()) {
                if !world.contains::<TransformDirty>(child).unwrap_or(false) {
                    let _ = world.add(child, TransformDirty);
                }
                stack.push(child);
            }
        }
    }

    // Phase 2: recompute every marked entity, chunked (serial first version).
    let mut to_recompute: Vec<Entity> = Vec::new();
    world.iterate::<TransformDirty>(|e, _| to_recompute.push(e));
    for chunk in to_recompute.chunks(PROPAGATE_CHUNK) {
        recompute_chunk(world, chunk, &mut on_recompute);
    }
}

/// Builds the `transform_propagate` post-update system, ordered after
/// `hierarchy_maintain` so it sees a consistent hierarchy.
pub fn transform_propagate_system() -> System {
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
        propagate,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::go::scene::Scene;
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
        let mut scene = Scene::new();
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
        let mut scene = Scene::new();
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
        let mut scene = Scene::new();
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
        let mut scene = Scene::new();
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
        let mut scene = Scene::new();
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
        let mut scene = Scene::new();
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

        let mut scene = Scene::new();
        let root = scene.create_go(Transform::default()).unwrap();
        let mid = scene.create_go(Transform::default()).unwrap();
        let _leaf = scene.create_go(Transform::default()).unwrap();
        scene.set_parent(mid, Some(root)).unwrap();
        // The two systems declare ordering (maintain before propagate), so the
        // schedule builds without an unordered conflict on Parent/Children.
        let schedule = Schedule::build(vec![
            hierarchy_maintain_system(),
            transform_propagate_system(),
        ])
        .expect("go systems must be ordered without conflict");
        let mut schedule = schedule;
        scene
            .set_transform_position(root, Vector3F::new(2.0, 0.0, 0.0))
            .unwrap();
        schedule.run_stage(scene.world_mut(), Stage::PostUpdate);
        assert!(!scene.world().contains::<TransformDirty>(root).unwrap());
    }
}
