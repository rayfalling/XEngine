## Context

XEngine 的 ECS 采用 Archetype SoA 存储：同一组件类型集的实体聚合在一个 archetype，`entities: Vec<u32>` 保存每行对应的实体槽位 index，`slots[i].row` 记录该实体在某 archetype 内的行号。当实体 add/remove/destroy 组件、需要跨 archetype 迁移时，`Archetype::remove_row` / `remove_row_migrate` 负责移除某行并把最后一行 swap 搬入，world.rs 随后用返回的"被搬入实体 index"更新其 `slot.row`。本 bug 正在这两个返回语义上。

## Goals / Non-Goals

**Goals:**
- 修正 `remove_row` / `remove_row_migrate` 的 swap 返回语义，使"搬入实体的槽位行号"正确更新。
- 消除批量 add/remove/destroy 触发迁移时的越界 panic 与静默数据错位。
- 用回归测试锁定多实体迁移场景（既有单测只覆盖单实体 `row == last` 的 pop 路径）。

**Non-Goals:**
- 不涉及 GO 层（`benches/go_access.rs` spike）评估与优化。
- 不做性能优化、不引入外部依赖 / 架构变更。
- 不改公开 API 签名与调用契约。

## Decisions

### D1 根因：`swap_remove` 返回值语义被误用
`Vec::swap_remove(row)` 返回的是【row 位置被移除的元素】，而搬入 `row` 的元素来自最后一个槽位。原实现 `return Some(self.entities.swap_remove(row))` 把"被移除实体"当成"被搬入实体"返回给了 world.rs；于是 world.rs `self.slots[m as usize].row = row as u32;` 更新的是**已移除实体**的 slot（其槽位本就要失效），而真正搬入 `row` 的实体其 `slot.row` 保持旧值/旧行号 → 之后任何按该 `slot.row` 的访问都落到错误行，越界或读到别的实体组件。
- 备选：返回 `Option` 但也返回 pair（更啰嗦）；返回"被搬入实体"是唯一满足调用方需求的选择。

### D2 修复方案：swap 前先读取被搬入实体
两个函数在 `row != last` 分支改为 `let moved_in = self.entities[last]; self.entities.swap_remove(row); Some(moved_in)`；`row == last` 时 `pop()` 并返回 `None`（此时无搬入实体）。列（columns）的 `remove_swap` / `move_swap` 调用顺序保持不变——它们与实体行号按同一索引对齐，仅实体 index 列表的返回语义需要修正。两处注释注明"swap_remove 返回被移除实体，搬入实体须在 swap 前读取"。

### D3 为什么既有单测没抓到
既有 archetype / World 侧测试全部是单实体场景：移除的行总是最后一行（`row == last`），走 `pop()` 分支返回 `None`，从未断言 swap 分支。swap 分支只在"从多实体 archetype 移除中间某行"时触发，而旧测试没有构造这种布局，因此返回语义错误长期未被检测。

### D4 回归测试设计
`crates/xengine-core/tests/go_mig_stress.rs`：100 个实体各带 `Transform`，以循环批量 `toggle` 一个可加可删的 `Marker` 组件，强制实体在"有 Marker / 无 Marker"两个 archetype 间反复往返迁移；每 tick 后对全部实体 `get::<Transform>` 断言值未被改写到别的实体，并断言 `entity_count` 与 Marker 计数。命中 swap 分支且任何一次 slot.row 错位都会立刻越界或断言失败。

## Risks / Trade-offs

- [列索引与实体 index 对齐] → columns 的 `remove_swap`/`move_swap` 与 `entities` 同按 `row` 索引操作，仅调整实体列表的返回值读取顺序，不改变列索引语义；单测同样全绿。
- [`row == last` 仍返回 `None`] → 语义与旧实现一致（无搬入实体时返回 None），不影响既有单实体调用方。
- [修复只作用于核心层] → 不引入平台层依赖，风险面收敛于 archetype 迁移路径。

## Open Questions

- 无（根因已确诊、修复已实施、回归测试已就绪并验证通过）。
