//! Placeholder benchmark: 100k entity create + single iteration.
//!
//! Harness=false (no external bench framework); measures wall time for the
//! O(1) amortized lifecycle contract. A stable baseline harness lands in a
//! later change (criterion or equivalent).

use std::time::Instant;
use xengine_core::World;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct Position(f32, f32);

const N: u32 = 100_000;

fn main() {
    // create benchmark
    let start = Instant::now();
    let mut world = World::new();
    for i in 0..N {
        world.create1(Position(i as f32, 0.0)).unwrap();
    }
    let create_elapsed = start.elapsed();
    println!(
        "create({N}): {:?} ({:.2} ns/entity)",
        create_elapsed,
        create_elapsed.as_nanos() as f64 / N as f64
    );

    // single-component iteration benchmark
    let start = Instant::now();
    let mut count = 0u64;
    world.iterate::<Position>(|_e, _p| count += 1);
    let iter_elapsed = start.elapsed();
    println!(
        "iterate({N}): {:?} ({:.2} ns/entity)",
        iter_elapsed,
        iter_elapsed.as_nanos() as f64 / N as f64
    );
    assert_eq!(count, N as u64);

    // join iteration benchmark (2 components)
    #[derive(Debug, Clone, Copy)]
    #[allow(dead_code)]
    struct Velocity(f32, f32);
    let mut world2 = World::new();
    for i in 0..N {
        world2
            .create2(Position(i as f32, 0.0), Velocity(1.0, 2.0))
            .unwrap();
    }
    let start = Instant::now();
    let mut sum = 0u64;
    world2
        .query2::<Position, Velocity>(|_e, _p, _v| sum += 1)
        .unwrap();
    let join_elapsed = start.elapsed();
    println!(
        "query2({N}): {:?} ({:.2} ns/entity)",
        join_elapsed,
        join_elapsed.as_nanos() as f64 / N as f64
    );
    assert_eq!(sum, N as u64);
}
