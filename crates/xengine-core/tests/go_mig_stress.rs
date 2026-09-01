//! 临时复现测试：批量连续 toggle 组件（触发 archetype 迁移）下 slot.row 的一致性
//! 调查 go_access bench E 模式的 panic：`swap_remove index 50 should be < len 50`。

use xengine_core::{Entity, World};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Transform(f32);

#[derive(Clone, Copy, Debug, PartialEq)]
struct Marker;

fn toggle_n(w: &mut World, entities: &[Entity]) {
    for &e in entities {
        if w.contains::<Marker>(e).unwrap() {
            w.remove::<Marker>(e).unwrap();
        } else {
            w.add(e, Marker).unwrap();
        }
    }
}

#[test]
fn batch_toggle_invariants() {
    let mut w = World::new();
    let mut es = Vec::new();
    for i in 0..100u32 {
        es.push(w.create1(Transform(i as f32)).unwrap());
    }
    // 模拟 bench：35 次 toggle + 全量 get
    for tick in 0..35 {
        toggle_n(&mut w, &es);
        for &e in &es {
            // 每个实体必须仍可解析组件且值正确
            let t = w.get::<Transform>(e).unwrap().unwrap();
            assert_eq!(t.0, e.index() as f32, "tick {tick}: corrupt entity");
        }
    }
}

#[test]
fn batch_toggle_count() {
    let mut w = World::new();
    let mut es = Vec::new();
    for i in 0..100u32 {
        es.push(w.create1(Transform(i as f32)).unwrap());
    }
    toggle_n(&mut w, &es);
    assert_eq!(w.entity_count(), 100);
    let with = |w: &World| {
        let mut n = 0;
        w.iterate::<Marker>(|_, _| n += 1);
        n
    };
    assert_eq!(with(&w), 100);
    toggle_n(&mut w, &es);
    assert_eq!(with(&w), 0);
    assert_eq!(w.entity_count(), 100);
}
