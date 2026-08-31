## Why

XEngine 目前只有 Hello-world 骨架。已确认的架构约束（核心层 100% Rust；设备平台层 C++/Objective-C/Rust 混合驱动 D3D12/Metal/Vulkan；每特性分支 + MR；核心函数必须有单元测试）需要一个可落地的核心基础。ECS（实体-组件）与帧更新调度是所有后续能力（渲染、物理、脚本、设备层接入）的根：没有这两者，任何引擎能力都无法启动。本变更建立最小但完整、可测试的核心闭环。

## What Changes

- 从单包 Cargo 工程重构为 **workspace 多 crate**：`crates/xengine-core`（核心层库，纯 Rust、零外部依赖、零平台依赖）、`crates/xengine`（bin 示例入口，保留 `cargo run` 输出）
- 新增能力 **`core-ecs`**：
  - 实体句柄（u32 index + u32 世代，防悬垂，O(1)）
  - **组件类型运行时注册表**（`TypeId → 描述符{size/align/drop/scriptable}`），**允许脚本组件注册**（type-erased payload，脚本运行时选型后续变更）
  - Archetype SoA 组件存储（type-erased 列 + drop 语义），spawn / insert / remove / destroy 生命周期
  - 查询：single 组件迭代 + ≤3 组件交集 join（缓存友好迭代）
  - 资源（Res / ResMut，TypeId 键存储）
- **生命周期契约（本变更同步明确）**：函数清单 `create / destroy / add / remove::<T> / get / get_mut / contains / iterate / query / clear`（名称按规范定稿）+ 语义约定（重复 `add` 报错 `InsertAlreadyExists`；`remove` 缺失组件 no-op 幂等；stale 句柄访问 `Err(StaleEntity)` 而 `destroy` 幂等；drop 按组件注册顺序确定性执行；世代复用防悬垂；**Commands 延迟操作队列**首版实现，flush 于系统边界；clear 销毁全部实体保留空结构）
- 新增能力 **`core-frame`**：
  - Unity 模型帧循环：`FixedUpdate`（固定步长累积器，每帧 0..N 次）/ `Update`（每渲染帧 1 次）/ `PostUpdate`（每渲染帧 1 次）
  - **限帧模式（目标帧率，Update dt≈1/target）与不限帧模式（测量帧时间）双模式，可切换**；FixedUpdate 步长在两种模式下恒定
  - 函数式 System（参数抽取 + 访问元数据）+ 阶段注册
  - **Schedule 自动拓扑排序**（before/after 显式依赖 + 读写冲突自动检测，未排序的写-写/写-读冲突构建期报错）
  - `RenderSnapshot` 渲染快照**接口占位**（核心定义，设备层未来消费；本变更不实现设备层）
- 每个核心公开类型/函数配套单元测试；workspace `cargo test` 全绿

## Capabilities

### New Capabilities
- `core-ecs`: 实体-组件核心基元：世代句柄、运行时组件注册（type-id 扩展，含脚本组件）、Archetype SoA 存储、生命周期、single/join 查询、资源存储
- `core-frame`: 帧更新与系统调度：Unity 式阶段（FixedUpdate/Update/PostUpdate）、限帧/不限帧模式、函数式系统、拓扑排序+冲突检测调度、RenderSnapshot 接口

### Modified Capabilities
- 无（首个变更，`openspec/specs/` 尚无既有规范）

## Non-goals

- 设备平台层及其图形驱动绑定（D3D12 / Metal / Vulkan）——本变更仅定义 `RenderSnapshot` 核心接口
- 脚本运行时（Lua / WASM 等）实现——首版仅开放运行时注册机制与 `scriptable` 标记
- 并行 / 多线程系统调度（单线程首版，接口预留）
- 事件系统、序列化 / 资产管线、调试 / 编辑器 UI
- CI 工作流（后续独立变更 `dev-ci`）
- 性能基准库落地（本变更仅占位 `benches` 约定，稳定基线后续变更）

## Impact

- 仓库结构：根 `Cargo.toml` 变 workspace；新增 `crates/xengine-core`、`crates/xengine`；`src/main.rs` 迁移至 bin crate
- API：项目为全新代码，无既有公开 API 破坏（**BREAKING** 仅指工程布局变更）
- 依赖：保持零外部依赖（std only），不引入第三方 ECS（自研）
- 层 / 后端：本变更为核心层；设备层仅接口占位
- 文档：README、AGENTS.md 结构章节同步

## Acceptance Criteria

- workspace `cargo test` 全部通过（每个核心公开类型/函数均有单测）
- `cargo run` 正常输出引擎名；`cargo tree` 显示 `xengine-core` 仅依赖 std 与项目内 crate
- `core-ecs`：世代防悬垂（旧句柄失效）、运行时注册（含 scriptable 路径）、生命周期 drop 语义、join 集合正确性均有单测
- **生命周期契约**：重复 insert 报错、remove 缺失 no-op、stale 访问 Err / despawn 幂等、Commands 顺序 flush、clear 全销毁——每个语义均有单测断言
- `core-frame`：FixedUpdate 0..N 次语义、Update/PostUpdate 每帧各 1 次、限帧/不限帧切换、冲突检测触发、拓扑排序确定性与顺序稳定均有单测
- 性能预算：spawn/insert/destroy 摊还 O(1)；single/join 每匹配实体 O(1) 且列连续（SoA）；`benches` 目录占位
