# XEngine — Agent Instructions

XEngine 是一个 Rust 高性能游戏引擎项目，采用 OpenSpec 规范驱动开发，全程由 AI 代理（Agent）执行。

## 开发流程（所有 Agent 必须遵守）

1. 任何功能/缺陷修复**必须先**创建 OpenSpec 变更提案：`openspec proposal create <id>`
2. 按 `openspec/config.yaml` 中的项目上下文与约定编写提案（specs、tasks、Non-goals、验收标准）
3. 实施完成后必须校验：`openspec proposal validate <id>`，并确保 `cargo test` 全绿
4. **硬性规则：变更必须完成归档（`openspec proposal archive <id>`）之后，才允许合入 main 分支。**
   未归档的 `openspec/changes/` 条目不视为完成，main 只接受已归档规范对应的代码。

## 决策与审批（User Approval Gate）

- **所有选择必须由用户决策**：遇到方案取舍、技术选型、范围/优先级选择时，先给出选项与利弊，由用户拍板；Agent 不得自行决定。
- **设计方案必须经用户审批同意后才能写入和执行**：Agent 出具的设计方案（设计文档、实现计划）必须先提交用户审批；未经同意，不得写入任何文件（提案/设计/代码落盘）或开始执行。

## 质量与合入（CI 门禁，用户只做设计/架构决策）

- **用户只做设计决策与架构规划**：用户不亲自审核代码；代码质量验证交由 **CI（GitHub Actions）**自动化（公开仓库标准 runner 免费）
- MR 合入条件 = **CI 全部通过（cargo test 全绿 + clippy/format 检查 + 需含单测）+ OpenSpec 变更已归档**；无需人工代码审查
- 设计/架构审批仍然**前置**（User Approval Gate）；代码层面决策在 CI 门禁下由执行者（AI）完成
- 流程：设计审批 → 实现（分支）→ `cargo test`/`clippy` 本地全绿 → `openspec validate` → `openspec archive` → 推分支 → MR（等待 CI 绿灯 → 合入）

## 架构约束（分层）

- **核心层（Core）**：全部为 **Rust** 交互（ECS/SoA 数据导向、纯 Rust 公开接口）
- **设备平台层（Device/Platform）**：**C++ / Objective-C / Rust 混合调用**，分别对接底层图形驱动：
  - **D3D12**（Windows）— C++
  - **Metal**（Apple）— Objective-C / C++
  - **Vulkan**（跨平台）— C/C++/Rust 绑定
- 核心层不直接依赖任何平台图形 API；平台层通过明确 FFI/绑定桥接（`# Safety`、unsafe 收敛、接口归核心层所有）

## 分支与合入策略

- 每个特性开发使用**独立分支**：`feat/<特性>-<OpenSpec 变更ID>`（另有 `fix/`、`docs/`、`perf/`）
- 以 **MR（Merge Request）** 方式合入 main，MR 必须关联对应 OpenSpec 变更（已归档）
- **禁止直接向 main 推送**；MR 描述注明变更 ID 与影响层（core / device）
- **MR 合入方式固定为 merge commit（no-FF）**：不使用 squash/变基合入；合入后**自动删除原始分支**（仓库 `delete_branch_on_merge=true` 已启用，未来 MR 沿用此约束）

## 测试与 MR 门禁

- **核心函数和组件必须有对应的单元测试**（`#[test]`、模块/单元测试；组件含行为测试）
- **每次 MR 必须保证单元测试全部通过（`cargo test` 全绿）才允许合入**——这是合入 main 的硬性前置条件，与 openspec 归档并列
- 性能敏感代码额外需要 `cargo bench` 基线
- 提交信息采用 conventional commits：`feat/fix/docs/refactor/perf/test/chore`
- 详细规范工作流见技能：`.agents/skills/openspec/`（DSH 自动发现；其他 AI 工具请手动加载该技能）
