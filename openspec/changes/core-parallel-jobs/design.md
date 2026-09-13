## Context

核心层现状（已归档：`openspec/specs/core-ecs`、`core-frame`、`go-layer`、`core-math`、`ci-gate`）：

- `System` 持有 `Box<dyn FnMut(&mut World)>`（`crates/xengine-core/src/system.rs:33`），测试闭包使用 `Rc<RefCell<..>>`（`schedule.rs`），即系统闭包**非 `Send`**；`Schedule::run_stage` 串行执行每个系统并在系统边界 flush 命令。
- `World` 含 `hook_context: Option<*mut ()>` 与 `resources: HashMap<TypeId, Box<dyn Any>>`（`world.rs:25-36`），因而 `World: !Send + !Sync`；`Column` 已 `unsafe impl Send + Sync`（`storage.rs:23-24`，附 `# Safety` 论证）。
- go 层传播阶段 2 已按 `PROPAGATE_CHUNK = 64` 切分并注释为"并行接入点"（`go/global_transform.rs:74-107`），但 `recompute_chunk` 在同一循环内执行 `world.remove::<TransformDirty>(e)`——**该操作触发 archetype 迁移（行 swap）**，属结构变更，故不能与其它 chunk 并行；`compute_world` 每实体新建 `Vec` + `HashSet`（热路径堆分配）。
- go-layer 规范原文即写明"单线程首版（阶段 2 为串行循环，接口与并行接入点一致）；并行按实体 chunk 化接入点为后续调度层"——本变更即兑现该承诺。

