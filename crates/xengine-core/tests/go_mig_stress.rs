//! 回归测试：archetype swap-remove 后实体行簿记（slot.row）一致性。
//!
//! 背景：`Archetype::remove_row` / `remove_row_migrate` 曾把
//! `Vec::swap_remove(row)` 的返回值（被移除实体）误当作搬入实体，
//! 导致搬入实体的槽位行号不更新；批量 add/remove 迁移与 destroy
//! 路径会出现越界 panic（`swap_remove index (is 50) should be < len (is 50)`）
//! 或静默数据错位。本套用例锁定修复后的行为：批量 toggle（add/remove 迁移）
//! 与批量 destroy（swap-remove 删除）后，每个存活实体的组件值与其句柄一致。

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

/// destroy（swap-remove 删除路径）回归：批量销毁子集后，剩余实体槽位行
/// 簿记必须一致——每个存活实体可解析且值正确。先毁前部（触发大量
/// 非末行 swap），再毁中段（残余 swap），确保覆盖 `remove_row` 的
/// 非单实体分支。
#[test]
fn batch_destroy_keeps_remaining_entities_consistent() {
    let mut w = World::new();
    let mut es = Vec::new();
    for i in 0..100u32 {
        es.push(w.create1(Transform(i as f32)).unwrap());
    }
    // 销毁前 50 个（每个 destroy 都是非末行 swap-remove）。
    for &e in es.iter().take(50) {
        w.destroy(e).unwrap();
    }
    assert_eq!(w.entity_count(), 50);
    // 再销毁中段 25 个（继续打乱剩余行的 swap 布局）。
    for &e in es.iter().skip(50).take(25) {
        w.destroy(e).unwrap();
    }
    assert_eq!(w.entity_count(), 25);
    // 剩余实体（尾部 25 个）值必须与句柄一致。
    for &e in es.iter().skip(75) {
        let t = w
            .get::<Transform>(e)
            .unwrap()
            .expect("entity must be alive");
        assert_eq!(t.0, e.index() as f32, "corrupt entity after destroy");
    }
    // 已销毁句柄全部失效（不是 stale 数据）。
    for &e in es.iter().take(75) {
        assert!(!w.contains_entity(e));
        assert!(w.get::<Transform>(e).is_err());
    }
}

/// 交替 destroy + toggle：destroy 换行与 add/remove 迁移交叠时，
/// 行簿记仍一致（覆盖 remove_row 与 remove_row_migrate 的协同）。
#[test]
fn destroy_then_toggle_mixed_invariants() {
    let mut w = World::new();
    let mut es = Vec::new();
    for i in 0..100u32 {
        es.push(w.create1(Transform(i as f32)).unwrap());
    }
    // 销毁一半（偶数槽位）。
    for &e in es.iter().step_by(2) {
        w.destroy(e).unwrap();
    }
    // 幸存者 = 奇数槽位，批量 toggle 35 次 + 全量 get。
    let survivors: Vec<Entity> = es.iter().skip(1).step_by(2).copied().collect();
    for tick in 0..35 {
        toggle_n(&mut w, &survivors);
        for &e in &survivors {
            let t = w.get::<Transform>(e).unwrap().unwrap();
            assert_eq!(t.0, e.index() as f32, "corrupt at tick {tick}");
        }
    }
    assert_eq!(w.entity_count(), 50);
}
