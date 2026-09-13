## Why

核心层目前完全单线程：系统以 `&mut World` 串行执行，go 层变换传播阶段 2 虽已按 `PROPAGATE_CHUNK = 64` 切好"待重算实体"并明确声明为**并行接入点**（`crates/xengine-core/src/go/global_transform.rs:74-107`），实际仍是串行循环；且 `compute_world` 每个实体都要新建 `Vec<Entity>` + `HashSet<Entity>`，属热路径堆分配（违反项目"热路径避免堆分配"约定）。要兑现"高性能数据导向引擎"的定位，必须先有**零依赖的并行执行底座**，再把已预留的接入点真正并行化。本变更按 NeoX XJobSystem 的双层模型落地：**JobSystem（帧任务队列 + 工作线程池）** 与 **ThreadPool（长任务专用线程）**，预算互相扣除。

## What Changes

- **新增 `core-jobs` 能力**（`crates/xengine-core/src/parallel/`，零外部依赖，仅 std：thread/atomic/Mutex/Condvar）：
  - `JobSystem`：**帧任务队列** + 工作线程池。`begin_frame(frame)` / `end_frame()` 帧屏障（返回时帧内作业全部完成并清空）；`spawn` / `spawn_at(category)` / `spawn_main`（承载 `!Send` 闭包，仅主线程按提交序执行）；`parallel_for(range, grain_size, f)`（分块 fork-join）与 `spawn_parallel_for(...) -> JobHandle`（fork-only）；三级优先级（High/Normal/Low，非抢占、同优先级按提交序）
  - `JobHandle` / `JobGroup` / `JobChain` / `JobConditional`：句柄 `is_finished` / `wait`（**帮助式等待**，等待线程参与执行可用作业）/ `wait_blocking`（纯挂起）；禁止自我等待（检测后给出明确 panic，不静默死锁）
  - 线程预算**对齐 NeoX**：`JobConfig { hint: f32 = 1.0, reserve: u32 = 2, min: u32 = 1, max: u32 = 8 }`，`workers = clamp(f(hint) - reserve, min, max)`（`hint > 1.0` 指定线程数，`hint <= 1.0` 为硬件线程比例）；`JobSystem::single_threaded()` 提供与并行**逐结果一致**的内联执行模式（测试与确定性基线）
  - **panic 安全**（Rust 特有，NeoX 无对应问题）：worker panic 必须被捕获并记录，由等待线程 `resume_unwind` 抛出；任何 panic 都不得造成完成计数不归零的死锁
  - **确定性**：主线程队列按提交序执行、分块结果按块索引写回 → worker 数变化不改变可观察结果（与 core-frame "帧确定性"要求一致）
  - 可观测：`worker_count` / `active_job_count` / `current_job` / `FrameStats`
  - `ThreadPool`：**命名 lane 的专用线程**（长任务），`spawn_long(lane, f) -> LongTaskHandle`、`acquire(lane) -> OwnedThread`（RAII 借用/归还）；lane 线程与帧 worker **不共享**——`reserve` 从 worker 预算中划出（与 JobSystem 预算互相扣除）
- **core-ecs 扩展**：新增只读并行视图 `World::read_view() -> WorldReadView<'_>`（仅暴露 `get::<T: Sync>()` / `contains::<T>()`）；`WorldReadView` 携带**本变更唯一新增的 `unsafe impl Sync`**（`# Safety`：视图存活期间只做组件列共享读取，不暴露资源/钩子上下文/命令队列，且期间无结构变更）
- **go-layer：传播阶段 2 真正并行化**（`openspec/specs/go-layer/spec.md`）：阶段 2 拆为 **2a 并行计算**（按 `PROPAGATE_CHUNK` 分块，每实体独立遍历祖先链，结果写入预分配缓冲；worker scratch 复用 + 深度上限替代每实体 `HashSet`，**热路径零分配**）与 **2b 串行写回**（写 `GlobalTransform`、清 `TransformDirty`、按原顺序触发 `on_recompute`）；结构变更（清 dirty 触发 archetype 迁移）全部留在串行段，并行段只读 + 写自有缓冲。**BREAKING**：`transform_propagate_system()` → `transform_propagate_system(jobs: Arc<JobSystem>)`（无参单线程便捷构造保留，行为等价）
- **core-frame：Engine 集成**（`openspec/specs/core-frame/spec.md`）：`Engine::with_jobs(Arc<JobSystem>)` / `Engine::jobs()`，`tick` 内 `begin_frame` / `end_frame` 建立帧屏障；帧阶段语义与帧确定性**不变**

## Capabilities

### New Capabilities
- `core-jobs`: 核心层并行执行底座——JobSystem 帧任务队列（工作线程池、线程预算、优先级、执行位置三类、句柄/组/链、`parallel_for` 分块、帮助式等待、panic 安全、确定性、单线程模式、可观测）与 ThreadPool 长任务专用线程（lane、RAII 借用、预算扣除）

