//! Spike benchmark: 对比「GO = Entity 别名 + 三组件套餐（Transform / WorldRef /
//! Parent+Children）+ 脚本包装层导出」与「包装句柄直接持有数据位置」的访问性能。
//!
//! 这是规划期的验证代码（非正式 API）：GO 组件以本地结构体模拟，用于回答
//! 「脚本层每帧访问 GO 组件」的四种工程形态成本差，不做架构承诺。
//!
//! 本变更新增 **GoHandle 档**（模式 F）：使用正式 go 层
//! `Scene::go_handle` / `Scene::go_view`（位置缓存 + 世代校验，无裸指针），
//! 与既有 A / C / E 档对照记录稳定访问成本。
//!
//! 各档位：
//! - lookup_per_access: 别名方案最坏路径，每次访问都以 Entity 做完整 ECS 查询。
//! - resolve_per_frame: 脚本层每帧导出 GO 视图，每实体每帧解析一次组件引用，
//!   当帧内多次直访（无持久缓存）。
//! - persistent_ptr: 包装句柄方案，句柄持久缓存组件裸指针，跨帧直接访问；
//!   在组件集稳定期间零查找（结构变更后缓存失效需重建）。
//! - archetype_iter: 别名方案 + Rust 系统内部迭代，ECS 连续列遍历，脚本
//!   与系统共享数据时的最优路径。
//! - persistent_ptr_mig: persistent_ptr 每 tick 有 1% 实体发生组件增删（archetype
//!   迁移），句柄缓存全量失效并重建，检验结构变化期的真实成本。
//! - go_handle: 正式 GoHandle（位置缓存 + 世代校验）稳定访问路径。

use std::hint::black_box;
use std::time::Instant;

use xengine_core::go::{Scene, Transform as GoTransform};
use xengine_core::{Entity, SceneHandle, World};
use xengine_math::Vector3F;

// ── GO 三组件套餐（模拟形态，占用与真实接近）─────────────────────────────

/// 变换：位置 + 旋转 + 缩放（40 字节，对齐 4）
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
struct Transform {
    pos: [f32; 3],
    rot: [f32; 4],
    scale: [f32; 3],
}

/// 世界引用关系：所属 world/序列号/世代/标志（16 字节）
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
struct WorldRef {
    world_id: u32,
    serial: u32,
    generation: u32,
    flags: u32,
}

/// 继承关系（父）：无 niche 的 12 字节
#[derive(Clone, Copy, Debug, PartialEq)]
struct Parent {
    parent: Option<Entity>,
}

/// 继承关系（子）：Spine 里子实体用 Vec 存放（模拟真实层级，读取 len/首元素）
#[derive(Debug, PartialEq)]
struct Children {
    children: Vec<Entity>,
}

/// 结构变动用标记组件（模拟脚本动态挂载组件导致的 archetype 迁移）
#[derive(Clone, Copy, Debug, PartialEq)]
struct Marker;

const N: usize = 10_000;
const TICKS: u32 = 30;
const WARMUP: u32 = 5;
const MIGRATE_PER_TICK: usize = N / 100;

// ── 场景构建 ───────────────────────────────────────────────────────────

fn build_world() -> (World, Vec<Entity>) {
    let mut w = World::new();
    let mut entities = Vec::with_capacity(N);
    for i in 0..N {
        let e = w
            .create4(
                Transform {
                    pos: [i as f32, 0.0, 0.0],
                    rot: [1.0, 0.0, 0.0, 0.0],
                    scale: [1.0, 1.0, 1.0],
                },
                WorldRef {
                    world_id: 0,
                    serial: i as u32,
                    generation: 0,
                    flags: 0,
                },
                Parent {
                    parent: if i == 0 { None } else { Some(entities[0]) },
                },
                Children {
                    children: Vec::new(),
                },
            )
            .unwrap();
        entities.push(e);
    }
    (w, entities)
}

/// 统计期望值（Black-box 累积，防止编译器优化掉整个循环）
fn fake_use(v: u64) {
    black_box(v);
}

// ── 模式 A：每次访问完整 ECS 查询（别名方案 · 脚本最坏路径）───────────────

