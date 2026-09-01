## 1. Workspace 重构

- [x] 1.1 根 `Cargo.toml` 改造为 workspace（members: `crates/xengine-core`、`crates/xengine`；resolver 2；edition 2024），同步 workspace 元数据（name 保持 xengine）
- [x] 1.2 创建 `crates/xengine-core`（lib）与 `crates/xengine`（bin）骨架；`src/main.rs` 迁移至 `crates/xengine/src/main.rs`，`engine_name()` 随迁并保留单测；`cargo run`/`cargo test` 验证全绿
- [x] 1.3 `cargo tree` 验证 `xengine-core` 仅依赖 std 与项目内 crate（零外部依赖）；README/AGENTS 结构章节同步（docs 提交）

## 2. core-ecs：实体句柄与注册表

- [x] 2.1 实现 `Entity{index,generation}`、`EntityAllocator`（free-list + generation 递增 + 溢出停用）、`StaleEntity` 错误；配套单测（复用递增、停用、O(1) 断言桩）
- [x] 2.2 实现 `ComponentDescriptor{size,align,drop_fn,scriptable}` 与 `ComponentRegistry`（TypeId 键、运行时注册、重复注册 Err、drop 函数存储）；配套单测（注册/重复注册/脚本标记）

## 3. core-ecs：存储与 World 生命周期

- [x] 3.1 实现 type-erased 列（字节缓冲 + drop 一次语义 + 对齐/迁移安全，`# Safety` 文档）与 `Archetype`（组件集标识 + 列 SoA）；配套单测（列连续性、drop 计数）
- [x] 3.2 实现 `World`：`create`（可空/带初始组件）、`add`（批量、重复 Err(InsertAlreadyExists)）、`remove`（缺失 no-op）、`destroy`（drop 按注册序、回收表、stale 幂等）、`get/get_mut/contains`（stale → Err）、`iterate`、`clear`（全销毁保留空结构）；全套生命周期单测
- [x] 3.3 实现 archetype 迁移（add/remove 后方实体进出正确、列连续）；配套单测

## 4. core-ecs：查询与资源

- [x] 4.1 实现 single `iterate::<T>()` 与 join `query(A & B & C)`（≤3，archetype 位掩码匹配 + 列迭代，每实体 O(1)）；配套单测（结果集正确、移除后消失）
- [x] 4.2 实现资源存储（TypeId 键、type-erased + drop）与 `Res/ResMut` 访问（未注册 → Err）；配套单测
- [x] 4.3 实现借用检查元数据（组件/资源读-写集合；可变+不可变/双可变拒绝）；配套单测
- [x] 4.4 实现 `Commands`（create/add/remove/destroy/资源变更，入队序 flush 于系统边界，语义与同步 API 一致）；配套单测（顺序、清空）

## 5. core-frame：帧循环与模式

- [x] 5.1 实现 `Engine`/`Engine::tick` 帧循环（FixedUpdate 累积器 0..N 次、Update/PostUpdate 每帧 1 次，固定步长默认 1/60s 可配）；配套单测（慢帧 5 次、快帧 0 次、顺序断言）
- [x] 5.2 实现限帧（目标帧率对齐 dt≈1/F）与不限帧（实测 dt + 最大帧时间钳制）双模式及切换；配套单测（60fps、实测 12ms、切换后 FixedUpdate 恒定）

## 6. core-frame：系统与调度

- [x] 6.1 实现 `System` trait（函数式包装 + 访问元数据 + run(ctx)）与阶段注册（FixedUpdate/Update/PostUpdate）；配套单测（按阶段调用序）
- [x] 6.2 实现 `Schedule`：显式 before/after 依赖图 + 冲突检测（未排序 W-W/W-R 构建期报错并指明系统对）+ Kahn 拓扑排序（无冲突按注册序稳定）；配套单测（冲突报错、显式排序消除、确定性）
- [x] 6.3 系统接入 engine 执行（tick 驱动阶段、系统边界 flush Commands）；集成测试（tick 全链路 + 确定性日志断言）

## 7. core-frame：RenderSnapshot 接口占位

- [x] 7.1 定义 `RenderSnapshot` 接口/空实现，帧末阶段产出；`cargo tree` 证明无平台依赖；配套单测（接口可用、空数据产出）

## 8. 收尾与回归

- [x] 8.1 `benches` 占位（100k 实体 create + single 迭代，cargo bench 可运行）
- [x] 8.2 文档同步（README 结构说明、模块 rustdoc 示例）；workspace `cargo test` 全绿 + `cargo run` 正常
- [x] 8.3 `openspec validate core-ecs-foundation` 通过；按流程归档（`openspec archive`）后创建 MR（feat/core-ecs-foundation → main）→ https://github.com/rayfalling/XEngine/pull/1
