## 1. 代码与测试（已完成，本提案记录）

- [x] 1.1 修正 `crates/xengine-core/src/archetype.rs` 的 `Archetype::remove_row`：`row != last` 分支先读取 `self.entities[last]`（被搬入实体）再 `swap_remove(row)` 并返回 `Some(moved_in)`；`row == last` 走 `pop()` 返回 `None` —— **验证**：`cargo test -p xengine-core` 通过；world.rs 批量 add/remove 迁移后 `get::<Transform>` 值正确、无越界。
- [x] 1.2 修正 `Archetype::remove_row_migrate`：同样先读 last 再 swap，保留既有 `# Safety` 注释语义；`row != last` 返回 `Some(被搬入实体)`、`row == last` 返回 `None` —— **验证**：`cargo test -p xengine-core` 通过；`debug_assert!(row < self.len())` 不再触发。
- [x] 1.3 在两处函数注释说明"swap_remove 返回被移除实体，搬入实体须在 swap 前读取" —— **验证**：源码注释已同步。

## 2. 回归测试（已完成，本提案记录）

- [x] 2.1 新增 `crates/xengine-core/tests/go_mig_stress.rs`（未跟踪，将随本 fix 提交）——**验证**：`batch_toggle_invariants` 对 100 实体批量 toggle `Marker` 35 次，每 tick 断言 `get::<Transform>` 值不变；`batch_toggle_count` 断言 `entity_count == 100` 与含 `Marker` 实体数正确；`cargo test -p xengine-core` 全部通过。

## 3. 提案与文档（本变更产物）

- [x] 3.1 编写本 OpenSpec 变更：`proposal.md`、`specs/core-ecs/spec.md`（delta）、`design.md`、`tasks.md`、`.openspec.yaml`（`schema: spec-driven` + `created: 2026-09-01`）。
- [x] 3.2 `openspec change validate fix-ecs-swapmove-slotrow` 通过 —— **验证**：CLI 输出 "Change fix-ecs-swapmove-slotrow is valid"。
- [x] 3.3 `cargo test -p xengine-core` 全绿 —— **验证**：38 unit + 2 integration（batch_toggle_invariants / batch_toggle_count）+ 0 doc-tests，全部通过。

## 4. 提交（本变更产物，不使用 GO 层文件）

- [x] 4.1 提交 `fix: correct archetype swap-remove return semantics (slot row tracking)`，包含 `archetype.rs`（修改）与 `tests/go_mig_stress.rs`（新增，随本 fix 提交）。
- [x] 4.2 提交 `docs(openspec): proposal for ecs swapmove slotrow fix`，包含 `openspec/changes/fix-ecs-swapmove-slotrow/` 目录。
- [x] 4.3 **不提交** `crates/xengine-core/Cargo.toml`（新增 `[[bench]] name="go_access"`）与 `crates/xengine-core/benches/go_access.rs`（GO 层 spike），保留在工作区。
