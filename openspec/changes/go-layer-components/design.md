## Context

规划序：ECS/帧调度（已归档）→ 数学基元 `xengine-math`（core-math-primitives，实现中）→ **GO 层**（本变更）→ 渲染层/设备层。GO 层是渲染采集与脚本层的共同数据源；性能形态由 spike 基准确认（`benches/go_access.rs`：完整查询链 12.9 ns/GO、持久裸指针 0.87 ns/GO、1% 结构变化 21.7 ns/GO → 结论：包装层采用**位置缓存 + 世代校验**，不用裸指针）。

## Goals / Non-Goals

**Goals:**
- `Component` 生命周期钩子（core-ecs 扩展，type-erased fn ptr，迁移路径不触发）
- GO = Entity 别名 + 三组件套餐（Transform/WorldRef/Parent+Children）+ `create_go`
- 层级维护（双向一致/剥离/级联/环检测）+ 变换传播（PostUpdate，先父后子，GlobalTransform 派生缓存）
- 包装层 GoHandle（位置缓存 + 世代校验，O(1) 稳态/失效重解析）契约 + 基准对照
- 单测全覆盖；`cargo test` 全绿

**Non-Goals:**
- 脚本运行时选定/绑定（组件反射、脚本侧 GO 对象构造）——后续独立变更
- 渲染组件（Camera/Light/MeshRenderer）与渲染采集——渲染层变更
- 名称/标签/层（Name/Layer/Active）、场景序列化、Prefab
- 多 World 并行加载/激活（单 World 先行，WorldRef 预留世界标识）
- 传播脏标记/增量优化（首版每帧全量；benchmark 驱动后续）
- 物理/动画/事件系统

## Decisions

### D1 GO 语义与 Scene 概念（用户决策 + 概念修正）
`pub type GameObject = Entity`；无包装结构；`create_go` 生成三件套实体。**概念修正（用户）：游戏对象的容器是 `Scene`（场景），不是 ECS `World`**——`Scene`（`xengine-core::go`）拥有 `World` + `scene_id` + serial 分配器；GO 生命周期/层级/传播/包装层访问全部经 Scene。`Engine`（core-frame）改为持有 `Scene`（`Pin<Box<Scene>>`），tick 与调度语义不变（系统仍 `&mut World`；Engine 内部经 scene 桥接）。**备选**：包装句柄（脚本层便利，后续脚本变更引入；核心不引）、Scene 仅作引用标记（不承载 World——否决：场景即游戏运行时容器）。

### D2 三组件字段（用户决策 + 本变更定稿）
- `Transform { position: Vector3F, rotate: QuatF, scale: Vector3F }`（用户命名 rotate；局部 TRS；数学约定由 core-math 锁定）
- `SceneRef { scene_id: u32, serial: u64, generation: u32 }`（原 WorldRef，**按用户概念修正改名**：Scene 的引用关系——scene_id 全局唯一、serial 场景内稳定键、generation 世代镜像）
- `Parent { parent: Option<Entity> }`、`Children { children: Vec<Entity> }`（与 spike 基准同构）

### D3 Component 钩子（用户决策：scene 必要传入；world 不另传）
- `trait Component: 'static { fn on_add(&mut self, scene: &mut Scene) {} fn on_remove(&mut self, scene: &mut Scene) {} }`（默认空实现）
- **机制**：core ECS 不感知 `Scene` 类型——描述符存 type-erased 双参钩子 `fn(*mut u8, *mut ())`（组件数据指针 + 生命周期上下文指针）；`World::bind_hook_context(ctx)` 注入（单线程）；`Scene::new() -> Pin<Box<Scene>>` 固定地址后绑定自身指针；钩子执行时 `*mut ()` 还原为 `&mut Scene`（unsafe 收敛于 go 层分发函数 + `# Safety` 文档；单线程约束入文档）
- 触发点：`on_add`——add 成功（值入列）后；`on_remove`——remove/destroy/clear 真正删除前（`Column::remove_swap`/`drop_at` 统一入口）；**`move_swap`/`take_bytes`（迁移）不触发**；Commands flush 路径同语义；未绑定上下文时跳过（go 层保证绑定先于任何触发）
- **备选**：事件化（延迟消费 → 组件行可能已迁移，不安全，×）；钩子移出 core 由 Scene 包装（拦不住 Commands flush 路径，×）；带 `&mut World` 参数（用户否决：world 不必要传入，Scene 即上下文）

