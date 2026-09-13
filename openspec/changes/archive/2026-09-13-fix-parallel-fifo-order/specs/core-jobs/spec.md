## MODIFIED Requirements

### Requirement: 优先级
- OLD: 作业提交 MUST 接受 `JobPriority`（`High` / `Normal` / `Low`）。调度 MUST 为非抢占式：更高优先级作业 MUST 在调度点优先于更低优先级作业被工作线程取用；同优先级作业 MUST 按提交序取用（FIFO）。优先级 MUST NOT 改变作业结果与可观察的依赖语义（仅影响取得顺序）。
- NEW: 作业提交 MUST 接受 `JobPriority`（`High` / `Normal` / `Low`）。调度 MUST 为非抢占式：更高优先级作业 MUST 在调度点优先于更低优先级作业被取用；同优先级作业 MUST 按提交序取用（FIFO）。**顺序约束的作用域 MUST 为"取用（dispatch）顺序"**：由于等待是帮助式的（`wait` / `end_frame` 会让调用线程参与执行），并发执行时**多个作业的完成顺序 MUST NOT 被假定为提交顺序**；需要确定性完成顺序的调用方 MUST 自行建立依赖（如 `JobChain`）或使用不帮助的等待（`wait_blocking`）配合单执行者。优先级 MUST NOT 改变作业结果与依赖语义（仅影响取用顺序）。

#### Scenario: 高优先级先于低优先级被取用
- **WHEN** 先提交一个 `Low` 优先级作业（等待外部信号），再提交一个 `High` 优先级作业，工作线程数 = 1
- **THEN** `High` 作业在 `Low` 作业完成后、任何其后提交的 `Low` 作业之前被取用

#### Scenario: 同优先级 FIFO
- **WHEN** 以相同优先级依次提交三个作业（追加到同一日志），并用不帮助的等待（`wait_blocking`）等待最后一个句柄，使唯一工作线程独占执行
- **THEN** 日志顺序等于提交顺序（取用序 = 完成序）

#### Scenario: 并发执行下完成顺序不作保证
- **WHEN** 以相同优先级提交多个作业，且调用线程在帮助式等待（`wait` / `end_frame`）中参与了执行
- **THEN** 各作业仍按提交序被取用，但完成顺序 MAY 与提交顺序不同（调用方 MUST NOT 依赖它；需要顺序时使用 `JobChain` 或 `wait_blocking`）
