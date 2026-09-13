# Tasks

## 1. core-jobs：JobSystem 底座

- [x] 1.1 `src/parallel/mod.rs` + `jobs.rs` 骨架：`JobConfig`（`hint/reserve/min/max` + `Default`）、`worker_count_for(config)` 预算计算（`clamp(f(hint) - reserve, min, max)`，下限 1）、`JobSystem::new` / `single_threaded` / `worker_count`；单测：默认预算（8 核→6、2 核→1）、显式 `hint=4.0`→4、单线程模式不启动线程
- [x] 1.2 作业记录与 slab：作业槽（世代、状态机、`Arc<JobsInner>` 句柄）、`JobHandle`（`is_finished` / `wait` / `wait_blocking`）、活跃计数；单测：句柄有效期、帧末回收后世代不串号
- [x] 1.3 工作线程主循环与关停协议：共享任务队列（优先级 + FIFO）、`Condvar` 唤醒、`Drop` 拒绝新作业 → 广播唤醒 → join；单测：空帧、重复 `begin_frame`/`end_frame`、关停时主队列非空、空闲关停（无死锁，带超时保护）
- [x] 1.4 提交路径：`spawn` / `spawn_at(JobCategory)` / `spawn_main`（主线程线程本地队列，`!Send` 闭包）；单测：主线程作业按提交序、非 owner 线程 `spawn_main` panic、`Current` 内联执行且句柄已完成、`Rc<RefCell<..>>` 载荷可编译
- [x] 1.5 优先级：`JobPriority{High, Normal, Low}` 非抢占调度；单测：单 worker 下 High 先于后续 Low 取用、同优先级 FIFO（提交序日志）
- [x] 1.6 分块并行：`parallel_for(range, grain_size, f)`（fork-join）与 `spawn_parallel_for`（fork-only）；`grain_size == 0` 取默认粒度、空范围立即返回、块数为 `ceil(len/grain)`、每块恰一次；单测：结果与串行逐元素一致、fork-only 与其它作业重叠不污染
- [x] 1.7 等待语义：`wait`（帮助式：泵主线程队列 + 消费可窃取分块）、`wait_blocking`（`Mutex + Condvar`）、**自我等待检测**（明确 panic）+ 递归帮助深度上限；单测：帮助式等待期间本地队列被消费（带看门狗）、自我等待 panic、`active_job_count` 前后变化
- [x] 1.8 panic 安全：`catch_unwind(AssertUnwindSafe)` + payload 记录 + 完成计数无条件递减 + 等待线程 `resume_unwind`；分块 panic 记首个 payload、其余块收尾后于调用线程抛出；单测：作业 panic 传播且工作线程存活、`parallel_for` 块 panic 不挂起、panic 后系统仍可用
- [x] 1.9 帧语义：`begin_frame(frame)` / `end_frame() -> FrameStats{frame, jobs, completed, duration}`（帧屏障：帧内作业全部完成 + 主队列泵空 + 帧内槽位回收）；单测：帧屏障返回即完成、跨帧无残留与无重复统计
- [x] 1.10 组合原语：`JobGroup`（≤4 内联零分配 + 组等待）、`JobChain`（`then` / `then_wait` / `tail`）、`sync.rs::JobConditional`（`wait` / `notify_all` / `reset`，先唤醒后等待立即返回）；单测覆盖各组
- [x] 1.11 可观测：`active_job_count()`、`current_job()`（作业内 `Some`、非作业 `None`）、`FrameStats`；单测：计数在提交/完成前后变化、`current_job` 语义
- [x] 1.12 受控作用域并行（**实施中追加批准**）：`parallel_for_collect` 的 lifetime 擦除（唯一一处 `transmute`）+ `# Safety` 论证；擦除后 `Send + 'static` 载荷合法性、panic 时先 join 全部块再 `resume_unwind`（避免 ctx 提前释放）；单测：借用闭包可并行、块 panic 不挂起、worker 数 1/2/4 与单线程结果一致

## 2. core-jobs：ThreadPool（长任务专用线程）

