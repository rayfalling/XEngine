//! Schedule: stable topological ordering with conflict detection.

use std::fmt;

use crate::system::{AccessKind, Stage, System};
use crate::world::World;

/// Errors raised while building a schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// Two systems access the same component/resource with a write involved
    /// and no explicit ordering relation exists.
    UnorderedConflict {
        stage: Stage,
        a: &'static str,
        b: &'static str,
        key: &'static str,
    },
    /// Explicit before/after relations contain a cycle.
    Cycle { stage: Stage },
}

impl fmt::Display for ScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnorderedConflict { stage, a, b, key } => write!(
                f,
                "conflict on '{key}' between systems '{a}' and '{b}' in {stage:?}: add an explicit before/after relation"
            ),
            Self::Cycle { stage } => write!(f, "cycle detected in explicit ordering of {stage:?}"),
        }
    }
}

impl std::error::Error for ScheduleError {}

/// A compiled schedule: per-stage stable topological orders.
pub struct Schedule {
    systems: Vec<System>,
    orders: Vec<(Stage, Vec<usize>)>,
}

impl Schedule {
    /// Builds a schedule from a set of systems.
    ///
    /// For each stage the order is a stable topological sort:
    /// explicit `before`/`after` edges are honored; conflicts (same key with
    /// a write on either side) force an explicit relation; unconstrained
    /// systems keep registration order (deterministic).
    pub fn build(systems: Vec<System>) -> Result<Self, ScheduleError> {
        let stages = [Stage::FixedUpdate, Stage::Update, Stage::PostUpdate];
        let mut orders = Vec::new();
        for stage in stages {
            let idxs: Vec<usize> = systems
                .iter()
                .enumerate()
                .filter(|(_, s)| s.stage() == stage)
                .map(|(i, _)| i)
                .collect();
            let order = topo_stage(&systems, &idxs, stage)?;
            orders.push((stage, order));
        }
        Ok(Self { systems, orders })
    }

    /// Runs all systems of one stage in compiled order, flushing the command
    /// queue at every system boundary.
    pub fn run_stage(&mut self, world: &mut World, stage: Stage) {
        if let Some((_, order)) = self.orders.iter().find(|(s, _)| *s == stage) {
            for &i in order {
                self.systems[i].run(world);
                world.flush_commands();
            }
        }
    }

    /// Number of systems in the schedule.
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }
}

/// Checks whether two systems conflict on a shared access key.
fn conflicts(a: &System, b: &System) -> Option<&'static str> {
    for (an, ak) in a.access() {
        for (bn, bk) in b.access() {
            if an == bn && (*ak == AccessKind::Write || *bk == AccessKind::Write) {
                return Some(an);
            }
        }
    }
    None
}

