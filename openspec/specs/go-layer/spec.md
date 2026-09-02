# go-layer Specification

## Purpose
游戏对象层：定义 GO（Entity 别名）及其三组件套（Transform/SceneRef/Parent+Children）、Scene 游戏对象容器（拥有 ECS World，承载组件生命周期钩子上下文）、层级维护与级联 destroy/detach、dirty 标记驱动的全局变换传播（按实体并行接入点），以及脚本识别的 GoHandle（位置缓存+世代校验）契约。渲染层在此之上消费 GlobalTransform 与场景结构。

## Requirements
### Requirement: Scene 容器与 GO 语义
`Scene` SHALL 为 GO 层的游戏对象容器（`xengine-core::go`），拥有 ECS `World`、分配 `scene_id`（全局唯一）与场景内 `serial` 序号；`SceneHandle::new()` SHALL 返回声明为 `SceneHandle`（唯一安全句柄；内部 `Pin<Box<Scene>>` 固定地址供钩子上下文引用，`Scene: !Unpin`，单线程）。`GameObject` SHALL 为 `Entity` 的类型别名（无包装结构，实体即 GO）。GO 的创建/生命周期/层级/传播/包装层访问 MUST 经 `Scene` API 完成（ECS `World` 仍可直接操作，但钩子只保证经 Scene 的运行路径完整触发）。`Scene::create_go` SHALL 生成三组件套餐：`Transform`、`SceneRef`、`Parent`（`Children` 由层级维护系统维护）。

#### Scenario: create_go 组成
- **WHEN** `scene.create_go(transform)`（SceneRef/字段自动生成）
- **THEN** 返回 Entity；该实体 `contains::<Transform/SceneRef/Parent>` 全真；`Parent.parent` 为 None；`SceneRef` 自动填充（见下）

### Requirement: Transform 组件
`Transform` 为实现 `Component` 的组件：MUST 含字段 `position: Vector3F`（`xengine_math`）、`rotate: QuaternionF`、`scale: Vector3F`，表示**局部** TRS（相对父节点）。缩放 MAY 为负数（允许镜像）。`Transform::default()` MUST 为 `position=ZERO`、`rotate=Identity`、`scale=ONE`。本地变换写入 MUST 经 Scene set API（`set_go_transform`/`set_transform_position/rotation/scale`）或显式 `mark_transform_dirty`，写入后置位 `TransformDirty`（见传播要求）。

#### Scenario: 默认值
- **WHEN** `Transform::default()`
- **THEN** `position==ZERO`、`rotate==Identity`、`scale==ONE`

### Requirement: SceneRef 组件
`SceneRef` 为实现 `Component` 的组件：`scene_id: u32`（所属场景标识，全局唯一）、`serial: u64`（场景内单调递增创建序号，用于调试/序列化/跨场景稳定引用）、`generation: u32`（实体世代镜像）。创建 GO 时由 `Scene` 自动填充：MUST 与实体当前世代、所属场景一致。引用关系以 `(scene_id, serial)` 为稳定键（跨场景/存档）。组件不可变（无 setter 以外的直接写位置——通过销毁重建或存档恢复 API 更新）。

#### Scenario: 自动填充
- **WHEN** `scene.create_go` 创建实体 e
- **THEN** `scene_ref.scene_id == scene.scene_id()`、`serial == 上一序号 + 1`（单调）、`generation == e.generation()`

### Requirement: 层级组件与维护
层级边由 `Parent { parent: Option<Entity> }`（child 侧）与 `Children { children: Vec<Entity> }`（parent 侧）构成；`HierarchyMaintain`（PostUpdate）保证双向一致：任何 parent 变更/实体销毁后，MUST 清理孤儿（child 的 parent 指向已销毁实体）与悬挂 Children 条目；重建父关系 MUST 从旧父的 Children 移除、加入新父的 Children。环（祖先环）MUST 被拒绝（`Err(HierarchyCycle)`）。Scene 层 `destroy(entity)` MUST 默认级联销毁整棵子树（深度优先，每实体恰一次销毁、组件 drop 恰一次、钩子 `on_remove` 恰一次，无孤立节点）；`detach(entity)` MUST 显式剥离（实体与子树保留，Parent 置 None 转根，原父 Children 移除）。ECS 级 `World::destroy` 维持单实体语义不变。

#### Scenario: 重建父子
- **WHEN** `e2.parent = Some(e1)`（经维护系统处理）后再次 `e2.parent = Some(e3)`
- **THEN** `e1.Children` 不含 e2，`e3.Children` 含 e2，`e2.Parent` 指向 e3

#### Scenario: destroy 默认级联
- **WHEN** 对含三层子树的根调用 Scene 层 `destroy`
- **THEN** 根与全部后代实体销毁恰一次（无孤立节点、无重复 drop、`on_remove` 每组件恰一次），级联顺序为深度优先

#### Scenario: detach 显式剥离
- **WHEN** 对树中间的实体调用 `detach`
- **THEN** 该实体与其子树保留；实体 `Parent == None` 转为根，原父 `Children` 中移除该实体

#### Scenario: 环检测
- **WHEN** 尝试建立 a→b→a 的父子链
- **THEN** 返回 `Err(HierarchyCycle)`，两侧状态回滚到原先

