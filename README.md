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
