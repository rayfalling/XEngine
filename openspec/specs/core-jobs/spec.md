# core-jobs Specification

## Purpose
核心层并行执行底座：`JobSystem`（帧任务队列 + 工作线程池——NeoX `xjobsystem` 对齐的线程预算、三级优先级、Compute/Current/Main 执行位置、帮助式与阻塞式等待、panic 安全、`begin_frame`/`end_frame` 帧屏障、单线程等价模式、作用域 `parallel_for`）与 `ThreadPool`（长任务专用线程 lane，线程预算与 `JobConfig::reserve` 互相扣除）。由 go 层变换传播等系统消费，并作为后续系统级并行调度与并行查询原语的地基。

## Requirements

### Requirement: 作业系统初始化与线程预算
`xengine_core::parallel` SHALL 提供 `JobSystem` 与 `JobConfig`（零外部依赖，仅 std）。`JobConfig` MUST 含 `hint: f32`（`hint > 1.0` 指定期望工作线程数；`hint <= 1.0` 指定占硬件线程数的比例，`1.0` 即用满硬件线程）、`reserve: u32`（从上述结果中扣除、留给专用线程的线程数）、`min: u32`、`max: u32`；默认值 MUST 为 `hint = 1.0`、`reserve = 2`、`min = 1`、`max = 8`（对齐 NeoX `InitializeParam`）。工作线程数 MUST 计算为 `clamp(f(hint) - reserve, min, max)` 且 `>= 1`。`JobSystem::new(config)` SHALL 启动工作线程；`JobSystem::single_threaded()` SHALL 创建不启动任何工作线程的单线程实例（所有作业在调用线程内联执行）；`worker_count()` MUST 返回实际工作线程数。`Drop` MUST 关停并 join 全部工作线程，且 MUST NOT 遗留阻塞线程。

#### Scenario: 线程预算计算
- **WHEN** 以默认 `JobConfig`（`hint=1.0`、`reserve=2`、`min=1`、`max=8`）在 8 硬件线程机器上创建
- **THEN** `worker_count() == 6`；若硬件线程为 2 则结果为 `1`（下限生效）；若显式 `hint = 4.0, reserve = 0, min = 1, max = 8` 则结果为 `4`

#### Scenario: 单线程模式不启动线程
- **WHEN** `JobSystem::single_threaded()`
- **THEN** `worker_count() == 0`；提交的作业在调用线程内联完成；`Drop` 不需要 join 任何线程

#### Scenario: 关停回收
- **WHEN** `JobSystem` 被 drop（帧屏障之后）
- **THEN** 全部工作线程 join 结束、无阻塞残留，进程可正常退出

### Requirement: 帧任务队列与帧屏障
`JobSystem` SHALL 提供帧任务队列语义：`begin_frame(frame: u64)` 标记新帧开始；帧内提交的作业属于该帧；`end_frame() -> FrameStats` MUST 作为**帧屏障**——返回时帧内提交的全部作业 MUST 已完成、主线程本地队列 MUST 已泵空、该帧提交计数 MUST 归零。`end_frame()` MUST NOT 遗留跨帧未完成作业。未调用 `begin_frame` 时提交的作业 MUST 归属当前帧上下文（不得 panic）。

#### Scenario: 帧屏障返回即完成
- **WHEN** 一帧内提交 N 个作业（含 `spawn`、`spawn_main`、`spawn_parallel_for`）后调用 `end_frame()`
- **THEN** 返回时全部 N 个作业已完成、帧队列为空，`FrameStats` 报告该帧作业数与耗时

#### Scenario: 跨帧无残留
- **WHEN** 第 1 帧提交并 `end_frame`，随后第 2 帧 `begin_frame` 并再次 `end_frame`
- **THEN** 第 2 帧的 `FrameStats.jobs` 只统计第 2 帧提交的作业（无跨帧残留或重复统计）

### Requirement: 作业提交与执行位置
`JobSystem` SHALL 提供三类执行位置 `JobCategory`：`Compute`（工作线程池）、`Current`（调用线程内联执行）、`Main`（主线程本地队列）。`spawn(name, priority, f)` MUST 等价于 `spawn_at(JobCategory::Compute, ...)` 且要求 `f: FnOnce() + Send + 'static`；`spawn_main(name, f)` MUST 允许 `f: FnOnce() + 'static`（**允许 `!Send`**，例如借用场景数据），其作业 MUST 仅由主线程执行且 MUST 按提交序执行（先进先出），由 `run_one_local()` / `pump()` / `end_frame()` 驱动。`Current` 类别的作业 MUST 在提交调用线程上立即执行完毕后再返回句柄。

