## Context

XEngine 目前仅有单包 Hello-world Cargo 工程。已确认的架构约束：核心层 100% Rust（数据导向 ECS）、设备平台层 C++/Objective-C/Rust 混合驱动 D3D12/Metal/Vulkan、每特性分支 + MR、核心函数必有单测、所有技术选择由用户决策且设计需审批后写入与执行。本设计定义首个变更 `core-ecs-foundation` 的两项新能力 `core-ecs` 与 `core-frame` 的实现方式：**分层 workspace + 自研 ECS（运行时注册、Archetype SoA、single/join 查询）+ Unity 式帧调度（FixedUpdate/Update/PostUpdate、限帧/不限帧、拓扑排序 + 冲突检测）**。

## Goals / Non-Goals

**Goals:**
- 建立可落地的分层结构：`crates/xengine-core`（纯 Rust、零外部依赖、零平台依赖）+ `crates/xengine`（bin）
- `core-ecs`：世代句柄、运行时组件注册（type-id 扩展、脚本组件预留）、Archetype SoA 存储、生命周期（create/destroy/add/remove/clear + Commands）、single/join 查询、资源
- `core-frame`：Unity 模型帧循环、限帧/不限帧模式、函数式系统、拓扑排序调度 + 读写冲突检测、RenderSnapshot 接口占位
- 每个核心公开类型/函数有单测；`cargo test` 全绿

**Non-Goals:**
- 设备平台层与图形驱动接入（仅接口）、脚本运行时、并行调度、事件系统、序列化/资产、CI、基准库正式落地

## Decisions

### D1 组件存储：type-erased 列 + 运行时注册表（type-id）
实现：`ComponentRegistry`（`TypeId → ComponentDescriptor{ size, align, drop_fn, scriptable: bool }`）；每 archetype 每组件类型一个字节列缓冲。**备选**：枚举变体表（扩展需改枚举，无法承载脚本组件 → 否决）；编译期静态类型存储（无法运行时注册脚本类型 → 否决）。**理由**：用户要求 typeid 保留扩展性并允许脚本组件；type-erased 是唯一同时满足运行时注册与 SoA 迭代的路径。

### D2 Archetype 组织
实体按组件集分组；add/remove 触发实体在 archetype 间迁移；同类型组件列连续。**备选**：稀疏 hash 表（迭代局部性差 → 否决）。**理由**：缓存友好、join 可走位掩码匹配。

### D3 世代句柄（index + generation）
u32 index 分配（free-list 回收）+ u32 generation（复用递增；溢出停用槽位）。**备选**：裸指针（移动即失效 → 否决）；u64 单值（无法稳定检测复用 → 否决）。

### D4 生命周期语义（用户决策定稿）
- 命名：`create(components) -> Entity`（可空）、`destroy(entity)`（drop → 回收表 → 复用槽位）、`add(entity, components)`（批量）、`remove::<T>(entity)`、`get/get_mut/contains`、`iterate/query(join)`、`clear()`
- 重复 `add` 同一组件 → `Err(InsertAlreadyExists)`（用户 Q1/补充确认，防隐式覆盖）
- `remove` 缺失组件 → no-op 幂等
- stale 句柄（世代不符）访问 → `Err(StaleEntity)`；`destroy` stale → 幂等 Ok
- drop 顺序：按组件注册顺序，每个实体恰好一次；clear 全销毁保留空结构
- Commands：系统边界 flush，入队顺序生效，语义与同步 API 一致

### D5 查询：archetype join 最小子集
`query(A & B & C)`（≤3）与 `iterate::<T>()`（single）；archetype 位掩码匹配 + 列迭代；借用检查（同组件不可变+可变/双可变 → 拒绝）。**备选**：多组件多次遍历（性能差 → 否决）。**理由**：用户决策 join；帧调度确定后多组件遍历是每帧主路径。

### D6 帧模型：Unity 式 + 双模式（用户决策定稿）
`Engine::tick`：每渲染帧执行 FixedUpdate（固定步长累积器，默认 1/60s 可配，0..N 次）→ Update（每帧 1 次，帧 dt）→ PostUpdate（每帧 1 次）。限帧模式：目标帧率对齐（Update dt ≈ 1/target）；不限帧模式：实测 dt + 最大帧时间钳制（防积分爆炸）；切换后 FixedUpdate 恒定。

### D7 调度：自动拓扑排序 + 冲突检测（用户决策 B）
Schedule：系统访问元数据（组件/资源读写集合）→ 系统依赖图（显式 before/after + 冲突边）→ Kahn 拓扑排序；写-写/写-读冲突无显式排序 → 构建期错误（指明系统对）；无冲突按注册序稳定（确定性 tiebreak）。**备选**：固定相位桶顺序（用户否决）。

### D8 系统形式：函数式 + 零外部依赖
首版：`System` trait（`run(&mut World, &mut Cmd) -> Result`+ 访问元数据声明），提供 `system()` 包装 + `macro_rules!` 参数糖；proc-macro derive（自动抽取）留后续变更。**备选**：引入 bevy_ecs/legion（自研 + 零依赖约束 → 否决）；立即引入 proc-macro（零依赖优先 → 延后）。

### D9 Commands
`Commands` 队列（create/add/remove/destroy/资源），系统内排队、系统边界 flush、入队序。**理由**：系统持有借用时不能同步变更，必须延迟；确定性调度需要固定 flush 时点。

### D10 资源与线程
资源：TypeId 键 + type-erased 数据 + drop（与组件存储同机制）。线程：单线程首版（并行调度接口预留，后续变更）。

### D11 RenderSnapshot 占位
核心定义 `RenderSnapshot` 接口/空实现；帧末阶段调用；不依赖任何平台 API；设备层未来消费并扩展。

## Risks / Trade-offs

- [type-erased 列 unsafe 边界] → 不变量：size/align/drop 由注册表唯一来源；`# Safety` 文档；miri 后续纳入；并输出全覆盖的 drop/迁移单测
- [冲突检测误报/漏报] → 访问元数据记录精确到组件/资源；冲突=未排序 W-W/W-R；采用保守（宁可报错）策略 + 单测矩阵
- [拓扑排序歧义] → 稳定注册序 tiebreak；确定性有单测断言
- [世代溢出] → 槽位停用而非 panic；文档说明 2^32 复用上限
- [受限模式 dt 与真实帧率偏差] → dt 用目标值（限帧）/测量值（不限帧），上限钳制；文档说明
- [迁移工程布局] → 一次提交内完成，main 上仅无主分支（依赖分支），bin 与 core 分离后 `cargo run`/`cargo test` 行为不变

## Migration Plan

1. 根 `Cargo.toml` 改为 workspace（members: `crates/xengine-core`, `crates/xengine`）
2. `src/main.rs` 迁至 `crates/xengine/src/main.rs`；保留 `cargo run` 输出
3. `xengine-core` 先建 lib 空壳（engine_name 等基础），`xengine` 依赖 core
4. 随实现逐项加 ECS/Frame 模块；单测伴随每个模块
5. 回滚：分支独立、MR 前不触碰 main；失败即丢弃分支

## Open Questions

- 固定步长默认值：设计采用 1/60s（可配置），待用户确认默认取值
- 脚本运行时选型（Lua/WASM/Ruby…）：后续独立变更
- 基准库（criterion 等）引入：`benches` 占位 + 后续变更（保持零依赖于公共代码）
- proc-macro derive 参数抽取：后续变更引入（若用户偏好更少样板）
