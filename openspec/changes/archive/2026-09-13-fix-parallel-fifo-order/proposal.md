## Why

`core-parallel-jobs` 归档后暴露两处问题（本地全量测试循环复现，CI 为唯一门禁）：

1. **测试缺陷**：`parallel::jobs::tests::same_priority_keeps_submission_order` 断言"同优先级作业的**完成**顺序等于提交顺序"。但 `JobSystem` 的等待路径是**帮助式**的——`end_frame()` 与 `handle.wait()` 都会让调用线程从队列中**取用并执行**作业。因此当工作线程与调用线程并发执行时，取用顺序仍是 FIFO，但**完成顺序不再有保证**（实测日志出现 `["first","third","second"]`：主线程取走 `second` 后尚未执行完，工作线程已取走并完成 `third`）。同源风险也存在于 `high_priority_jumps_ahead_of_low`（同一模式，尚未复现但同理）。
2. **规范场景错误**：`openspec/specs/core-jobs/spec.md` 的"优先级"要求中，场景 `同优先级 FIFO` 写为"日志顺序等于提交顺序"——与实现语义不符（实现保证的是取用/派发顺序）。规范文本本身需要修正，否则后续实现会照着一个不可满足的场景去改代码。

## What Changes

- **`core-jobs` 规范修正**（`openspec/specs/core-jobs/spec.md`，"优先级"要求）：明确 FIFO 约束作用在**取用（dispatch）顺序**上；并发执行下**完成顺序不作保证**，并把 `同优先级 FIFO` 场景改写为可满足、可验证的表述（区分"取用序"与"由依赖/单执行者决定的完成序"）。
- **测试修正**（`crates/xengine-core/src/parallel/jobs.rs`）：
  - `same_priority_keeps_submission_order`：改为通过 `wait_blocking()`（**不帮助**）等待最后一个句柄，使唯一工作线程按 FIFO 独占执行，从而确定性地断言执行/完成顺序。
  - `high_priority_jumps_ahead_of_low`：同样改用 `wait_blocking()`，消除主线程帮助执行导致的顺序竞争。
- 仅测试与规范文本改动，**无生产代码行为变更**。

## Capabilities

### New Capabilities
（无）

### Modified Capabilities
- `core-jobs`: "优先级"要求明确 FIFO 只约束取用顺序、并发执行下完成顺序不作保证；`同优先级 FIFO` 场景改为可满足表述

## Impact

- 仓库：`crates/xengine-core/src/parallel/jobs.rs`（2 个单测）、`openspec/specs/core-jobs/spec.md`
- API：无变化（无公开接口改动）
- 依赖 / 层 / 后端：无变化（核心层 100% Rust，零外部依赖；不涉及设备层）
- 性能预算：无影响（仅测试路径）

## Acceptance Criteria

- `cargo test --workspace` 连续多轮全绿（含此前 flaky 的两个优先级测试），单测数量不变或增加
- 优先级语义仍被验证：`High` 在 `Low` 之前被取用、同优先级按提交序取用（在单执行者条件下断言）
- `cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all -- --check` 通过
- `openspec validate fix-parallel-fifo-order` 通过并完成归档
- 生产代码零行为变更（`cargo test` 中其余 160+ 项断言不变）