#### Scenario: 主线程作业承载非 Send 数据
- **WHEN** 在主线程调用 `spawn_main`，闭包捕获 `Rc<RefCell<..>>` 等 `!Send` 数据并修改之
- **THEN** 编译通过；`pump()` / `end_frame()` 后修改可见；工作线程从不执行该闭包

#### Scenario: 主线程作业按提交序
- **WHEN** 依次 `spawn_main` 三个闭包（追加 A、B、C 到同一日志）
- **THEN** `pump()` 后日志为 A、B、C（提交序，确定性）

#### Scenario: Current 内联执行
- **WHEN** 调用 `spawn_at(JobCategory::Current, ..)` 并立即检查副作用
- **THEN** 返回时闭包已在本线程执行完毕（句柄 `is_finished() == true`）

### Requirement: 优先级
作业提交 MUST 接受 `JobPriority`（`High` / `Normal` / `Low`）。调度 MUST 为非抢占式：更高优先级作业 MUST 在调度点优先于更低优先级作业被工作线程取用；同优先级作业 MUST 按提交序取用（FIFO）。优先级 MUST NOT 改变作业结果与可观察的依赖语义（仅影响取得顺序）。

#### Scenario: 高优先级先于低优先级被取用
- **WHEN** 先提交一个 `Low` 优先级作业（等待外部信号），再提交一个 `High` 优先级作业，工作线程数 = 1
- **THEN** `High` 作业在 `Low` 作业完成后、任何其后提交的 `Low` 作业之前被取用

#### Scenario: 同优先级 FIFO
- **WHEN** 以相同优先级依次提交三个作业（追加到同一日志）
- **THEN** 日志顺序等于提交顺序

### Requirement: 作业句柄与作业组
`spawn*` MUST 返回 `JobHandle`；`JobHandle` MUST 提供 `is_finished()` 与 `wait(&self)`（帮助式等待，见等待语义要求）。`JobGroup` SHALL 聚合多个 `JobHandle` 并提供组等待（等价于逐个等待，但 MUST 只阻塞一次唤醒）；`JobGroup` MUST 支持小规模（`<= 4`）句柄零堆分配存储。句柄在其 `JobSystem` 存活期间 MUST 保持有效（`JobSystem` 被 drop 后使用句柄 MUST 返回确定性的失败而非未定义行为）。

#### Scenario: 组等待
- **WHEN** 提交 3 个作业进 `JobGroup` 并对其 `wait()`
- **THEN** 返回时 3 个作业全部完成

#### Scenario: 小规模组零分配
- **WHEN** 构造含 2 个句柄的 `JobGroup`
- **THEN** 不产生堆分配（内联存储；容量上限内的 `push` 不扩容）

### Requirement: 分块并行（parallel_for）
`JobSystem` SHALL 提供 `parallel_for(range: Range<u32>, grain_size: u32, f: impl Fn(u32, u32) + Send + Sync)`（提交后等待全部块完成，fork-join）与 `spawn_parallel_for(...) -> JobHandle`（仅提交，fork-only）。分块 MUST 由 `grain_size` 决定（每块至多 `grain_size` 个元素，最后一块可更小）；`grain_size == 0` MUST 视为默认粒度（不得 panic、不得死循环）。空范围 MUST 立即成功返回（无作业、无 panic）。每块 MUST 恰好执行一次，且块内参数 MUST 为半开区间 `[start, end)`。`f` MUST 能同时被多个线程调用（`Fn`）。

#### Scenario: 结果与串行一致
- **WHEN** `parallel_for(0..1000, 64, |s, e| { for i in s..e { out[i] = i * 2 } })`
- **THEN** `out` 与串行写入结果逐元素一致；块数 == `ceil(1000 / 64) == 16`（每次调用恰好覆盖一次）

#### Scenario: 空范围与零粒度
- **WHEN** `parallel_for(0..0, 64, ..)` 与 `parallel_for(0..10, 0, ..)`
- **THEN** 前者立即完成且不执行任何块；后者以默认粒度正常执行、不 panic、不死循环

#### Scenario: fork-only 可与其它作业重叠
- **WHEN** `spawn_parallel_for(...)` 返回句柄后立即提交另一个独立作业，再对句柄 `wait()`
- **THEN** 两者正确完成、结果不互相污染

