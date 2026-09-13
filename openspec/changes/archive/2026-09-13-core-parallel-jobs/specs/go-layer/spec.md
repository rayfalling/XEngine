## MODIFIED Requirements

### Requirement: 变换传播（dirty 标记驱动，按实体并行）
- OLD: `GlobalTransform { world: Matrix4F }` 为派生缓存组件（非三件套、非必需）。本地变换的写入 MUST 经 Scene set API（`set_go_transform(e, f)` / `set_transform_position/rotation/scale`）或 `mark_transform_dirty(e)`（直写字段后的显式标记），写入后 MUST 置位 `TransformDirty`（marker 组件）。`TransformPropagate`（PostUpdate）MUST 按下述两阶段执行：**阶段 1（顺序、读层级边）**：从全部 dirty 实体出发沿 Children 标记其**全部后代**为"待重算"（父变动 MUST 波及整棵子树——即使子 local 未变），产出待重算实体集；**阶段 2（并行、按实体固定数量 chunk 拆分）**：每个待重算实体 SHALL **独立遍历自身祖先链的 local TRS** 并以行向量约定累乘 `world(e) = trs(e)·…·trs(parent)·trs(root)`（行向量：最右因子最先作用） 写入自身 `GlobalTransform`（不依赖祖先的 GlobalTransform 已更新、无先后顺序约束），随后重置自身 dirty 标记。每个实体只写自己的 GlobalTransform（SoA 列按 chunk 切分，无写冲突）。无 `GlobalTransform` 的实体跳过写入（不报错）。未标记的直写字段变更 MUST 在文档中声明为"需显式 `mark_transform_dirty`"，传播系统不做快照兜底。遍历 MUST 把无 Parent 实体视为根集合。单线程首版（阶段 2 为串行循环，接口与并行接入点一致）；并行按实体 chunk 化接入点为后续调度层，行为 MUST 与单线程一致。
- NEW: `GlobalTransform { world: Matrix4F }` 为派生缓存组件（非三件套、非必需）。本地变换的写入 MUST 经 Scene set API（`set_go_transform(e, f)` / `set_transform_position/rotation/scale`）或 `mark_transform_dirty(e)`（直写字段后的显式标记），写入后 MUST 置位 `TransformDirty`（marker 组件）。`TransformPropagate`（PostUpdate）MUST 按下述两阶段执行：**阶段 1（顺序、读层级边）**：从全部 dirty 实体出发沿 Children 标记其**全部后代**为"待重算"（父变动 MUST 波及整棵子树——即使子 local 未变），产出待重算实体集；**阶段 2 MUST 拆为 2a 并行计算与 2b 串行写回**：**2a（并行计算）**：待重算实体按 `PROPAGATE_CHUNK` 固定数量 chunk 拆分为并行作业，每个待重算实体 SHALL **独立遍历自身祖先链的 local TRS** 并以行向量约定累乘 `world(e) = trs(e)·…·trs(parent)·trs(root)`（行向量：最右因子最先作用；不依赖祖先的 GlobalTransform 已更新、无先后顺序约束），结果 MUST 写入**每实体独立的预分配结果缓冲**；2a MUST 为纯只读阶段（经 core-ecs 只读并行视图读取 `Transform`/`Parent`，MUST NOT 发生任何结构变更、MUST NOT 写 `GlobalTransform`），每 worker MUST 复用 scratch 缓冲以保持热路径零堆分配，祖先链遍历 MUST 有深度上限保护（畸形环 MUST NOT 挂起，超出上限按根终止）；**2b（串行写回）**：按实体将结果写入自身 `GlobalTransform`（无 `GlobalTransform` 的实体跳过写入、不报错）、重置自身 `TransformDirty`（全部结构变更 MUST 只发生在 2b），并按实体顺序触发 `on_recompute` 回调（每实体恰一次）。并行执行由 JobSystem 提供（`transform_propagate_system(jobs: Arc<JobSystem>)`）；`single_threaded` 配置或 worker 数为 0 时 MUST 与原串行实现行为完全一致，且结果 MUST 与 worker 数无关（逐字节一致）。每个实体只写自己的 GlobalTransform（SoA 列按 chunk 切分，无写冲突）。未标记的直写字段变更 MUST 在文档中声明为"需显式 `mark_transform_dirty`"，传播系统不做快照兜底。遍历 MUST 把无 Parent 实体视为根集合。

#### Scenario: 级联重算与重置
- **WHEN** 根 local 变动（set API）→ `TransformDirty` 置位，一帧后读取
- **THEN** 根及全部后代 GlobalTransform 重算正确（根 90° 旋转后子 position (1,0,0) 变为 (0,1,0)）；传播后波及实体 `TransformDirty` 全部重置；未波及实体重算次数为 0

#### Scenario: 子树局部变动
- **WHEN** 仅叶子节点 local 变动
- **THEN** 只重算该叶子（及其子，如有）；根与中间节点不重算

#### Scenario: 祖先链独立计算
- **WHEN** 树中间节点与叶子同时 dirty（阶段 2a 按实体并行计算）
- **THEN** 叶子结果等于 `trs(leaf)·…·trs(root)` 链路累乘——与祖先是否先算无关，逐实体独立成立；每实体恰一次写入、恰一次重置

#### Scenario: 未标记直写
- **WHEN** 直接写 `Transform` 公共字段且未调用标记 API
- **THEN** 该实体当帧不被重算（dirty 未置位）；履行文档契约后下一帧正常

#### Scenario: 无缓存实体
- **WHEN** 实体挂载 Transform 但无 GlobalTransform，且为根
- **THEN** 传播正常完成、无 panic，实体没有 GlobalTransform 组件进入

#### Scenario: 并行结果与串行逐字节一致
- **WHEN** 同一场景与同一 dirty 集合分别在 worker 数 1 / 2 / 4 / 8 与 `single_threaded` 配置下调用传播
- **THEN** 所有实体的 `GlobalTransform.world` 逐字节一致，`TransformDirty` 重置集合一致，`on_recompute` 每实体恰一次

#### Scenario: 阶段 2a 无结构变更
- **WHEN** 传播过程中观察组件布局变化
- **THEN** 2a 阶段不产生 archetype 迁移（无组件增删）；实体行号变化只可能发生在 2b 的 `TransformDirty` 清除期间

#### Scenario: 畸形环不挂起
- **WHEN** 祖先链存在畸形环（绕过层级 API 手工构造）
- **THEN** 传播在深度上限内终止并按根处理，不挂起、不越界、不 panic