### Modified Capabilities
- `core-ecs`: 新增只读并行视图 `WorldReadView`（`World::read_view()`；收窄的 `unsafe impl Sync` + `# Safety` 契约）
- `go-layer`: 变换传播阶段 2 由"串行首版"升级为真并行（2a 并行计算 + 2b 串行写回，热路径零分配，结果与单线程逐字节一致）
- `core-frame`: `Engine` 集成 JobSystem 与帧屏障（`begin_frame` / `end_frame`），帧阶段语义与确定性不变

## Impact

- 仓库：`crates/xengine-core`（新增 `src/parallel/{mod,jobs,threads,sync}.rs`；`world.rs` 只读视图；`go/global_transform.rs` 阶段 2；`frame.rs` 集成；`benches/propagate.rs`；`lib.rs` 导出）、`crates/xengine`（demo 使用并行传播）
- API：新增公开 `xengine_core::parallel::{JobSystem, JobConfig, JobHandle, JobGroup, JobChain, JobConditional, JobPriority, JobCategory, ThreadPool, LaneSpec, LongTaskHandle, OwnedThread, FrameStats}`；`World::read_view`；**BREAKING** `transform_propagate_system` 签名（由无参改为接收 `Arc<JobSystem>`，无参便利路径保留为单线程等价）
- 依赖：**仍为零外部依赖**（不引入 rayon；仅 std 同步原语），符合 `openspec/specs/` 与 README 声明的核心层零外部依赖定位
- 层 / 后端：**核心层（100% Rust）**；不涉及设备平台层，D3D12 / Metal / Vulkan 后端均不受影响
- 性能预算：见 Acceptance Criteria（并行加速比、单线程回归上限、热路径零分配、确定性）

## Non-goals

- **系统级并行调度**（`System: Send` + World 并发访问模型 + 冲突图→并行批次）——下一独立变更；本变更只提供底座，系统仍以 `&mut World` 串行执行
- `iterate_mut_par` / join 查询并行原语（用户系统直接消费并行的 API）——下一变更
- work-stealing 队列、任务窃取调度器（首版为共享队列 + 原子游标分块；按基准决定是否升级）
- fiber/协程/可挂起任务（NeoX `XJOB_RESUMABLE_JOB_IMPL_BOOST_COROUTINE` 对应物）、`SpecificThread` 执行位置（留待与 ThreadPool lane 对接）
- 渲染/设备层并行（渲染提交数据、多线程命令缓冲）、SIMD 内核、脚本运行时、序列化
- 线程亲和性/优先级绑定、Tracy/性能分析器集成（仅保留 `name`/可观测计数）

## Acceptance Criteria

- `cargo test --workspace` 全绿；新增核心公开函数/组件均有单测
- **正确性**：`parallel_for` 结果与串行逐元素一致；`spawn`/`spawn_main` 按提交序可观察；`single_threaded()` 与多 worker 结果一致
- **确定性**：传播在同一输入下于 worker 数 1/2/4/8 时产出逐字节一致的结果；`on_recompute` 每实体恰一次；`TransformDirty` 每实体恰一次重置
- **panic 安全**：worker panic 被捕获，等待线程 `resume_unwind` 收到同一 payload；panic 后 JobSystem 仍可继续提交与帧屏障（无死锁；单测带超时保护）
- **帧屏障**：`end_frame()` 返回后帧内提交的作业全部完成、帧队列清空；跨帧无残留作业
- **并行写安全**：阶段 2a 只读 + 写自有缓冲，2b 串行写回；`WorldReadView` 是唯一新增 unsafe（`unsafe impl Sync` + `# Safety` 文档），并发读一致性有单测
- **热路径零分配**：`compute_world` 不再每实体分配 `Vec`/`HashSet`（worker scratch 复用 + 深度上限；深链与畸形环单测保证不挂起）
- **性能预算**（人工记录进归档，**不作为 CI 门禁**——CI runner 为 2 核且噪声大）：`benches/propagate.rs` 覆盖宽树/深链/平坦三形态 × N=10k/100k，分段报告 2a/2b 耗时。**实测结果（worker=8，N=100k，见 `README.md`）**：阶段 2a 并行化有效（宽树 3.55→0.76 ms，4.6x；深链 1523→185 ms，8.2x）；总加速受串行 2b（逐实体 archetype 迁移清 dirty，占宽树/平坦 75%+ 时长）限制——深链 **6.2x**、宽树 **1.09x（无回退）**、平坦在噪声内。目标"≥2.0x"在 2a 主导负载上达成，在 2b 主导负载上未达成，瓶颈明确并把"2b 批量清除 / dirty 集合化"列为后续独立变更。
- `cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all -- --check` 通过
- `openspec change validate core-parallel-jobs` 通过，并完成归档（合入 main 前置条件）