参考实现（用户指定）：NeoX `xjobsystem`（conan 包 `xjobsystem/1.0.4@NeoXEngine/stable`，源码位于本机 `F:\NeteaseProject\NeoX\conan_source\xjobsystem\`；NeoX 侧经 `engine\nxthreading\nxjobsystem\nxjobsystem.h` 别名引入，底层为 Google Filament JobSystem，另有 `nxThreadCategory`/`nxThreadPool` 提供专用线程）。

## Goals / Non-Goals

**Goals:**
- 零依赖并行底座：`JobSystem`（帧任务队列 + 工作线程池）与 `ThreadPool`（长任务专用线程），线程预算对齐 NeoX（`hint=1.0` / `reserve=2` / `min=1` / `max=8`）
- 传播阶段 2 真正并行化（2a 并行计算 + 2b 串行写回），热路径零分配，结果与 worker 数无关
- `core-ecs` 只读并行视图（唯一新增 unsafe 的收敛点）
- `Engine` 帧屏障集成；既有帧语义与帧确定性不变
- 单测覆盖全部新增核心函数/组件；`cargo test` 全绿；基准记录串行/并行基线

**Non-Goals:**
- 系统级并行调度（`System: Send` + World 并发访问模型 + 冲突图→并行批次）——下一变更
- `iterate_mut_par` / join 查询并行原语——下一变更
- work-stealing 调度器（首版共享队列 + 原子游标分块）
- fiber/协程/可挂起任务（NeoX `XJOB_RESUMABLE_JOB_IMPL_BOOST_COROUTINE`）、`SpecificThread` 执行位置
- 渲染/设备层并行、SIMD、脚本运行时、序列化、线程亲和性/优先级绑定、Tracy 类分析器集成

## Decisions

### D1 双层模型：JobSystem（帧任务队列）+ ThreadPool（长任务专用线程）（用户决策）
`JobSystem` 负责帧内并行工作（帧任务队列 + 工作线程池，帧屏障内完成）；`ThreadPool` 负责跨帧长任务（IO/编译等）——**独立两对象**，`JobConfig::reserve` 从帧 worker 预算中划出并留给专用线程，二者预算互相扣除。**备选**：单一池内分两类 lane（长任务可能挤占帧任务，用户否决）；JobSystem 内部持有专用线程池（职责混合，用户否决）。对应 NeoX：`initialize` 的 `reserve` 参数 + `nxThreadCategory` 的 `AsyncPSOCreationThread` 等专用线程池。

### D2 线程预算对齐 NeoX（用户决策）
`JobConfig { hint: f32 = 1.0, reserve: u32 = 2, min: u32 = 1, max: u32 = 8 }`；`workers = clamp(f(hint) - reserve, min, max)`，其中 `hint > 1.0` 取 `hint` 为线程数、`hint <= 1.0` 取 `hint × 硬件线程数`；结果下限 1。`JobSystem::single_threaded()` 为不启动工作线程的内联模式（测试/确定性基线）。CI runner 仅 2 核，故并行相关单测 MUST 显式指定 worker 数（不经默认预算退化为串行而失去覆盖）。

### D3 语义迁移清单（自 NeoX XJobSystem）
**迁移**：帧任务队列 + 帮助式 `wait`（等待线程参与执行，`waitAndRelease` 语义）/ `blocked_wait`（纯挂起）；`run_at` 的执行位置类别；`parallel_for` / `parallel_for_simple(grain_size)`（拆分条件 `count >= grain_size * 2`）与 fork-only 变体；`schedule_one()` 主线程泵；`JobGroup`（小规模内联存储，NeoX 为 ≤7 句柄内联数组）；`JobChain`；`JobConditional`；`get_worker_thread_count` / `get_active_job_count` / `get_this_job`；`XJOB_USE_SINGLE_THREAD` 单线程开关；`reserve` 专用线程预算。
**不迁移**：TBB/Filament 双后端（Rust 侧只用 std 同步原语自研）、u16 `JobID` + 65534 上限 + 10s 超时（改为 slab + 世代校验，见 D6）、boost 协程/fiber 本地数据、Tracy 颜色/事件（仅保留 `name`）、`void*` payload 通道（Rust 侧用泛型闭包 + `Send`/`Sync` 约束表达，见 D8）。

### D4 执行位置三类别：Compute / Current / Main
`Compute` → 工作线程池；`Current` → 调用线程内联执行（NeoX `runAt(CurrentThreadArena, DONT_SIGNAL)`）；`Main` → **主线程线程本地队列**，承载 `!Send` 闭包（如借用 `Scene` 数据的作业），仅主线程按提交序执行。这是 NeoX `runAt(mainThreadId)` 的**安全 Rust 等价物**：`!Send` 闭包永不进入工作线程，因而无需任何 unsafe。`SpecificThread`（NeoX 第四类）留待下一变更与 `ThreadPool` lane 对接。

### D5 等待语义与自我等待防护
`wait` 为帮助式（泵主线程队列 + 消费可窃取分块，对应 NeoX `job_wait`/`waitAndRelease`）；`wait_blocking` 为纯挂起（对应 `blocked_wait`，基于 `Mutex + Condvar`）。**Rust 差异**：NeoX 未处理自我等待（依赖 `XJOB_MAX_RECURSIVE_WAIT_LEVEL 50` 上限）；本实现 MUST 在检出"等待自身/祖先作业"时以明确 panic 失败，并提供递归帮助深度上限。帧屏障 `end_frame()` = 等待本帧全部作业 + 泵空主线程队列（对应 NeoX 帧末 `wait(group)` 惯用法）。

### D6 作业标识：slab + 世代（替代 NeoX u16 jobpool）
`JobHandle` 为 slab 索引 + 世代（`Arc<JobsInner>` 保持系统存活），避免 NeoX 的 `XJOB_MAX_JOB_COUNT 65534` 上限与 `XJOB_JOB_LIMIT_EXCEEDED_TIMEOUT_MS` 超时降级路径；`end_frame()` 后回收本帧槽位（帧内复用）。**备选**：u16 环形 ID（需溢出保护与超时机制，复杂度不划算）。

### D7 panic 安全（Rust 特有，NeoX 无对应问题）
工作线程内闭包以 `catch_unwind(AssertUnwindSafe(..))` 执行；panic payload 存入作业记录；完成计数照常递减（不得因 panic 不归零导致等待者永久阻塞）；等待该作业的线程以 `resume_unwind` 复现 payload。分块并行中任一块 panic → 记录首个 payload，其余块安全收尾后于调用线程抛出。**这是本变更最重要的正确性差异点**：NeoX 用 C++ 异常/直接崩溃处理，Rust 侧若不做捕获即等价于"工作线程静默死亡 + 计数器泄漏 = 死锁"。

### D8 unsafe 边界与并行写策略（用户决策：方案 A）
**并行计算 + 串行写回**：阶段 2a 只读（经 `WorldReadView`）+ 写每实体独立结果缓冲；阶段 2b 串行写回 `GlobalTransform`、清 `TransformDirty`（结构变更全部在此）。本变更**唯一新增 unsafe** 为 `WorldReadView` 的 `unsafe impl Sync`，其 `# Safety` 论证：视图只暴露 `get::<T: Sync>` / `contains`（组件列共享读），不暴露资源、钩子上下文指针、命令队列或任何结构变更入口；视图只在并行区域内、且期间不存在 `&mut World` 访问。**备选**：按 archetype 列切分并行写（额外裸指针切分 unsafe，风险更大，用户否决）；串行快照后并行计算（祖先链遍历仍串行，上限低，用户否决）。

### D9 确定性与"可观察行为不变"
主线程队列按提交序、同优先级 FIFO、分块结果按块索引写回 ⇒ 结果与 worker 数无关（`core-frame` 帧确定性要求不被破坏）。回归手段：同一负载在 worker 数 {1,2,4,8} 与 `single_threaded` 下断言逐字节一致；`on_recompute` 每实体恰一次；`TransformDirty` 重置集合一致。