### D4 GlobalTransform 派生缓存 + dirty 标记驱动（用户决策）
三件套之外的派生组件（传播写入，渲染采集主路径读取，避免每帧重复层级遍历）；非强制挂载。**dirty 驱动**：`TransformDirty`（marker 组件）由 Scene set API 置位；**两阶段传播**——阶段 1（顺序、读层级边）从 dirty 实体沿 Children 标记全部后代为"待重算"（父变动波及子树——即使子 local 未变）；阶段 2 **按 Entity 并行**（固定数量 chunk 拆分，SoA 列按 chunk 切分）：每个待重算实体**独立遍历自身祖先链的 local TRS 累乘** `world(e)=trs(root)·…·trs(parent)·trs(e)`（行向量约定），写入自己的 `GlobalTransform` 并重置自身 dirty——**无先后顺序依赖**（祖先即使同时 dirty 也无需先算完；纯只读父链 + 写自身行），符合 SoA 并行快速计算要求。公共字段保留 pub（直写需显式 mark，文档契约；快照兜底留后续独立变更）。**并行接入点**：单线程首版阶段 2 为串行循环，但接口与 chunk 化并行一致（后续调度层并行化，行为等价：每实体恰一次写入、恰一次重置）。**备选**：先父后子/按子树分块串行传播（用户否决——依赖顺序、无法按实体并行、不符合 SoA）；全量逐帧重算（×）；快照比较兜底（后续）。

### D5 destroy 语义（用户决策：默认级联）
GO 层 `destroy(entity)` **默认级联**销毁整棵子树（深度优先，每实体恰一次——杜绝"树中间删除产生孤立节点"的管理问题）；`detach(entity)` 为显式剥离（实体与子树保留、转根）。ECS 级 `World::destroy` 单实体语义不变（core-ecs 已归档）。**备选**：默认剥离（用户已否决——中间节点删除产生孤立节点管理问题）。

### D6 包装层（性能决策：spike 数据支撑）
`GoHandle { entity, cache: Option<GoLoc { arch, row, gen }> }`；访问 = 校验（generation 比对 + arch/row 新鲜度）→ 命中 O(1) 经 World 内部位置接口取值 / 未命中重解析。禁止持裸指针（bench E 档证明 1% 结构变化即抹平收益且 unsafe）。公开：`scene.go_view(&mut self, handle) -> Result<GoView>`（借用量）。脚本层绑定后续；本变更交付契约与 Rust 侧实现。

### D7 层级维护与传播时点
HierarchyMaintain / TransformPropagate 挂 PostUpdate（游戏逻辑更新后、渲染采集前）；传播范围：无 Parent 的实体为根集合，递归 Children；环检测在维护操作时（设父/重挂）拒绝。层级信息读取均由组件持有（无外部边表：与 ECS 生命周期自动同步，见决策轴②备选 C 否决理由——之前讨论定 Bevy 式 Parent/Children 组件化）。

## Risks / Trade-offs

- [钩子 = type-erased 双参 fn ptr + 上下文指针（unsafe 桥)] → core 不依赖 Scene 类型（分层干净）；unsafe 收敛在 go 层分发函数（`# Safety`）；`Pin<Box<Scene>>` 固定地址 + 单线程约束入文档；钩子子集覆盖测试（add/remove/destroy/clear/迁移/Commands 全路径）
- [Children 含 Vec 在 SoA 列] → 每行一个堆 vec：维护系统访问为主，热路径（传播）只遍历 Children（同列内指针间跳可接受）；首版不优化
- [传播只处理 dirty 子树] → dirty 驱动后仅脏子树重算；并行分块为后续效率变更（行为等价契约保留）
- [直写字段未标记] → 文档契约（set API / mark_transform_dirty）；不做快照兜底；后续如需，独立变更引入
- [GoHandle 世代校验成本] → 每访问 O(1) 数组读 + 比较；基准 A 档（12.9ns）与 C 档（0.87ns）之间可接受；若超标，后续用 world 级"generation 表"虹吸优化
- [detach 与级联并存] → 文档明确；默认级联（用户决策），剥离需显式 detach

## Migration Plan

1. `xengine-core`：新增 `component.rs`（trait + hooks 描述符扩展 + register_component）+ world.rs 触发点与 `bind_hook_context` + 单测（匹配/迁移/无钩子/Commands 场景）
2. `Scene`（go 模块）：拥有 World + scene_id/serial 分配 + `Pin<Box<Scene>>` + 上下文绑定；`Engine` 改为持有 Scene（tick 桥接、调度不变）
3. GO 模块：套餐组件（Transform/SceneRef/Parent/Children）+ create_go + 层级维护 + destroy 级联/detach + 传播（dirty）+ GoHandle；单测伴随
4. 依赖：xengine-core → xengine-math（等 core-math-primitives 实现 MR 合入后再切主分支依赖；期间用路径依赖）
5. `cargo test` 全绿；clippy/fmt；扩展 go_access 基准为 GoHandle 档
6. Openspec validate → archive → 分支 MR（关联本变更）

## Open Questions

- `SceneRef.serial` 类型（u64 草案；序列化/存档实现时需配套分配器持久化）——后续存档变更处理
- `Scene` 与未来渲染层上下文（场景级相机/资源容器）的合并形态——渲染层变更时扩展 Scene（预留字段位）
- GlobalTransform 是否进入三件套（用户定三件套；本变更作派生可选缓存；如需强制挂载，改传播/采集契约即可，影响小）
