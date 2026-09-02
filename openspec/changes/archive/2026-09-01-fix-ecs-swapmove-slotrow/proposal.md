## Why

`Archetype::remove_row` 与 `remove_row_migrate` 返回了错误的"被搬入实体 index"。两者都直接返回 `self.entities.swap_remove(row)`，但 `Vec::swap_remove(row)` 返回的是【row 处被移除的实体 index】，而 World 的 add/remove/destroy（world.rs 中 `if let Some(m) = moved { self.slots[m as usize].row = row as u32; }`）需要的是【swap 前 last 行处、被搬进 row 的实体 index】。搬入实体的 `slot.row` 因此永不更新，导致后续按 `slot.row` 访问越界（debug `storage.rs:164 assertion failed: i < self.len`；release `swap_remove index (is 50) should be < len (is 50)`）或静默读到别的实体的组件值（get/get_mut 数据错位）。该 bug 在批量对实体 add/remove 组件（触发 archetype 迁移）时必现。

## What Changes

- 修正 `crates/xengine-core/src/archetype.rs` 两个函数在 swap 分支下的返回语义：
  - `remove_row`：`row != last` 时先读取 `self.entities[last]`（被搬入的实体），再 `swap_remove(row)`，返回 `Some(moved_in)`；`row == last` 时走 `pop()` 返回 `None`。
  - `remove_row_migrate`：同样先读 last 再 swap，保持既有 `# Safety` 注释语义；两分支返回语义与 `remove_row` 一致。
  - 两处注释明确注明"swap_remove 返回被移除实体，搬入实体须在 swap 前读取"。
- 新增回归测试 `crates/xengine-core/tests/go_mig_stress.rs`：100 个实体（带 Transform）批量反复 toggle `Marker` 组件 35 次，每 tick 后断言 `entity_count`、含 Marker 实体数、以及每个实体 `get::<Transform>` 值不变（`batch_toggle_invariants` / `batch_toggle_count`）；当前 `cargo test -p xengine-core` 全绿。
- 补全本 OpenSpec 变更提案（proposal / specs（delta）/ design / tasks）以记录根因与验收。

## Capabilities

### New Capabilities
- 无（本变更为缺陷修复，不新增能力）。

### Modified Capabilities
- `core-ecs`: 在 `openspec/specs/core-ecs/spec.md` 中为"生命周期 / Archetype 迁移"新增一条独立 Requirement 与对应 Scenarios，明确 swap 搬入实体后其槽位行号 MUST 被更新、per-entity 访问 MUST 返回该实体自身组件。行为正确性修复且公开 API 不变。

## Non-goals

- 不涉及 GO 层（`crates/xengine-core/benches/go_access.rs` 的 GO 层 spike）——本变更只修 archetype 迁移的 slot.row 一致性问题，不评估 GO 访问模式。
- 不做性能优化 / 行为重构；不引入新的外部依赖或架构变更。
- 不更改任何公开 API 签名（`remove_row` / `remove_row_migrate` 签名与调用契约不变，仅修正返回值的语义）。

## Impact

- 层 / 后端：核心层（`xengine-core`），纯 Rust；不涉及设备平台层（D3D12 / Metal / Vulkan）。
- API：公开 API 不变；修复的是行为正确性（之前批量 add/remove/destroy 触发迁移时 slot.row 错位）。
- 行为：从"越界 panic 或静默数据错位"变为"迁移后 per-entity 访问返回自身组件值"。
- 测试：新增集成回归测试 `go_mig_stress.rs`（已存在），覆盖 100 实体批量 toggle / destroy 的正确性；既有单测（单实体、`row == last` 走 pop 分支）保持通过。

## Acceptance Criteria

- `cargo test -p xengine-core` 全部通过（含新回归测试 `batch_toggle_invariants`、`batch_toggle_count`）。
- 批量 add/remove 组件（触发 archetype 迁移）后，每个实体 `get::<Transform>` 返回自身组件值，无越界 / panic / 数据错位。
- 批量 `destroy` 后其余实体仍可访问且值正确；反复 toggle（往返迁移）后句柄与数据一致。
- `openspec change validate fix-ecs-swapmove-slotrow` 通过。