### D10 Engine 集成方式（显式注入，不用全局态）
`Engine::with_jobs(Arc<JobSystem>)` / `Engine::jobs()`；`tick` 内 `begin_frame`/`end_frame` 建立帧屏障。`transform_propagate_system(jobs: Arc<JobSystem>)` 显式接收句柄（**BREAKING**），并保留无参便利构造（内部使用 `single_threaded` 实例，等价旧行为）。**备选**：全局/环境 JobSystem（隐藏状态、测试不确定，否决）；经 `World` 资源查找（隐式依赖，传播系统取值失败时行为分叉，否决）。

### D11 热路径零分配（传播）
`compute_world` 改为：worker 级 scratch `Vec<Entity>` 复用（跨实体/跨帧不重建）+ 祖先链深度上限（`MAX_HIERARCHY_DEPTH`，缺省按根终止）替代每实体 `HashSet` 环检测；结果写入预分配缓冲。基准分 2a/2b 两段报告，若 2b（逐实体 archetype 迁移清 dirty）成为主导，则"按 archetype 分组批量清除"列为下一变更（本变更先测量、不预优化）。

### D12 基准与性能口径
`benches/propagate.rs`：宽树（1 根 + N 子）/ 深链（N 级链）/ 平坦（无层级）三形态 × N = 10k/100k，报告 2a、2b 分段耗时与串行基线的加速比；`benches/jobs.rs`（或并入）报告 `parallel_for` 吞吐与帧屏障开销。**CI 不做性能门禁**（runner 2 核且噪声大），性能数字人工记录进变更归档；单线程档回归 ≤5%、并行档 ≥2.0x（≥8 逻辑核、N=100k）作为目标而非门禁。

## Risks / Trade-offs

- [常驻线程池的唤醒/关停竞态] → 关停协议显式化（拒绝新作业 → 广播唤醒 → join），单测覆盖"主队列非空时 drop""空闲 lane drop""重复 begin/end""空帧"。
- [panic 导致计数器泄漏 → 死锁] → D7 的 `catch_unwind` + payload 记录 + 完成计数无条件递减；单测含超时保护（panic 用例 MUST 在有限时间返回）。
- [帮助式等待使主线程"帮忙过度"，推迟帧末逻辑] → 帮助范围限定为本地队列 + 可窃取分块，并提供纯阻塞等待；帧屏障前后行为有单测锁定。
- [并行收益被 2b（逐实体迁移清 dirty）吃掉] → 基准分段报告 2a/2b；若 2b 主导，批量清除列为下一变更（D11）。
- [`unsafe impl Sync` 窄契约被后续误用] → 视图 API 面收窄到 `get::<T: Sync>`/`contains`，`# Safety` 写入 rustdoc 与规范；并发读一致性单测。
- [2 核 CI 上并行测试退化/抖动] → 单测不依赖默认预算与时间断言，显式指定 worker 数并只断言正确性/确定性。
- [BREAKING：`transform_propagate_system` 签名] → 保留无参单线程便利构造（等价旧行为），变更说明与 demo 同步更新。
- [线程池数量与硬件不一致导致资源浪费] → 预算计算函数独立可测（`worker_count_for`），默认值与 NeoX 对齐并可由用户覆盖。

## Migration Plan

1. `crates/xengine-core/src/parallel/`：`jobs.rs`（JobConfig/预算计算、作业记录与 slab、工作线程主循环、spawn/spawn_at/spawn_main、优先级、parallel_for、等待语义、panic 安全、帧屏障、JobGroup/JobChain、可观测）、`threads.rs`（LaneSpec/ThreadPool/spawn_long/acquire/OwnedThread）、`sync.rs`（JobConditional）；`lib.rs` 导出
2. `world.rs`：`World::read_view()` + `WorldReadView`（唯一 `unsafe impl Sync` + `# Safety`）
3. `go/global_transform.rs`：`compute_world` 去堆分配（scratch + 深度上限）→ 阶段 2a 并行计算（`WorldReadView` + 预分配缓冲）→ 阶段 2b 串行写回；`transform_propagate_system(jobs)` 接入
4. `frame.rs`：`Engine::with_jobs` / `jobs()` + tick 内 `begin_frame`/`end_frame`
5. `crates/xengine/src/main.rs` demo 切换到并行传播；`benches/propagate.rs` 新增
6. `cargo test` 全绿 → `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check` → `openspec change validate` → `openspec archive` → 分支 `feat/parallel-jobs-core-parallel-jobs` + MR（关联已归档变更，等 CI 绿灯）

## Open Questions

- `SpecificThread` 执行位置与 `ThreadPool` lane 的对接形态（下一变更：`spawn_at(Category::Lane(lane))`？）
- 是否引入 work-stealing（首版共享队列 + 原子游标；由 `benches` 数据决定是否升级）
- 帧屏障是否需要在 FixedUpdate 每次固定步后额外同步（当前仅在帧末；若后续 FixedUpdate 内引入并行作业，需按固定步同步）
- 主线程帮助式等待的默认开关（当前始终帮助；若某些项目要求帧末严格顺序，可增配置）
