# Tasks

## 1. 规范修正（core-jobs）

- [x] 1.1 `specs/core-jobs/spec.md`：`优先级` 要求改为明确 FIFO 作用域为**取用顺序**，并声明并发执行下完成顺序不作保证（需要确定性完成序时使用 `JobChain` 或 `wait_blocking` + 单执行者）
- [x] 1.2 场景改写：`同优先级 FIFO`（单执行者 + 不帮助等待下断言日志序）与新增 `并发执行下完成顺序不作保证`

## 2. 测试修正

- [x] 2.1 `parallel/jobs.rs::same_priority_keeps_submission_order`：改用 `wait_blocking()` 等待最后一个句柄，消除主线程帮助执行导致的顺序竞争
- [x] 2.2 `parallel/jobs.rs::high_priority_jumps_ahead_of_low`：同样改用 `wait_blocking()`，消除同源竞争
- [x] 2.3 两个测试保持原有语义断言（High 先于 Low 被取用；同优先级按提交序）

## 3. 验证与交付

- [x] 3.1 `cargo test --workspace` 连续 8 轮全绿（含此前 flaky 的两个优先级测试；修复前 6 轮复现 2 次失败）
- [x] 3.2 `cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all -- --check` 通过
- [x] 3.3 `openspec validate fix-parallel-fifo-order` 通过
- [x] 3.4 归档 `openspec archive fix-parallel-fifo-order`

> 交付：分支 `fix/parallel-fifo-order` → MR（关联已归档修复变更，CI 全绿后合入 main）。
