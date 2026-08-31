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
- 本仓库不携带机器级 Agent preset（DSH 预设根仅在 `$DSH_HOME/.agent-presets`，不随仓库分发）；Agent 层面的全部约束由上述仓库内文件承载，确保任何环境克隆后规则一致。

## Getting Started

```powershell
# 构建
cargo build

# 运行
cargo run

# 测试
cargo test
```

## License

(TODO: 待确定 — MIT / Apache-2.0 双许可或其它)