/// Direction of the explicit relation between two systems: a->b (true) or
/// b->a (false), or None when unordered.
fn relation(a: &System, b: &System) -> Option<bool> {
    let a_before_b = a.before() == Some(b.name()) || b.after() == Some(a.name());
    let b_before_a = b.before() == Some(a.name()) || a.after() == Some(b.name());
    match (a_before_b, b_before_a) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

/// Stable Kahn topological sort for one stage.
fn topo_stage(
    systems: &[System],
    idxs: &[usize],
    stage: Stage,
) -> Result<Vec<usize>, ScheduleError> {
    // Conflict + relation edges.
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for (pos_a, &a) in idxs.iter().enumerate() {
        for &b in &idxs[pos_a + 1..] {
            let rel = relation(&systems[a], &systems[b]);
            if let Some(a_first) = rel {
                if a_first {
                    edges.push((a, b));
                } else {
                    edges.push((b, a));
                }
            } else if conflicts(&systems[a], &systems[b]).is_some() {
                let key = conflicts(&systems[a], &systems[b]).unwrap();
                return Err(ScheduleError::UnorderedConflict {
                    stage,
                    a: systems[a].name(),
                    b: systems[b].name(),
                    key,
                });
            }
        }
    }

    // Indegrees.
    let mut indegree: std::collections::HashMap<usize, usize> =
        idxs.iter().map(|&i| (i, 0)).collect();
    for (_, to) in &edges {
        *indegree.get_mut(to).unwrap() += 1;
    }
    let mut order: Vec<usize> = Vec::with_capacity(idxs.len());
    let mut remaining: Vec<usize> = idxs.to_vec();
    while !remaining.is_empty() {
        // Stable pick: the smallest registration index with zero indegree.
        let pick = remaining
            .iter()
            .copied()
            .filter(|&n| indegree.get(&n).copied().unwrap_or(0) == 0)
            .min()
            .ok_or(ScheduleError::Cycle { stage })?;
        remaining.retain(|&n| n != pick);
        order.push(pick);
        for (from, to) in &edges {
            if *from == pick {
                *indegree.get_mut(to).unwrap() -= 1;
            }
        }
    }
    Ok(order)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn sys(name: &'static str, stage: Stage, access: &[(&'static str, AccessKind)]) -> System {
        System::with_spec(name, stage, access, None, None, |_| {})
    }

    #[test]
    fn conflict_without_order_is_rejected() {
        let a = sys("reader", Stage::Update, &[("T", AccessKind::Read)]);
        let b = sys("writer", Stage::Update, &[("T", AccessKind::Write)]);
        let err = match Schedule::build(vec![a, b]) {
            Err(e) => e,
            Ok(_) => panic!("expected unordered conflict"),
        };
        assert!(matches!(
            err,
            ScheduleError::UnorderedConflict {
                a: "reader",
                b: "writer",
                ..
            }
        ));
    }

    #[test]
    fn explicit_order_resolves_conflict() {
        let a = System::with_spec(
            "reader",
            Stage::Update,
            &[("T", AccessKind::Read)],
            Some("writer"),
            None,
            |_| {},
        );
        let b = System::with_spec(
            "writer",
            Stage::Update,
            &[("T", AccessKind::Write)],
            None,
            Some("reader"),
            |_| {},
        );
        let schedule = Schedule::build(vec![a, b]).unwrap();
        assert_eq!(schedule.system_count(), 2);
    }

    #[test]
    fn unordered_non_conflicting_keeps_registration_order() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let log = Rc::new(RefCell::new(Vec::new()));
        let mk = |name: &'static str, key: &'static str| {
            let l = log.clone();
            System::with_spec(
                name,
                Stage::Update,
                &[(key, AccessKind::Write)],
                None,
                None,
                move |_| {
                    l.borrow_mut().push(name);
                },
            )
        };
        let systems = vec![mk("x", "A"), mk("y", "B"), mk("z", "C")];
        let schedule = Schedule::build(systems).unwrap();
        let mut schedule = schedule;
        schedule.run_stage(&mut World::new(), Stage::Update);
        assert_eq!(*log.borrow(), vec!["x", "y", "z"]);
    }

    #[test]
    fn run_stage_invokes_in_topological_order() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let log = Rc::new(RefCell::new(Vec::new()));
        let mk = |name: &'static str,
                  key: &'static str,
                  before: Option<&'static str>,
                  after: Option<&'static str>| {
            let l = log.clone();
            System::with_spec(
                name,
                Stage::Update,
                &[(key, AccessKind::Write)],
                before,
                after,
                move |_| {
                    l.borrow_mut().push(name);
                },
            )
        };
        // one runs before two (explicit), both write T.
        let systems = vec![
            mk("one", "T", Some("two"), None),
            mk("two", "T", None, Some("one")),
        ];
        let mut schedule = Schedule::build(systems).unwrap();
        schedule.run_stage(&mut World::new(), Stage::Update);
        assert_eq!(*log.borrow(), vec!["one", "two"]);
        schedule.run_stage(&mut World::new(), Stage::Update);
        assert_eq!(*log.borrow(), vec!["one", "two", "one", "two"]);
    }
}