fn lookup_per_access(w: &mut World, entities: &[Entity], dt: f32) -> u64 {
    let mut sum = 0u64;
    for &e in entities {
        let t = w.get_mut::<Transform>(e).unwrap().unwrap();
        t.pos[0] += dt;
        let wr = w.get::<WorldRef>(e).unwrap().unwrap();
        sum += wr.serial as u64;
        let p = w.get::<Parent>(e).unwrap().unwrap();
        sum += p.parent.is_some() as u64;
        let c = w.get::<Children>(e).unwrap().unwrap();
        sum += c.children.len() as u64;
    }
    sum
}

// ── 模式 B：每帧导出一次 GO 视图，当帧直访（脚本层实际形态）────────────────

fn resolve_per_frame(w: &mut World, entities: &[Entity], dt: f32) -> u64 {
    // 每帧：从 Entity 解析出各组件的访问位置（本步骤=完整查询链）
    let mut views: Vec<(
        *mut Transform,
        *const WorldRef,
        *const Parent,
        *const Children,
    )> = Vec::with_capacity(entities.len());
    for &e in entities {
        let t = w.get_mut::<Transform>(e).unwrap().unwrap() as *mut Transform;
        let wr = w.get::<WorldRef>(e).unwrap().unwrap() as *const WorldRef;
        let p = w.get::<Parent>(e).unwrap().unwrap() as *const Parent;
        let c = w.get::<Children>(e).unwrap().unwrap() as *const Children;
        views.push((t, wr, p, c));
    }
    // 当帧：多次直访
    let mut sum = 0u64;
    for &(t, wr, p, c) in &views {
        // Safety: 单线程；本帧内无组件增删，迁移不会发生，指针保持有效。
        unsafe {
            (*t).pos[0] += dt;
            sum += (*wr).serial as u64;
            sum += (*p).parent.is_some() as u64;
            sum += (*c).children.len() as u64;
        }
    }
    sum
}

// ── 模式 C：包装句柄持持久裸指针（句柄方案 · 结构稳定期最优路径）──────────

struct GoHandle {
    #[allow(dead_code)]
    entity: Entity,
    transform: *mut Transform,
    world_ref: *const WorldRef,
    parent: *const Parent,
    children: *const Children,
}

// Safety: 单线程场景；调用方保证句柄只在组件集稳定（无迁移）时使用。
unsafe fn deref_handle(h: &GoHandle, dt: f32) -> u64 {
    unsafe {
        (*h.transform).pos[0] += dt;
        let mut sum = (*h.world_ref).serial as u64;
        sum += (*h.parent).parent.is_some() as u64;
        sum += (*h.children).children.len() as u64;
        sum
    }
}

fn make_handles(w: &mut World, entities: &[Entity]) -> Vec<GoHandle> {
    entities
        .iter()
        .map(|&e| {
            let t = w.get_mut::<Transform>(e).unwrap().unwrap() as *mut Transform;
            let wr = w.get::<WorldRef>(e).unwrap().unwrap() as *const WorldRef;
            let p = w.get::<Parent>(e).unwrap().unwrap() as *const Parent;
            let c = w.get::<Children>(e).unwrap().unwrap() as *const Children;
            GoHandle {
                entity: e,
                transform: t,
                world_ref: wr,
                parent: p,
                children: c,
            }
        })
        .collect()
}

fn persistent_ptr(handles: &[GoHandle], dt: f32) -> u64 {
    let mut sum = 0u64;
    for h in handles {
        sum += unsafe { deref_handle(h, dt) };
    }
    sum
}

// ── 模式 D：Rust 系统内部 archetype 连续列迭代（别名方案最优路径）──────────

fn archetype_iter(w: &mut World, dt: f32) -> u64 {
    w.iterate_mut::<Transform>(|_, t| t.pos[0] += dt);
    let mut sum = 0u64;
    let mut s1 = 0u64;
    w.iterate::<WorldRef>(|_, wr| s1 += wr.serial as u64);
    sum += s1;
    let mut s2 = 0u64;
    w.iterate::<Parent>(|_, p| s2 += p.parent.is_some() as u64);
    sum += s2;
    let mut s3 = 0u64;
    w.iterate::<Children>(|_, c| s3 += c.children.len() as u64);
    sum += s3;
    sum
}

// ── 模式 C-mig：结构变化（archetype 迁移）时的句柄失效重建 ─────────────────

