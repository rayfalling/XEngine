//! Placeholder baseline benchmark for the `xengine-math` hot operations.
//!
//! `harness = false` (no external bench framework, matching the existing
//! `crates/xengine-core/benches/ecs.rs`); measures wall time for the O(1)
//! constant-cost operations as a baseline. A stable timing harness lands in a
//! later change (criterion or equivalent).

use std::hint::black_box;
use std::time::Instant;
use xengine_math::*;

const N: u32 = 100_000;

fn main() {
    let a = Matrix4F::from_trs(
        Vector3F::new(1.0, 2.0, 3.0),
        &QuaternionF::from_euler_yxz(0.3, 0.4, 0.5),
        Vector3F::new(2.0, 3.0, 4.0),
    );
    let b = Matrix4F::from_trs(
        Vector3F::new(-1.0, 0.0, 5.0),
        &QuaternionF::from_euler_yxz(0.1, 0.6, 0.2),
        Vector3F::new(1.0, 1.0, 1.0),
    );
    let p = Vector3F::new(1.0, 2.0, 3.0);
    let q = QuaternionF::from_euler_yxz(0.3, 0.4, 0.5);

    // Matrix-multiply baseline.
    let start = Instant::now();
    let mut acc = Matrix4F::IDENTITY;
    for _ in 0..N {
        acc = black_box(a.mul(&b));
    }
    let el = start.elapsed();
    println!(
        "mat4_mul({N}): {:?} ({:.2} ns/op)",
        el,
        el.as_nanos() as f64 / N as f64
    );
    black_box(acc);

    // Point transform baseline.
    let start = Instant::now();
    let mut sum = Vector3F::ZERO;
    for _ in 0..N {
        sum = black_box(a.transform_point(p));
    }
    let el = start.elapsed();
    println!(
        "transform_point({N}): {:?} ({:.2} ns/op)",
        el,
        el.as_nanos() as f64 / N as f64
    );
    black_box(sum);

    // Quaternion rotate baseline.
    let start = Instant::now();
    let mut rot = Vector3F::ZERO;
    for _ in 0..N {
        rot = black_box(q.rotate_vec3(p));
    }
    let el = start.elapsed();
    println!(
        "quat_rotate({N}): {:?} ({:.2} ns/op)",
        el,
        el.as_nanos() as f64 / N as f64
    );
    black_box(rot);
}
