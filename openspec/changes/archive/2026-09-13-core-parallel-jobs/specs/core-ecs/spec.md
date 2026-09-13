## ADDED Requirements

### Requirement: 只读并行视图（WorldReadView）
`World` SHALL 提供只读并行视图 `World::read_view(&self) -> WorldReadView<'_>`，供并行作业在多线程间**共享读取**组件列。`WorldReadView` MUST 只暴露收窄的只读 API（`get::<T: Sync + 'static>(entity) -> Option<&T>` 与 `contains::<T: 'static>(entity) -> bool`），MUST NOT 暴露资源访问、生命周期上下文指针、命令队列、任何 `&mut` 访问或结构变更入口（`add`/`remove`/`create`/`destroy`/`clear`/`flush_commands`）。`WorldReadView` MAY 通过 `unsafe impl Sync` 实现跨线程共享，其 `# Safety` 契约 MUST 在文档中声明：(a) 视图存活期间对 `World` 只发生组件列的共享读取，绝无结构变更或可变访问；(b) 视图不跨越并行区域存活；(c) 视图只允许取出满足 `T: Sync` 的组件引用。并行调用方 MUST NOT 在持有视图期间通过其它路径取得 `&mut World`。

#### Scenario: 并发读取结果一致
- **WHEN** 两个及以上线程持有同一 `WorldReadView` 并并发 `get::<T>()` 同一批实体
- **THEN** 每个线程读到的组件值与单线程读取完全一致，无数据竞争、无未定义行为

#### Scenario: 类型约束由编译器强制
- **WHEN** 尝试通过视图取出 `T: !Sync` 的组件引用，或尝试调用 `add`/`remove`/资源访问
- **THEN** 编译错误（视图不提供这些 API，且 `get` 的 `T: Sync` 约束生效）

#### Scenario: 失效实体与不存在组件
- **WHEN** 对已销毁实体或未挂载该组件的实体调用 `view.get::<T>(e)`
- **THEN** 返回 `None`（不 panic、不越界）

#### Scenario: 视图不改变既有语义
- **WHEN** 通过视图读取后继续使用 `World` 的既有 API（读写、结构变更）
- **THEN** 行为与存在视图之前完全一致（视图为纯只读旁路，不引入额外状态）
