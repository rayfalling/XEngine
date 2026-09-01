//! Function-style systems with declared access metadata.

use crate::world::World;

/// The phase a system registers to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Stage {
    /// Fixed-step update (0..N per rendered frame).
    FixedUpdate,
    /// Standard per-rendered-frame update.
    Update,
    /// Standard per-rendered-frame late update.
    PostUpdate,
}

/// Access kind for a component or resource name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessKind {
    Read,
    Write,
}

/// A function-style system: a closure plus scheduling metadata.
///
/// Access metadata drives the conflict detection in [`crate::schedule::Schedule`];
/// systems declare the component/resource names they touch.
pub struct System {
    name: &'static str,
    stage: Stage,
    access: Vec<(&'static str, AccessKind)>,
    before: Option<&'static str>,
    after: Option<&'static str>,
    run: Box<dyn FnMut(&mut World)>,
}

impl System {
    /// Creates a system with no declared access.
    pub fn new(name: &'static str, stage: Stage, run: impl FnMut(&mut World) + 'static) -> Self {
        Self {
            name,
            stage,
            access: Vec::new(),
            before: None,
            after: None,
            run: Box::new(run),
        }
    }

    /// Creates a system with access metadata and optional explicit ordering.
    #[allow(clippy::too_many_arguments)]
    pub fn with_spec(
        name: &'static str,
        stage: Stage,
        access: &[(&'static str, AccessKind)],
        before: Option<&'static str>,
        after: Option<&'static str>,
        run: impl FnMut(&mut World) + 'static,
    ) -> Self {
        Self {
            name,
            stage,
            access: access.to_vec(),
            before,
            after,
            run: Box::new(run),
        }
    }

    pub fn name(&self) -> &'static str {
        self.name
    }

    pub fn stage(&self) -> Stage {
        self.stage
    }

    pub fn access(&self) -> &[(&'static str, AccessKind)] {
        &self.access
    }

    pub fn before(&self) -> Option<&'static str> {
        self.before
    }

    pub fn after(&self) -> Option<&'static str> {
        self.after
    }

    /// Run this system against the world.
    pub fn run(&mut self, world: &mut World) {
        (self.run)(world);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_metadata_is_exposed() {
        let s = System::with_spec(
            "sys",
            Stage::Update,
            &[("Health", AccessKind::Write)],
            Some("other"),
            Some("base"),
            |_| {},
        );
        assert_eq!(s.name(), "sys");
        assert_eq!(s.stage(), Stage::Update);
        assert_eq!(s.access(), &[("Health", AccessKind::Write)]);
        assert_eq!(s.before(), Some("other"));
        assert_eq!(s.after(), Some("base"));
    }
}
