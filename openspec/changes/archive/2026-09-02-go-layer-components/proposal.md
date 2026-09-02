## Why

渲染层之前的规划已定序：GO/Component 组织 → 渲染。`xengine-math`（变革 core-math-primitives）提供数学基元后，GO 层是下一个地基——它定义"游戏对象"的组件套与层级组织，是渲染采集（Camera/Light/Mesh）与脚本层的共同数据源。本变更落地：`Component` 生命周期钩子（core-ecs 扩展）、`Transform/WorldRef/Parent/Children` 三组件套餐、层级维护与变换传播系统、以及脚本可识别的 GO 包装层（位置缓存 + 世代校验）契约。性能形态已由 spike 基准确认（`go_access.rs`：每次完整查询 12.9 ns/GO，持久指针 0.87 ns/GO，结构变化 1% 时 21.7 ns/GO——故选择缓存+校验而非裸指针）。

## What Changes

- **core-ecs 扩展**（修改既有能力）：
  - 新增 `pub trait Component: 'static`（生命周期钩子 `fn on_add(&mut self, scene: &mut Scene)` / `fn on_remove(&mut self, scene: &mut Scene)`，默认空实现）——**场景上下文必要传入**；不单独传 World（经 `scene.world_mut()` 获取）
  - `ComponentDescriptor` 增加 `hooks`（type-erased `fn(*mut u8, *mut ())` 双参：组件指针 + 场景上下文指针）；新增 `register_component::<T: Component>()` 注册路径；现有 `register`/自动注册路径不受影响（hooks=None）
  - 钩子触发点在 core `World` 内部（覆盖同步 API 与 Commands flush 路径）：`on_add` 在组件成功加入实体后调用；`on_remove` 在组件被 remove/destroy/clear 真正删除前调用；**archetype 迁移（move/take）不得触发**；上下文指针由 `World::bind_hook_context`（单线程，配合 `Pin<Box<Scene>>` 固定地址）注入
- **Scene 概念与 GO 容器（新能力 `go-layer`）**：
  - **`Scene` = 游戏对象的运行时容器**（`xengine-core::go`）：`Scene { world: World, scene_id: u32, serial_allocator, … }`；场景拥有 ECS World；`Scene::new() -> Pin<Box<Scene>>`（固定地址供钩子上下文引用）；**GO 全部生命周期/访问 API 经 Scene**（`create_go` / `destroy` / `detach` / `add_component` / `remove_component` / `go_view` / 层级 / 传播挂钩）
  - `Engine`（core-frame）改为持有 Scene：`Engine::new(scene: Pin<Box<Scene>>, schedule, mode)`；tick 行为与既有调度语义不变（系统仍以 `&mut World` 运行，Engine 内部经 scene 桥接；钩子在 flush/帧边界照常触发）
  - GO = `Entity` 别名；`Scene::create_go` 生成三组件套餐
  - `Transform` 组件：`position: Vector3F`、`rotate: QuaternionF`、`scale: Vector3F`（local TRS，依赖 `xengine_math`）
  - `SceneRef` 组件（原 WorldRef，概念按 Scene 修正）：`scene_id: u32`、`serial: u64`（场景内单调序号，稳定跨场景/序列化引用）、`generation: u32`（世代镜像）
  - `Parent { parent: Option<Entity> }` + `Children { children: Vec<Entity> }`：层级边组件；`HierarchyMaintain`（PostUpdate）维持双向一致与孤儿/悬挂清理；**`destroy`（Scene 层）默认级联销毁整棵子树**、`detach` 显式剥离保留为根（ECS 级 `World::destroy` 单实体语义不变）
  - `GlobalTransform { world: Matrix4F }`：**派生缓存组件（非三件套）+ dirty 标记驱动**（`TransformDirty` marker；set API 置位；`TransformPropagate` 两阶段：脏闭包标记 → **按实体并行**（chunk 拆分、每实体独立遍历祖先链 local 累乘）重算并重置标记；无顺序依赖，符合 SoA 并行）
  - **包装层 `GoHandle`**（脚本识别 GO 契约）：`Entity + 位置缓存（archetype/row/世代）+ 世代校验`；O(1) 新鲜路径 / 失效重解析；`scene.go_view()` 返回借用视图；**脚本运行时绑定为非目标**

## Capabilities

### New Capabilities
- `go-layer`: 游戏对象层——Component 生命周期钩子接入、GO 三组件套（Transform/WorldRef/Parent-Children）、层级维护与变换传播、脚本 GO 包装层（位置缓存+世代校验）契约

### Modified Capabilities
- `core-ecs`: 组件注册描述符增加生命周期钩子（`Component` trait + `register_component` + 删除前 on_remove / 加入后 on_add；上下文指针注入；迁移路径不触发）
- `core-frame`: `Engine` 持有 `Scene`（`Pin<Box<Scene>>`）替代裸 World；tick/调度语义不变（系统仍 `&mut World`）

## Impact

- 仓库：`crates/xengine-math`（依赖）、`xengine-core`（Component/钩子 + go 模块：Scene/GO/层级/传播/GoHandle）、`xengine` bin 示例
- API：`xengine-core` 公开新增 `Component`/`register_component`/`Scene`/GO 模块；**`Engine::new` 签名变更（BREAKING：World → Scene）**；ECS World 既有 API 不变
- 依赖：xengine-core 新增 `xengine-math`（路径依赖，均属核心层零平台依赖）；仍无外部依赖
- 层 / 后端：核心层（100% Rust）；设备层后续消费 `GlobalTransform` + 渲染组件（渲染层变更）
- 性能预算：GoHandle 稳态访问 O(1)（单测锁定，基准对照 go_access 档位）；TransformPropagate 仅 dirty 子树重算；钩子仅在生命周期操作点触发（每组件每操作 O(1)）

## Acceptance Criteria

- `cargo test` 全绿（含钩子生命周期单测：on_add/on_remove 触发与不触发路径、迁移不触发、destroy 级联/剥离语义）
- GO 套餐：`create_go` 后实体含 Transform/WorldRef/Parent/Children；重复 create 无组件错误
- 层级维护双向一致性；**destroy 级联（默认）**与 `detach`（显式剥离）语义单测；环检测单测
- 传播正确性：dirty 标记→父 dirty 级联重算整棵子树→重置；未脏实体不重算（行为级断言）；父 move/rotate/scale 后子树 world 矩阵级联正确；无 GlobalTransform 实体不 panic
- GoHandle：新鲜路径数据正确（含实体被迁移后通过校验重解析）；stale/destroy 后访问返回错误；性能档位基准（go_access 延展）记录
- 新增核心公开函数/组件均有单测；`cargo clippy`/`fmt` 通过