### Requirement: 等待语义（帮助式与阻塞式）与自我等待防护
`JobHandle::wait` MUST 为**帮助式等待**：等待期间调用线程 MUST 参与执行可用工作（主线程本地队列的作业与可窃取的分块），不得空转阻塞；`wait_blocking` MUST 为纯挂起等待（不执行其它作业，用于工作线程或不可重入上下文）。等待 MUST 在目标作业完成时返回，且 MUST NOT 因其他作业的失败而挂起。**自我等待 MUST 被检测并给出明确失败**（panic 消息指明作业名/句柄），MUST NOT 静默死锁；递归帮助式等待深度 MUST 有上限保护。

#### Scenario: 帮助式等待执行待办工作
- **WHEN** 在单工作线程实例上提交一个占用该线程的作业，随后主线程提交一个 `spawn_main` 作业并 `wait()` 前者
- **THEN** 前者完成后返回；主线程本地队列中的作业在等待期间被主线程消费（可用计数减少）

#### Scenario: 自我等待被拒绝
- **WHEN** 作业内部对自己（或自己的祖先链）调用 `wait()`
- **THEN** 检测到自我等待并 panic，消息包含作业名；不发生静默死锁

### Requirement: panic 安全
工作线程内作业的 panic MUST 被捕获（不得使工作线程终止、不得使完成计数不归零）。panic 的 payload MUST 被记录并在等待该作业的线程上以 `resume_unwind` 重新抛出；分块并行中任一块 panic MUST 使其余块完成（或安全终止）后，在 `parallel_for` 调用线程上重新抛出**第一个** payload。发生 panic 后 `JobSystem` MUST 仍可继续提交作业并完成帧屏障（无死锁）。

#### Scenario: 作业 panic 传播到等待线程
- **WHEN** 提交一个 `panic!("boom")` 的作业并 `wait()`
- **THEN** 调用线程收到 panic（消息含 "boom"）；工作线程存活；随后提交的作业正常完成

#### Scenario: 分块 panic 不挂起
- **WHEN** `parallel_for` 中某块 panic
- **THEN** `parallel_for` 在调用线程 panic（首个 payload），不挂起、不死锁；随后 `end_frame()` 正常返回

### Requirement: 确定性
作业系统 MUST 保证结果与可观察顺序确定：主线程本地队列按提交序执行；分块并行的结果由调用方按块索引写回，因而 MUST 与 worker 数无关；同优先级队列 MUST 按提交序取用。`JobSystem` MUST NOT 引入基于线程调度时序的可观察差异（例如"哪一块先完成"不得影响最终结果）。`end_frame()` 的 `FrameStats` MUST 只报告确定性量（作业数、完成数、本帧耗时），MUST NOT 让调用方依赖非确定性量。

#### Scenario: worker 数不改变结果
- **WHEN** 同一 `parallel_for` 负载分别在 `worker_count() ∈ {1, 2, 4, 8}` 配置下执行
- **THEN** 输出逐字节一致

#### Scenario: 单线程模式等价
- **WHEN** 同一负载在 `single_threaded()` 与多 worker 配置下执行
- **THEN** 结果一致（仅耗时不同）

### Requirement: 长任务线程池（ThreadPool）
`xengine_core::parallel` SHALL 提供 `ThreadPool` 与 `LaneSpec { name: &'static str, threads: u32 }`：每个 lane MUST 拥有固定数量、**专属于该 lane** 的线程，且这些线程 MUST NOT 参与帧任务（JobSystem 工作线程）执行。`ThreadPool::new(&[LaneSpec])` SHALL 启动 lane 线程；`spawn_long(lane, name, f) -> LongTaskHandle` MUST 把长任务投递到该 lane 并由其专用线程执行（允许跨帧存活），`LongTaskHandle` MUST 提供 `is_finished()` 与 `wait()`；`acquire(lane) -> OwnedThread` MUST 以 RAII 方式借出一条专用线程供调用方在整个长任务期间独占使用，归还 MUST 在 guard drop 时发生（归还后该线程可被再次借出）。线程预算 MUST 与 `JobConfig::reserve` 互相扣除：`reserve` 从帧 worker 预算中划出，专供此类专用线程；lane 线程数 MUST 可查询（`lane_threads(lane)`）。长任务 panic MUST 被捕获并可由 `LongTaskHandle::wait()` 复现；`ThreadPool` drop MUST 关停并 join 全部 lane 线程。

