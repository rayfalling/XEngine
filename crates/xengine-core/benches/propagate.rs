//! 变换传播基准：串行 vs 并行（阶段 2a 并行计算 / 2b 串行写回）分段记录。
//!
//! 三种层级形态 × 两种规模：
//! - wide：1 根 + N 子（深度 1）
//! - deep：N/1000 条 1000 深链（祖先链遍历缓存不友好）
//! - flat：N 个根实体（无层级）
//!
//! 每次 tick 全部实体置脏后传播一次；`last_propagate_timing` 提供 scan /
//! compute（2a）/ apply（2b）分段耗时，wall clock 为传播调用本身（不含置脏）。
//!
//! 运行：`cargo bench -p xengine-core --bench propagate`

use std::time::{Duration, Instant};

use xengine_core::go::SceneHandle;
use xengine_core::go::global_transform::propagate;
use xengine_core::{
    Entity, GlobalTransform, JobConfig, JobSystem, Scene, Transform, last_propagate_timing,
    propagate_with_jobs,
};
use xengine_math::{Matrix4F, Vector3F};

const ENTITY_COUNTS: [usize; 2] = [10_000, 100_000];
const CHAIN_DEPTH: usize = 1_000;
const TICKS: u32 = 20;
const WARMUP: u32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Wide,
    Deep,
    Flat,
}

impl Shape {
    fn name(self) -> &'static str {
        match self {
            Self::Wide => "wide",
            Self::Deep => "deep",
            Self::Flat => "flat",
        }
    }
}

