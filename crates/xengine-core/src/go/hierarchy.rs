//! Hierarchy edge components (`Parent` / `Children`) and the maintenance
//! system.

use std::fmt;

use crate::entity::Entity;
use crate::system::{AccessKind, Stage, System};

use super::component::Component;

/// The child side of a hierarchy edge: which parent this game object is
/// attached to (if any).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Parent {
    /// `Some(parent)` when this entity has a live parent, else `None` (root).
    pub parent: Option<Entity>,
}

/// The parent side of a hierarchy edge: the children collection (maintained by
/// the hierarchy system / `Scene::set_parent`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Children {
    pub children: Vec<Entity>,
}

impl Component for Parent {}
impl Component for Children {}

/// Errors raised by hierarchy operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HierarchyError {
    /// The proposed parent would form an ancestor cycle (a → b → a).
    Cycle,
    /// An entity handle is stale / destroyed.
    StaleEntity,
}

impl fmt::Display for HierarchyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cycle => write!(f, "hierarchy cycle rejected"),
            Self::StaleEntity => write!(f, "stale entity handle"),
        }
    }
}

impl std::error::Error for HierarchyError {}

/// Returns `true` if `ancestor` lies on `node`'s ancestor chain (a cycle would
/// result from attaching `node` under `ancestor`). Falls back to `false` on a
/// malformed (already-cyclic) chain rather than looping forever.
pub(crate) fn would_cycle(world: &crate::World, ancestor: Entity, mut node: Entity) -> bool {
    let mut seen = std::collections::HashSet::new();
    loop {
        if node == ancestor {
            return true;
        }
        if !seen.insert(node.index()) {
            return false; // existing cycle in the data; do not loop forever
        }
        let parent = match world.get::<Parent>(node) {
            Ok(Some(p)) => p.parent,
            _ => None,
        };
        match parent {
            Some(p) => node = p,
            None => return false,
        }
    }
}

/// PostUpdate reconciliation: cleans orphaned children (a `Parent` pointing at
/// a destroyed entity) and dangling/`inconsistent` `Children` entries, and
/// restores missing back-edges so the hierarchy stays bidirectional.
///
/// This runs as a [`System`]; `Scene::set_parent` already maintains the edges
/// eagerly, so this pass only repairs paths that bypassed it (e.g. a direct
/// `World::destroy` or a raw `Parent` field write).
pub fn maintain(world: &mut crate::World) {
    let entities = world.entities(); // owned snapshot; the borrow ends here

    // Phase A: orphan cleanup — a child whose parent no longer lives is
    // detached to root.
    for &e in &entities {
        let parent = match world.get::<Parent>(e) {
            Ok(Some(p)) => p.parent,
            _ => None,
        };
        if let Some(par) = parent
            && !world.contains_entity(par)
            && let Ok(Some(p)) = world.get_mut::<Parent>(e)
        {
            p.parent = None;
        }
    }

    // Phase B: restore missing back-edges — a child pointing at a live parent
    // must appear in that parent's `Children`.
    let mut missing_back_edges: Vec<(Entity, Entity)> = Vec::new();
    for &e in &entities {
        let parent = match world.get::<Parent>(e) {
            Ok(Some(p)) => p.parent,
            _ => None,
        };
        if let Some(par) = parent
            && world.contains_entity(par)
        {
            let lacks = match world.get::<Children>(par) {
                Ok(Some(c)) => !c.children.contains(&e),
                Ok(None) => true,
                Err(_) => true,
            };
            if lacks {
                missing_back_edges.push((e, par));
            }
        }
    }
    for (child, parent) in missing_back_edges {
        if !world.contains::<Children>(parent).unwrap_or(false) {
            let _ = world.add(
                parent,
                Children {
                    children: vec![child],
                },
            );
        } else if let Ok(Some(c)) = world.get_mut::<Children>(parent)
            && !c.children.contains(&child)
        {
            c.children.push(child);
        }
    }

    // Phase C: dangling / inconsistent children — drop entries whose child is
    // destroyed or does not point back at this parent.
    for &e in &entities {
        let keep: Option<Vec<Entity>> = match world.get::<Children>(e) {
            Ok(Some(c)) => {
                let mut k = Vec::with_capacity(c.children.len());
                for &child in &c.children {
                    let child_alive = world.contains_entity(child);
                    let points_back =
                        matches!(world.get::<Parent>(child), Ok(Some(p)) if p.parent == Some(e));
                    if child_alive && points_back {
                        k.push(child);
                    }
                }
                Some(k)
            }
            _ => None,
        };
        if let Some(k) = keep
            && let Ok(Some(c)) = world.get_mut::<Children>(e)
        {
            c.children = k;
        }
    }
}

/// Builds the `hierarchy_maintain` post-update system, ordered before
/// `transform_propagate` so propagation sees a consistent hierarchy.
pub fn hierarchy_maintain_system() -> System {
    System::with_spec(
        "hierarchy_maintain",
        Stage::PostUpdate,
        &[
            ("Parent", AccessKind::Write),
            ("Children", AccessKind::Write),
        ],
        Some("transform_propagate"),
        None,
        maintain,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_detection_on_chain() {
        let mut w = crate::World::new();
        let a = w.create_empty();
        let b = w.create_empty();
        let c = w.create_empty();
        // Build a -> b -> c; attaching a under c would cycle.
        let _ = w.add(a, Parent { parent: None });
        let _ = w.add(b, Parent { parent: Some(a) });
        let _ = w.add(c, Parent { parent: Some(b) });
        // `would_cycle(ancestor, node)` = is `ancestor` on `node`'s chain?
        assert!(would_cycle(&w, a, c), "a is an ancestor of c");
        assert!(would_cycle(&w, a, b), "a is an ancestor of b");
        assert!(!would_cycle(&w, c, a), "c is not an ancestor of a");
        assert!(!would_cycle(&w, b, a), "b is not an ancestor of a");
    }

    #[test]
    fn cycle_self_is_detected() {
        let mut w = crate::World::new();
        let a = w.create_empty();
        let _ = w.add(a, Parent { parent: None });
        assert!(would_cycle(&w, a, a));
    }
}