#### Scenario: 长任务不占用帧 worker
- **WHEN** 在 `JobSystem`（`reserve = 2`）+ 一个 `threads = 2` lane 的 `ThreadPool` 配置下，向 lane 投递两个阻塞型长任务
- **THEN** 帧内 `parallel_for` 仍可正常完成（帧 worker 不被长任务占用）；`lane_threads(lane) == 2`

#### Scenario: RAII 借用与归还
- **WHEN** `acquire(lane)` 取得 `OwnedThread` 并在其上执行工作，随后 drop guard
- **THEN** 工作在该 lane 的专用线程上完成；guard drop 后该线程可再次被 `acquire`（归还成功）

#### Scenario: 长任务 panic 可复现
- **WHEN** 长任务 panic 后对其 `LongTaskHandle::wait()`
- **THEN** 调用线程收到该 panic；线程池与 JobSystem 仍可继续使用

### Requirement: 关停与资源回收（线程池侧）
`JobSystem` 与 `ThreadPool` 的 `Drop` MUST 在不依赖调用方协作的前提下完成关停：停止接受新作业、唤醒并 join 全部线程、回收未执行作业的负载（析构闭包）。关停期间 MUST NOT 死锁（包括存在已完成但未被等待的句柄、主线程队列非空、lane 空闲等情形）。

#### Scenario: 关停时主线程队列非空
- **WHEN** `spawn_main` 投递作业但未 `pump()`，随后 drop `JobSystem`
- **THEN** drop 正常返回（未执行作业被析构回收），无死锁、无 panic

#### Scenario: 空闲线程池关停
- **WHEN** 创建 `ThreadPool` 后不投递任何任务并立即 drop
- **THEN** 全部 lane 线程 join 结束，drop 正常返回

### Requirement: 同步原语 JobConditional
`JobConditional` SHALL 提供跨线程一次性等待/唤醒：`wait()` 阻塞直到 `notify_all()` 被调用（或已调用过）；`reset()` MUST 使对象回到未唤醒状态并可再次使用；`notify_all()` MUST 唤醒全部当前等待者。`JobConditional` MUST 可与作业系统共存（等待期间不得造成工作线程饥饿死锁）。

#### Scenario: 等待与唤醒
- **WHEN** 线程 A 调用 `wait()`，随后线程 B 调用 `notify_all()`
- **THEN** A 被唤醒并返回；再次 `reset()` 后可重复该流程

#### Scenario: 先唤醒后等待
- **WHEN** 先 `notify_all()` 再 `wait()`
- **THEN** `wait()` 立即返回（不挂起）

### Requirement: 可观测
`JobSystem` SHALL 提供 `worker_count()`、`active_job_count()`（已提交未完成作业数）、`current_job()`（当前线程正在执行的作业句柄，无则 `None`）、`FrameStats { frame, jobs, completed, duration }`；`ThreadPool` SHALL 提供 `lane_threads(lane)` 与 `lane_count()`。可观测 API MUST 为只读且 MUST NOT 改变调度行为。

#### Scenario: 活跃作业计数
- **WHEN** 提交 3 个作业并全部等待完成
- **THEN** 提交后 `active_job_count() >= 1`（未完成期间）；全部完成后 `active_job_count() == 0`

#### Scenario: 当前作业
- **WHEN** 在作业内部调用 `current_job()`
- **THEN** 返回该作业的句柄（`Some`）；在非作业上下文返回 `None`

### Requirement: 性能预算（并行底座）
`JobSystem` MUST 在 `parallel_for` 热路径上摊还零堆分配（每帧调用 `parallel_for` 不因块数增长而分配：块状态复用/预分配上限内）。`benches/` MUST 提供覆盖并行底座的基准（分块并行吞吐、帧屏障开销），并记录基线数字。CI MUST NOT 以时间阈值作为门禁（runner 核数与噪声不可控），性能证据以人工记录形式进入变更归档。

#### Scenario: 稳态重复调用
- **WHEN** 在稳态下重复调用 `parallel_for`（同一 `JobSystem`，块数固定）
- **THEN** 摊还每次调用的堆分配为 0（复用块状态；单测/基准记录）

#### Scenario: 基准存在且可运行
- **WHEN** 运行 `cargo bench -p xengine-core`
- **THEN** 输出并行底座与传播的分段数据（串行档与并行档），可供归档记录
