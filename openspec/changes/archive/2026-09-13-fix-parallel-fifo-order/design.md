## Context

`core-parallel-jobs` 已归档（`openspec/specs/core-jobs`，14 项要求）并合入 main（PR #7/#8）。随后本地对 `cargo test --workspace` 做 6 轮循环，复现 2 次失败：

```
---- parallel::jobs::tests::same_priority_keeps_submission_order stdout ----
left: ["first", "third", "second"]
right: ["first", "second", "third"]
```

根因分析：`JobSystem` 的等待是**帮助式**的——`end_frame()` 与 `JobHandle::wait()` 都会让调用线程从队列取用并执行作业（NeoX `wait` / `schedule_one` 语义）。测试在单 worker 实例上先阻塞该 worker，再提交三个同优先级作业，最后调用 `end_frame()`；此时**主线程也参与取用**：主线程取走 `second` 后，工作线程已取走并完成 `third` —— 取用序仍是 FIFO，但完成序不再等于提交序。

对应地，归档规范中的场景 `同优先级 FIFO` 写为"日志顺序等于提交顺序"，在帮助式等待下不可满足，属规范缺陷（会误导后续实现）。

## Goals / Non-Goals

**Goals:**
- 修正 `core-jobs` 规范：FIFO 约束作用于**取用顺序**；并发执行下完成顺序不作保证
- 修正两个受影响的单测，使其确定性验证"取用序"，消除 flaky
- 生产代码零行为变更

**Non-Goals:**
- 修改 `JobSystem` 的调度/等待语义（帮助式等待是 NeoX 对齐的设计决策，保持不变）
- 引入"完成序"保证（若未来需要，应作为独立能力：依赖图或有序提交队列）
- 其余测试的稳定性加固（本轮循环未复现其它 flaky）

## Decisions

### D1 语义澄清而非行为变更（用户既有决策的推论）
帮助式等待是 `core-parallel-jobs` 的设计决策（D5：`wait` 帮助式 / `wait_blocking` 阻塞式）。因此"同优先级 FIFO"只能约束取用顺序——并发执行下完成顺序天然不保证（NeoX 同样如此）。本变更澄清规范文本，不改实现。**备选**：给 `end_frame()` 加"不帮助"模式（会削弱帧屏障语义，且与 D5 冲突，否决）。

### D2 测试改为单执行者确定性断言
`same_priority_keeps_submission_order` 与 `high_priority_jumps_ahead_of_low` 改用 `wait_blocking()`（**不帮助**）等待最后一个句柄：唯一工作线程独占执行，取用序 = 完成序，断言确定性成立。**备选**：断言"取用集合/计数"而非顺序（验证力度下降，否决）；用 `JobChain` 强制顺序（验证的就不是优先级/FIFO 了，否决）。

### D3 不引入新的被测能力
测试仍覆盖既有要求：高优先级先于低优先级被取用、同优先级按提交序取用。新增的规范场景"并发执行下完成顺序不作保证"由现有帮助式等待测试（`helping_wait_consumes_the_main_queue`）间接覆盖，不新增生产代码。

## Risks / Trade-offs

- [规范场景改宽后验证力度下降] → 场景仍要求"取用序 = 完成序（单执行者）"的确定性断言，只是把"并发完成序"显式排除；语义更精确而非更松。
- [仍有其它未复现的 flaky] → 本轮 6 轮 workspace 循环 + 12 轮 core lib 循环仅复现该一项；修复后再做多轮循环验证。
- [测试改动掩盖真实缺陷] → 已确认根因是"完成序不保证"这一固有语义（帮助式等待），非调度器 FIFO 失效（取用序正确）。

## Migration Plan

1. 修 `crates/xengine-core/src/parallel/jobs.rs` 两个测试（改用 `wait_blocking`）
2. 多轮 `cargo test --workspace` 循环验证无 flaky
3. `cargo clippy` / `cargo fmt --check` / `openspec validate` → 归档 → 分支 MR

## Open Questions

- 是否需要为"确定性完成序"提供显式能力（例如有序队列提交或完成回调序号）？——如后续有消费者需要，另开变更。
