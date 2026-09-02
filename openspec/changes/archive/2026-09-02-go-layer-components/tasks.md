# Tasks

## 1. core-ecs：Component 生命周期钩子扩展

- [x] 1.1 `src/component.rs`：`ComponentHooks`（`Option<fn(*mut u8, *mut ())>` ×2：组件数据指针 + 生命周期上下文指针）；`ComponentRegistry` 描述符 `hooks` 字段
- [x] 1.2 `registry.rs`：`ComponentDescriptor.hooks`（所有构造路径默认 None）；`register_component_meta::<T>`（go 层包装 `register_component::<T: Component>` 使用）
- [x] 1.3 `storage.rs`：删除路径统一入口（`remove_swap`/`drop_at`）在 drop 前调用 `hooks.on_remove`（若有）；`move_swap`/`take_bytes`/`push_copy` 不触发
- [x] 1.4 `world.rs`：`add` 成功后调用 `hooks.on_add`；`World::bind_hook_context(ctx: *mut ())`（单线程文档 + `# Safety`）；未绑定上下文时跳过触发；**Commands flush 路径同样触发**（flush 内 add/remove/destroy 走同一调用点）
- [x] 1.5 单测：触发计数（add→on_add 恰 1、remove→on_remove 恰 1、destroy→每组件恰 1、clear 全触发）；迁移 add/remove 不触发未删组件；Commands 路径一致；未绑定上下文零触发；重复注册保留首次；hooks None 类型零影响

## 2. Scene 容器与 Engine 桥接

- [x] 2.1 `src/go/scene.rs`：`Scene { world: World, scene_id: u32, serial_counter: u64 }`；`Scene::new() -> Pin<Box<Scene>>`（可空 scene_id 自动分配—全局 AtomicU32）；`bind_hook_context` 绑定自身；`scene.world()/world_mut()`；`Scene::scene_id()`
- [x] 2.2 `frame.rs`：`Engine::new(scene: Pin<Box<Scene>>, schedule, mode)`（BREAKING）；tick 经 scene → `schedule.run_stage(scene.world_mut(), …)`；`engine.scene()/scene_mut()`
- [x] 2.3 lib 导出与文档（单线程 + Pin 约束）
- [x] 2.4 单测：Scene 创建绑定 context 后钩子收到正确 scene_id；Engine tick 在既有语义下行为回归（现有 frame 测试适配））

## 3. GO 模块：三组件套餐与 create

- [x] 3.1 `src/go/transform.rs`：`Transform { position: Vector3F, rotate: QuaternionF, scale: Vector3F }` + `Component` + `Default`；`set API`（`set_position/rotation/scale` 标记 dirty 见 5）
- [x] 3.2 `src/go/scene_ref.rs`：`SceneRef { scene_id, serial, generation }` + `Component`；Scene 自动填充（create 分配 serial）
- [x] 3.3 `src/go/hierarchy.rs`：`Parent { parent: Option<Entity> }`、`Children { children: Vec<Entity> }` + `Component`；`HierarchyError`（Cycle/Stale）
- [x] 3.4 `src/go/mod.rs`：`pub type GameObject = Entity`；`Scene::create_go(transform)`（注册三组件 + 生成 SceneRef + Parent None）；lib 导出
- [x] 3.5 单测：create 组件齐全/默认值/serial 单调/SceneRef 匹配

## 4. 层级维护系统

- [x] 4.1 `HierarchyMaintain`（PostUpdate）：set_parent/reparent 双向一致性、孤儿清理、悬挂 Children 清理
- [x] 4.2 环检测：`Err(HierarchyCycle)` 回滚；单测 a→b→a
- [x] 4.3 Scene 层 `destroy` 默认级联（深度优先、每实体恰一次、drop/on_remove 恰一次、无孤立节点）；`detach` 显式剥离（子树保留转根）；ECS `World::destroy` 单实体语义不动
- [x] 4.4 单测：重建父子、destroy 级联（三层树）、detach 剥离、孤儿/悬挂清理

## 5. 变换传播系统（dirty 标记驱动）

- [x] 5.1 `GlobalTransform { world: Matrix4F }` 组件（派生、可选挂载）
- [x] 5.2 `TransformDirty` marker 组件 + Scene set API：`set_go_transform(e, f)` / `set_transform_position/rotation/scale(e, …)` / `mark_transform_dirty(e)`（写入后自动置位）；doc 说明直写 pub 字段需显式 mark
- [x] 5.3 `TransformPropagate`（PostUpdate）两阶段：**阶段 1**（顺序）从 dirty 实体沿 Children 标记全部后代为"待重算"；**阶段 2**（并行接入点，首版串行循环）对待重算实体**逐个独立遍历祖先链 local 累乘** `world(e)=trs(e)·…·trs(parent)·trs(root)`（行向量：`mul(A,B)` 先 A 后 B，最右因子最先作用），写自身 `GlobalTransform`、重置自身 dirty；无 GlobalTransform 实体跳过
- [x] 5.4 单测：父 dirty→整棵子树全部重算+重置；仅叶子 dirty→叶子级重算（根/中间重算计数为 0）；**祖先链独立计算**（中间节点+叶子同时 dirty，叶子结果=全链累乘，与祖先计算顺序无关）；`(pos=(1,0,0)` 挂 `Z90°` 根）矩阵数值断言；未标记直写不重算；无缓存实体跳过；多根隔离
- [x] 5.5 并行接入点：阶段 2 实现拆分为"待重算实体集按固定数量 chunk"的独立函数（单线程顺序调用；后续调度层并行化），文档记录 SoA 列按 chunk 切分的并行语义（每实体只写自身行、无写冲突、行为等价）
- [x] 5.6 系统接入 Schedule（PostUpdate 注册顺序与冲突检测声明：读 Transform/Parent/Children/TransformDirty，写 GlobalTransform）

## 6. 包装层 GoHandle（位置缓存 + 世代校验）

- [x] 6.1 `GoHandle { entity, loc: Option<GoLoc { arch, row, generation }> }`；World 内位置侧 O(1) 槽位访问接口（内部）
- [x] 6.2 访问 API：`Scene::go_view(&mut self, handle) -> Result<GoView>`（校验世代/位置；命中 O(1) 返回三组件引用；失效重解析或 `Err(GoHandleStale)`）；`GoView` 借用语义（无裸指针）
- [x] 6.3 单测：稳定期访问值正确；迁移后重解析；destroy 后 `Err(GoHandleStale)`；重复访问一致性
- [x] 6.4 基准扩展：`benches/go_access.rs` 增加 GoHandle 档（位置缓存校验访问），与既有 A/C/E 档对比记录

## 7. 验证与交付

- [x] 7.1 `cargo test`（xengine-core，含新单测）全绿
- [x] 7.2 `cargo clippy --all-targets -D warnings` + `cargo fmt --check` 通过
- [x] 7.3 `openspec change validate go-layer-components` 通过
- [x] 7.4 归档（已完成：`2026-09-02-go-layer-components` 归档，go-layer 规范创建 + core-ecs 规范更新）