- [x] 2.1 `src/parallel/threads.rs`：`LaneSpec { name, threads }`、`ThreadPool::new(&[LaneSpec])`、lane 专属线程与命令队列、`lane_threads(lane)` / `lane_count()` / `lane_id(name)` / `lane_id_at(index)` / `lane_name(lane)`；单测：线程数与 lane 数、按名查找
- [x] 2.2 `spawn_long(lane, name, f) -> LongTaskHandle`（`is_finished` / `wait` / `name`），长任务可跨帧存活；单测：投递后由 lane 专用线程执行（线程 id 断言）、句柄等待返回
- [x] 2.3 `acquire(lane) -> OwnedThread` RAII 借还：独占使用、drop 后线程归还并可再次借出；`OwnedThread::run` 在 lane 线程执行并取回结果、panic 可复现；单测：容量 1 时第二次 `acquire` 阻塞至 guard drop、`run` 返回值与线程 id
- [x] 2.4 预算扣除与互不干扰：`JobConfig::reserve` 从 worker 预算划出；lane 线程不参与帧任务；单测：lane 满载阻塞型长任务时帧内 `parallel_for` 仍按时完成
- [x] 2.5 长任务 panic 与关停：panic 捕获 + `wait()` 复现、`Drop` 关停并 join、排队任务取消；单测：长任务 panic 后线程池与 JobSystem 仍可用、空闲 drop 与满载 drop 均无死锁

## 3. core-ecs：只读并行视图

- [x] 3.1 `world.rs`：`World::read_view()` + `WorldReadView`（`get::<T: Sync + 'static>` / `contains` / `contains_entity`；唯一 `unsafe impl Sync` + `# Safety` 文档）；单测：视图读取值正确、失效实体与缺失组件返回 `None`、视图使用后既有 API 行为不变
- [x] 3.2 并发读一致性单测：`std::thread::scope` 多线程共享视图并发读取同一批实体与单线程结果一致

## 4. go-layer：传播阶段 2 并行化

- [x] 4.1 `compute_world` 去堆分配：线程本地 scratch 复用 + `MAX_HIERARCHY_DEPTH = 16_384` 深度上限替代每实体 `HashSet`；单测：畸形环在有限深度内终止不挂起、既有传播数值断言全部保持
- [x] 4.2 阶段 2a 并行计算：待重算集合按 `PROPAGATE_CHUNK` 分块提交 `parallel_for`，逐实体独立走祖先链，结果写入预分配缓冲（线程本地结果缓冲 + 短锁 memcpy 回写），经 `WorldReadView` 只读访问；单测：缓冲结果与串行逐元素一致
- [x] 4.3 阶段 2b 串行写回：写 `GlobalTransform`（无该组件跳过）、清 `TransformDirty`、按实体顺序触发 `on_recompute` 恰一次；单测：回调次数与顺序确定、dirty 重置集合一致
- [x] 4.4 确定性与等价回归：worker 数 {1,2,4} 与串行结果逐元素一致；既有 `global_transform.rs` 单测全绿
- [x] 4.5 `transform_propagate_system(jobs: Arc<JobSystem>)` 接入 + `transform_propagate_system_single_threaded()` 等价路径 + `propagate_with_jobs(world, jobs)` 公开入口 + 既有调用点适配
- [x] 4.6 分阶段计时钩子：`PropagateTiming { entities, scan, compute, apply, parallel }` + `last_propagate_timing()`（基准分段数据来源）

## 5. core-frame：Engine 集成

- [x] 5.1 `frame.rs`：`Engine::with_jobs(Arc<JobSystem>)` / `jobs()`；tick 内 `begin_frame`（帧开始）与 `end_frame`（PostUpdate 之后、`snapshot()` 之前）；单测：未注入时单线程等价（`RunStats` 一致）、tick 返回即帧内作业完成
- [x] 5.2 帧确定性回归：相同 dt 序列 × 10 帧的系统调用日志在注入多 worker 与不注入时一致
- [x] 5.3 `crates/xengine/src/main.rs` demo 切换到并行传播（注入 JobSystem、打印分段耗时）

## 6. 基准与交付验证

- [x] 6.1 `benches/propagate.rs`：宽树/深链/平坦 × N=10k/100k，scan/2a/2b 分段耗时 + 串行/并行加速比 + 逐元素一致性校验；已注册到 `Cargo.toml`
- [x] 6.2 记录基线数字（写入 `README.md`）：阶段 2a 并行收益 4.6x（宽树）/8.2x（深链）；总加速深链 6.2x、宽树 1.09x（无回退）、平坦噪声内；瓶颈定位为串行 2b（archetype 迁移），列为后续变更
- [x] 6.3 `cargo test --workspace` 全绿（161 项：core 117 + 集成 4 + math 40）
- [x] 6.4 `cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo fmt --all -- --check` 通过
- [x] 6.5 `openspec validate core-parallel-jobs` 通过
- [x] 6.6 归档 `openspec archive core-parallel-jobs`（合入 main 的前置条件）；README 项目结构段落已更新

> 交付：分支 `feat/parallel-jobs-core-parallel-jobs` 已提交 → MR（关联本已归档变更，CI 全绿后 auto-merge）。