fn persistent_ptr_mig(w: &mut World, entities: &[Entity], dt: f32) -> u64 {
    // 1% 实体挂/摘 Marker → 触发 archetype 迁移（行移动、列基址变化）
    for &e in entities.iter().take(MIGRATE_PER_TICK) {
        if w.contains::<Marker>(e).unwrap() {
            w.remove::<Marker>(e).unwrap();
        } else {
            w.add(e, Marker).unwrap();
        }
    }
    // 缓存全量失效 → 重建（真实工程对"结构变更"的处理：句柄作废重解析）
    let handles = make_handles(w, entities);
    let mut sum = 0u64;
    for h in &handles {
        sum += unsafe { deref_handle(h, dt) };
    }
    sum
}

// ── 模式 F：正式 GoHandle（位置缓存 + 世代校验，无裸指针）────────────────

fn build_go_scene() -> (
    xengine_core::go::SceneHandle,
    Vec<xengine_core::go::GoHandle>,
) {
    let mut scene = SceneHandle::new();
    let mut handles = Vec::with_capacity(N);
    for i in 0..N {
        let e = scene
            .create_go(GoTransform {
                position: Vector3F::new(i as f32, 0.0, 0.0),
                ..GoTransform::default()
            })
            .unwrap();
        handles.push(scene.go_handle(e));
    }
    (scene, handles)
}

fn go_handle_access(scene: &mut Scene, handles: &[xengine_core::go::GoHandle], dt: f32) -> u64 {
    let mut sum = 0u64;
    for h in handles {
        let mut view = scene.go_view(*h).unwrap();
        view.transform().position.x += dt;
        let serial = view.scene_ref().serial;
        let _ = view.parent();
        sum += serial;
    }
    sum
}

// ── 计时与报告 ─────────────────────────────────────────────────────────

fn bench<F: FnMut() -> u64>(name: &str, ticks: u32, mut f: F) {
    // 预热
    for _ in 0..WARMUP {
        fake_use(f());
    }
    let start = Instant::now();
    let mut acc = 0u64;
    for _ in 0..ticks {
        acc = acc.wrapping_add(f());
    }
    let elapsed = start.elapsed();
    let per_tick = elapsed.as_nanos() as f64 / ticks as f64;
    let per_go = per_tick / N as f64;
    println!(
        "{name:<28} total {:>8.2} ms   per-tick {:>6.1} µs   {:.2} ns/GO",
        elapsed.as_secs_f64() * 1e3,
        per_tick / 1e3,
        per_go
    );
    fake_use(acc);
}

fn main() {
    println!("GO 组件访问模型对比（N={N}, TICKS={TICKS}, 结构稳定=无迁移）\n");

    // 模式 A：完整查询（每次访问）
    {
        let (mut w, entities) = build_world();
        bench("A. lookup_per_access", TICKS, || {
            lookup_per_access(&mut w, &entities, 1.0 / 60.0)
        });
        // 正确性角标：第一实体位移应与 (预热+测量) 总 tick 数 × 步长一致
        let first = entities[0];
        let t = w.get::<Transform>(first).unwrap().unwrap();
        let expected = (WARMUP + TICKS) as f32 / 60.0;
        assert!(
            (t.pos[0] - expected).abs() < 1e-3,
            "pos={} expected={}",
            t.pos[0],
            expected
        );
    }

    // 模式 B：每帧导出视图
    {
        let (mut w, entities) = build_world();
        bench("B. resolve_per_frame", TICKS, || {
            resolve_per_frame(&mut w, &entities, 1.0 / 60.0)
        });
    }

    // 模式 C：持久句柄
    {
        let (mut w, entities) = build_world();
        let handles = make_handles(&mut w, &entities);
        bench("C. persistent_ptr", TICKS, || {
            persistent_ptr(&handles, 1.0 / 60.0)
        });
    }

    // 模式 D：archetype 迭代
    {
        let (mut w, _) = build_world();
        bench("D. archetype_iter", TICKS, || {
            archetype_iter(&mut w, 1.0 / 60.0)
        });
    }

    // 模式 E：持久句柄 + 结构变化
    {
        let (mut w, entities) = build_world();
        bench("E. persistent_ptr_mig", TICKS, || {
            persistent_ptr_mig(&mut w, &entities, 1.0 / 60.0)
        });
    }

    // 模式 F：正式 GoHandle（位置缓存 + 世代校验）
    {
        let (mut scene, handles) = build_go_scene();
        bench("F. go_handle", TICKS, || {
            go_handle_access(&mut scene, &handles, 1.0 / 60.0)
        });
    }
}
