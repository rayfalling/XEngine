use xengine_core::{AccessKind, Engine, FrameMode, Schedule, Stage, System, World};

fn main() {
    println!("Hello, world! ({})", xengine_core::engine_name());
    demo_engine();
}

/// Minimal end-to-end engine demo: one movement system over a few entities.
fn demo_engine() {
    #[derive(Debug, Clone, Copy)]
    struct Position(f32);
    #[derive(Debug, Clone, Copy)]
    struct Velocity(f32);

    let mut world = World::new();
    // Auto-registration path + explicit scriptable registration is covered
    // by the core test suite; here we exercise the basic loop.
    for i in 0..10 {
        world.create2(Position(i as f32), Velocity(1.0)).unwrap();
    }
    let systems = vec![
        System::with_spec(
            "movement",
            Stage::Update,
            &[("Position", AccessKind::Write), ("Velocity", AccessKind::Read)],
            None,
            None,
            move |w| {
                w.query2::<Position, Velocity>(|_e, pos, vel| {
                    pos.0 += vel.0;
                });
            },
        ),
    ];
    let schedule = Schedule::build(systems).unwrap();
    let mut engine = Engine::new(
        world,
        schedule,
        FrameMode::Capped { target_fps: 60 },
    );
    engine.tick(core::time::Duration::from_millis(16));
    println!("entities moved: {}", engine.world().entity_count());
}