fn build_scene(shape: Shape, n: usize) -> (SceneHandle, Vec<Entity>) {
    let mut scene = SceneHandle::new();
    let mut entities = Vec::with_capacity(n);
    match shape {
        Shape::Wide => {
            let root = scene.create_go(Transform::default()).unwrap();
            entities.push(root);
            for i in 1..n {
                let child = scene
                    .create_go(Transform {
                        position: Vector3F::new(i as f32, 0.0, 0.0),
                        ..Transform::default()
                    })
                    .unwrap();
                scene.set_parent(child, Some(root)).unwrap();
                entities.push(child);
            }
        }
        Shape::Deep => {
            let chains = n.div_ceil(CHAIN_DEPTH).max(1);
            for _ in 0..chains {
                let mut parent = scene.create_go(Transform::default()).unwrap();
                entities.push(parent);
                for level in 1..CHAIN_DEPTH {
                    if entities.len() >= n {
                        break;
                    }
                    let node = scene
                        .create_go(Transform {
                            position: Vector3F::new(0.0, 1.0, level as f32 * 0.001),
                            ..Transform::default()
                        })
                        .unwrap();
                    scene.set_parent(node, Some(parent)).unwrap();
                    entities.push(node);
                    parent = node;
                }
            }
        }
        Shape::Flat => {
            for i in 0..n {
                let root = scene
                    .create_go(Transform {
                        position: Vector3F::new(i as f32, 1.0, 0.5),
                        ..Transform::default()
                    })
                    .unwrap();
                entities.push(root);
            }
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
    (scene, entities)
}

fn mark_all_dirty(scene: &mut Scene, entities: &[Entity]) {
    for entity in entities {
        scene.mark_transform_dirty(*entity).unwrap();
    }
}

fn propagate_with(scene: &mut Scene, jobs: Option<&JobSystem>) {
    match jobs {
        Some(jobs) => propagate_with_jobs(scene.world_mut(), jobs),
        None => propagate(scene.world_mut()),
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Sample {
    total: Duration,
    scan: Duration,
    compute: Duration,
    apply: Duration,
}

fn measure(scene: &mut Scene, entities: &[Entity], jobs: Option<&JobSystem>) -> Sample {
    for _ in 0..WARMUP {
        mark_all_dirty(scene, entities);
        propagate_with(scene, jobs);
    }
    let mut sample = Sample::default();
    for _ in 0..TICKS {
        mark_all_dirty(scene, entities);
        let start = Instant::now();
        propagate_with(scene, jobs);
        sample.total += start.elapsed();
        if let Some(timing) = last_propagate_timing() {
            assert_eq!(timing.entities, entities.len(), "all entities recomputed");
            sample.scan += timing.scan;
            sample.compute += timing.compute;
            sample.apply += timing.apply;
        }
    }
    let ticks = TICKS;
    Sample {
        total: sample.total / ticks,
        scan: sample.scan / ticks,
        compute: sample.compute / ticks,
        apply: sample.apply / ticks,
    }
}

/// 串行与并行结果必须逐元素一致（基准同时也是回归哨兵）。
fn verify_equivalence(shape: Shape, n: usize) {
    let (mut scene, entities) = build_scene(shape, n);
    mark_all_dirty(&mut scene, &entities);
    propagate_with(&mut scene, None);
    let serial: Vec<Vector3F> = entities
        .iter()
        .map(|e| {
            scene
                .world()
                .get::<GlobalTransform>(*e)
                .unwrap()
                .unwrap()
                .world
                .transform_point(Vector3F::ZERO)
        })
        .collect();

    let jobs = parallel_jobs();
    mark_all_dirty(&mut scene, &entities);
    propagate_with(&mut scene, Some(&jobs));
    for (entity, expected) in entities.iter().zip(&serial) {
        let actual = scene
            .world()
            .get::<GlobalTransform>(*entity)
            .unwrap()
            .unwrap()
            .world
            .transform_point(Vector3F::ZERO);
        assert_eq!(actual, *expected, "parallel result must match serial");
    }
}

fn parallel_jobs() -> JobSystem {
    // 默认用满硬件线程（reserve=0）；`XENGINE_BENCH_WORKERS=n` 可固定 worker 数。
    match std::env::var("XENGINE_BENCH_WORKERS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
    {
        Some(workers) => JobSystem::new(JobConfig {
            hint: workers.max(1) as f32,
            reserve: 0,
            min: workers.max(1),
            max: workers.max(1),
        }),
        None => JobSystem::new(JobConfig {
            hint: 1.0,
            reserve: 0,
            min: 1,
            max: 64,
        }),
    }
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1e3
}

fn main() {
    let jobs = parallel_jobs();
    println!(
        "变换传播基准（workers={}, ticks={}, warmup={}）\n",
        jobs.worker_count(),
        TICKS,
        WARMUP
    );
    println!(
        "{:<6} {:>7} | {:>9} {:>8} {:>8} {:>8} | {:>9} {:>8} {:>8} {:>8} | {:>8}",
        "shape", "N", "ser ms", "scan", "2a", "2b", "par ms", "scan", "2a", "2b", "speedup"
    );

    let only = std::env::var("XENGINE_BENCH_SHAPES").ok();
    let shapes: Vec<Shape> = [Shape::Wide, Shape::Deep, Shape::Flat]
        .into_iter()
        .filter(|shape| {
            only.as_deref()
                .map(|filter| filter.split(',').any(|item| item.trim() == shape.name()))
                .unwrap_or(true)
        })
        .collect();

    let reverse = std::env::var("XENGINE_BENCH_ORDER")
        .map(|value| value == "ps")
        .unwrap_or(false);

    for shape in shapes {
        for n in ENTITY_COUNTS {
            // Each mode gets a freshly built scene: archetype rows get permuted
            // by the dirty-marker migrations, so reusing one scene would compare
            // two different memory layouts instead of two execution paths.
            // `XENGINE_BENCH_ORDER=ps` flips which mode runs first, which
            // separates real path differences from measurement-order effects.
            let (serial, parallel) = if reverse {
                let (mut parallel_scene, parallel_entities) = build_scene(shape, n);
                let parallel = measure(&mut parallel_scene, &parallel_entities, Some(&jobs));
                let (mut serial_scene, serial_entities) = build_scene(shape, n);
                let serial = measure(&mut serial_scene, &serial_entities, None);
                (serial, parallel)
            } else {
                let (mut serial_scene, serial_entities) = build_scene(shape, n);
                let serial = measure(&mut serial_scene, &serial_entities, None);
                let (mut parallel_scene, parallel_entities) = build_scene(shape, n);
                let parallel = measure(&mut parallel_scene, &parallel_entities, Some(&jobs));
                (serial, parallel)
            };
            let speedup = serial.total.as_secs_f64() / parallel.total.as_secs_f64().max(1e-9);
            println!(
                "{:<6} {:>7} | {:>9.3} {:>8.3} {:>8.3} {:>8.3} | {:>9.3} {:>8.3} {:>8.3} {:>8.3} | {:>7.2}x",
                shape.name(),
                n,
                ms(serial.total),
                ms(serial.scan),
                ms(serial.compute),
                ms(serial.apply),
                ms(parallel.total),
                ms(parallel.scan),
                ms(parallel.compute),
                ms(parallel.apply),
                speedup,
            );
        }
    }

    println!("\n串行/并行一致性校验（逐元素精确比较）：");
    for shape in [Shape::Wide, Shape::Deep, Shape::Flat] {
        verify_equivalence(shape, 4_096);
        println!("  {:<6} ok", shape.name());
    }
}