### Requirement: 变换传播（dirty 标记驱动，按实体并行）
`GlobalTransform { world: Matrix4F }` 为派生缓存组件（非三件套、非必需）。本地变换的写入 MUST 经 Scene set API（`set_go_transform(e, f)` / `set_transform_position/rotation/scale`）或 `mark_transform_dirty(e)`（直写字段后的显式标记），写入后 MUST 置位 `TransformDirty`（marker 组件）。`TransformPropagate`（PostUpdate）MUST 按下述两阶段执行：**阶段 1（顺序、读层级边）**：从全部 dirty 实体出发沿 Children 标记其**全部后代**为"待重算"（父变动 MUST 波及整棵子树——即使子 local 未变），产出待重算实体集；**阶段 2（并行、按实体固定数量 chunk 拆分）**：每个待重算实体 SHALL **独立遍历自身祖先链的 local TRS** 并以行向量约定累乘 `world(e) = trs(e)·…·trs(parent)·trs(root)`（行向量：最右因子最先作用） 写入自身 `GlobalTransform`（不依赖祖先的 GlobalTransform 已更新、无先后顺序约束），随后重置自身 dirty 标记。每个实体只写自己的 GlobalTransform（SoA 列按 chunk 切分，无写冲突）。无 `GlobalTransform` 的实体跳过写入（不报错）。未标记的直写字段变更 MUST 在文档中声明为"需显式 `mark_transform_dirty`"，传播系统不做快照兜底。遍历 MUST 把无 Parent 实体视为根集合。单线程首版（阶段 2 为串行循环，接口与并行接入点一致）；并行按实体 chunk 化接入点为后续调度层，行为 MUST 与单线程一致。

#### Scenario: 级联重算与重置
- **WHEN** 根 local 变动（set API）→ `TransformDirty` 置位，一帧后读取
- **THEN** 根及全部后代 GlobalTransform 重算正确（根 90° 旋转后子 position (1,0,0) 变为 (0,1,0)）；传播后波及实体 `TransformDirty` 全部重置；未波及实体重算次数为 0

#### Scenario: 子树局部变动
- **WHEN** 仅叶子节点 local 变动
- **THEN** 只重算该叶子（及其子，如有）；根与中间节点不重算

#### Scenario: 祖先链独立计算
- **WHEN** 树中间节点与叶子同时 dirty（阶段 2 按实体并行计算）
- **THEN** 叶子结果等于 `trs(leaf)·…·trs(root)` 链路累乘——与祖先是否先算无关，逐实体独立成立；每实体恰一次写入、恰一次重置

#### Scenario: 未标记直写
- **WHEN** 直接写 `Transform` 公共字段且未调用标记 API
- **THEN** 该实体当帧不被重算（dirty 未置位）；履行文档契约后下一帧正常

#### Scenario: 无缓存实体
- **WHEN** 实体挂载 Transform 但无 GlobalTransform，且为根
- **THEN** 传播正常完成、无 panic，实体没有 GlobalTransform 组件进入

### Requirement: 组件生命周期钩子（Scene 上下文）
实现 `Component` 的组件 MAY 定义 `fn on_add(&mut self, scene: &mut Scene)` 与 `fn on_remove(&mut self, scene: &mut Scene)`（默认空实现；不单独传 World——经 `scene.world_mut()` 获取）。钩子由 ECS 层经 type-erased 双参函数指针在正确时点触发（见 core-ecs：加入后 on_add、删除前 on_remove、迁移不触发、Commands 路径一致、上下文=绑定指针）。`SceneHandle::new()` SHALL 在内部经（crate 级的）`bind_hook_context` 将自身地址注入（外部不可再绑定），钩子执行时 `&mut Scene` 有效（`Scene: !Unpin` + `SceneHandle` 唯一构造保证地址稳定；单线程约束入文档）。

#### Scenario: 钩子拿到场景
- **WHEN** 组件 add 成功或 remove 前（组件实现 on_add/on_remove，记录 scene_id）
- **THEN** 钩子收到的 `scene.scene_id()` 等于所属场景 id，且钩子内对 `scene.world_mut()` 的访问合法（组件数据指针与场景上下文在调用期间稳定）

### Requirement: 包装层 GoHandle（脚本识别 GO 契约）
引擎 MUST 提供 `GoHandle`（Rust 侧）作为脚本层识别 GO 的句柄：包含 `entity` 与**位置缓存**（archetype id、row、世代镜像），并提供带**世代校验**的访问 API：校验通过（未销毁、未迁移）时 O(1) 直取组件引用（`GoView` 借用 Scene，经 scene.world 校验后返回引用）；校验失败（迁移/stale）时自动重解析或返回 `Err(GoHandleStale)`。GoHandle 不得缓存任何裸指针（禁止跨帧持指针）；所有访问都经 Scene/World 校验后以引用返回。脚本运行时绑定（UUID/类型注册/反射）为**非目标**（后续变更）。

#### Scenario: 稳定期 O(1) 访问
- **WHEN** 10000 个 GO 的 GoHandle 在组件集稳定下逐次访问 Transform
- **THEN** 每访问校验+取值 O(1)（基准对照 `go_access.rs` 持久指针档 0.87 ns/GO 级；完整查询档 12.9 ns/GO 为上限参照，二者之间），值正确

#### Scenario: 失效重解析
- **WHEN** 实体被 add/remove 组件迁移后被 GoHandle 访问
- **THEN** 世代/位置校验失败 → 重解析（或显式 `Err` 由调用方 refresh），访问返回迁移后的正确数据

#### Scenario: 销毁后访问
- **WHEN** 实体被 destroy 后 GoHandle 访问
- **THEN** 校验失败返回 `Err(GoHandleStale)`（不 panic、不 UB）

