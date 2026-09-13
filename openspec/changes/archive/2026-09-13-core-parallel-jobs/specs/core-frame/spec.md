## ADDED Requirements

### Requirement: Engine 的 JobSystem 集成与帧屏障
`Engine` SHALL 支持注入并行底座：`Engine::with_jobs(jobs: Arc<JobSystem>)` 与只读访问 `Engine::jobs() -> Option<&Arc<JobSystem>>`；未注入时 `Engine` MUST 在单线程语义下工作（等价于 `JobSystem::single_threaded()`，不得 panic）。`Engine::tick` MUST 在帧开始时调用 `begin_frame(帧号)`、在帧末（PostUpdate 之后、`snapshot()` 之前）调用 `end_frame()` 建立**帧屏障**：`tick` 返回时该帧提交的全部并行作业 MUST 已完成。集成 MUST NOT 改变既有帧阶段语义（FixedUpdate 0..N 次 → Update 1 次 → PostUpdate 1 次）、帧率模式语义（限帧/不限帧与钳制）与**帧确定性**（同一输入序列下系统调用顺序完全一致）。

#### Scenario: 未注入 JobSystem 的等价单线程行为
- **WHEN** 使用 `Engine::new(...)`（未注入 `JobSystem`）驱动若干帧
- **THEN** 帧阶段次数与顺序、`RunStats`、`snapshot()` 结果与注入单线程 `JobSystem` 时完全一致；不 panic

#### Scenario: tick 返回即帧内作业完成
- **WHEN** 某个 Update/PostUpdate 系统提交并行作业（例如并行传播）后 `tick` 返回
- **THEN** 该帧作业全部完成（帧屏障生效），不存在跨帧存活的本帧作业

#### Scenario: 帧确定性不因并行改变
- **WHEN** 用相同固定 dt 序列驱动 10 帧并记录系统调用日志（注入多 worker JobSystem）
- **THEN** 两次运行日志完全一致，且与注入单线程 JobSystem 时的日志一致

#### Scenario: 并行传播在帧内的位置
- **WHEN** `transform_propagate_system(jobs)` 注册到 PostUpdate 并由 `Engine::tick` 驱动
- **THEN** 传播在 PostUpdate 阶段内完成（渲染快照 `snapshot()` 之前），dirty 标记在该帧内被清除
