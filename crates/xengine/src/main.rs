use std::sync::Arc;
use std::time::Duration;

use xengine_core::go::global_transform::transform_propagate_system;
use xengine_core::go::hierarchy::hierarchy_maintain_system;
use xengine_core::{
    AccessKind, Engine, FrameMode, GlobalTransform, JobConfig, JobSystem, SceneHandle, Schedule,
    Stage, System, Transform, TransformDirty, last_propagate_timing,
};
use xengine_math::{Matrix4F, Vector3F};

fn main() {
    println!("Hello, world! ({})", xengine_core::engine_name());
    demo_engine();
}

/// End-to-end demo: a GO hierarchy, dirty-driven transform propagation running
/// phase 2a on the job system, and the engine's frame barrier.
fn demo_engine() {
    let jobs = Arc::new(JobSystem::new(JobConfig::default()));
    println!("job workers: {}", jobs.worker_count());

    let mut scene = SceneHandle::new();
    let root = scene.create_go(Transform::default()).unwrap();
    let mut leaves = Vec::new();
    for i in 0..16 {
        let leaf = scene
            .create_go(Transform {
                position: Vector3F::new(i as f32, 0.0, 0.0),
                ..Transform::default()
            })
            .unwrap();
        scene.set_parent(leaf, Some(root)).unwrap();
        scene
            .world_mut()
            .add(
                leaf,
                GlobalTransform {
                    world: Matrix4F::IDENTITY,
                },
            )
            .unwrap();
        leaves.push(leaf);
    }
    scene
        .world_mut()
        .add(
            root,
            GlobalTransform {
                world: Matrix4F::IDENTITY,
            },
        )
        .unwrap();

    // Update stage: move the root and mark it dirty (direct field writes need an
    // explicit mark, as documented on `Transform`).
    let mover = System::with_spec(
        "move_root",
        Stage::Update,
        &[
            ("Transform", AccessKind::Write),
            ("TransformDirty", AccessKind::Write),
        ],
        None,
        None,
        move |world| {
            if let Ok(Some(transform)) = world.get_mut::<Transform>(root) {
                transform.position = Vector3F::new(0.0, 1.0, 0.0);
            }
            let _ = world.add(root, TransformDirty);
        },
    );

    // PostUpdate: hierarchy maintenance, then parallel propagation.
    let schedule = Schedule::build(vec![
        mover,
        hierarchy_maintain_system(),
        transform_propagate_system(Arc::clone(&jobs)),
    ])
    .expect("systems are ordered without conflicts");

    let mut engine = Engine::new(scene, schedule, FrameMode::Capped { target_fps: 60 })
        .with_jobs(Arc::clone(&jobs));
    engine.tick(Duration::from_millis(16));

    let last = *leaves.last().expect("at least one leaf");
    let world = engine.scene().world();
    if let Ok(Some(global)) = world.get::<GlobalTransform>(last) {
        let origin = global.world.transform_point(Vector3F::ZERO);
        println!(
            "leaf world origin: ({:.2}, {:.2}, {:.2})",
            origin.x, origin.y, origin.z
        );
    }
    if let Some(timing) = last_propagate_timing() {
        println!(
            "propagate: entities={} scan={:?} 2a={:?} 2b={:?} parallel={}",
            timing.entities, timing.scan, timing.compute, timing.apply, timing.parallel
        );
    }
    println!("entities: {}", world.entity_count());
}
