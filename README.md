# XEngine

**XEngine** — 一个基于 Rust 的高性能游戏引擎（目标：极低开销、数据导向设计的现代游戏引擎）。

A high-performance game engine written in Rust, designed with a data-oriented, zero-cost-abstraction philosophy.

## Status

🚧 早期开发阶段 — 项目结构正在通过 OpenSpec 规范驱动流程逐步建立。

## Development Workflow（纯 AI 工作流）

本项目采用 **OpenSpec** 规范驱动的开发流程，全程由 AI 代理协作完成：

- **规范仓库**：`openspec/specs/` — 长期有效的系统规范
- **变更提案**：`openspec/changes/` — 每次功能开发都从提案开始
  - `openspec proposal create <名称>` — 提出变更
  - `openspec proposal list / show <ID>` — 查看变更状态
  - `openspec proposal apply <ID>` — 实施变更
  - `openspec proposal validate [<ID>]` — 校验规范与实现一致性
  - `openspec proposal archive <ID>` — 变更合并后归档到 specs/

每个 AI 代理在开发前必须先阅读 `openspec/config.yaml` 与相关 spec，确保实现与规范一致。

## Agent 约束（硬性规则）

- **AGENTS.md**（仓库根）— 所有 AI 代理自动加载的约束：任何变更必须走 OpenSpec 提案流程，且**必须完成 `openspec proposal archive <ID>` 归档后，才允许合入 main 分支**；同时约定 Rust 工程规范（edition 2024、数据导向设计、benchmark、conventional commits）。
- **技能**：`.agents/skills/openspec/SKILL.md` — 完整规范工作流（提案 → 实施 → 校验 → 归档 → 合入），DSH 自动发现、无需手动加载；其他 AI 工具可按相同路径手动加载。
- **架构分层**：核心层 100% Rust；设备平台层 C++/Objective-C/Rust 混合调用（D3D12 / Metal / Vulkan 图形驱动），核心层不直接依赖平台图形 API。
- **分支与 MR**：每个特性独立分支（`feat/<特性>-<变更ID>`，另有 `fix/docs/perf`）经 MR 合入；禁止直接推送 main；MR 关联已归档的 OpenSpec 变更。
- **测试与门禁**：核心函数与组件必须有对应的单元测试；**每次 MR 必须 `cargo test` 全部通过才允许合入**（与 openspec 归档并列的硬性前置条件）。
- 本仓库不携带机器级 Agent preset（DSH 预设根仅在 `$DSH_HOME/.agent-presets`，不随仓库分发）；Agent 层面的全部约束由上述仓库内文件承载，确保任何环境克隆后规则一致。

## Getting Started

```powershell
# 构建
cargo build

# 运行（引擎名 + 最小 ECS/帧循环 demo）
cargo run

# 测试（核心函数/组件单测 + 集成测试）
cargo test

# 基准（100k 实体 create/iterate/join 占位）
cargo bench -p xengine-core
```

## 项目结构

- `crates/xengine-core/` — **核心层（100% Rust，零外部依赖）**：ECS（实体/组件/Archetype SoA/查询/资源/Commands）、帧调度（FixedUpdate/Update/PostUpdate、限帧/不限帧、拓扑排序+冲突检测）、RenderSnapshot 接口
- `crates/xengine/` — bin 入口与最小 demo
- `openspec/` — OpenSpec 规范驱动工作流（见上）
- `.agents/skills/` — 官方 OpenSpec 工作流技能（自动发现）

## License

(TODO: 待确定 — MIT / Apache-2.0 双许可或其它)
