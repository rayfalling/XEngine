# XEngine — Agent Instructions

XEngine 是一个 Rust 高性能游戏引擎项目，采用 OpenSpec 规范驱动开发，全程由 AI 代理（Agent）执行。

## 开发流程（所有 Agent 必须遵守）

1. 任何功能/缺陷修复**必须先**创建 OpenSpec 变更提案：`openspec proposal create <id>`
2. 按 `openspec/config.yaml` 中的项目上下文与约定编写提案（specs、tasks、Non-goals、验收标准）
3. 实施完成后必须校验：`openspec proposal validate <id>`，并确保 `cargo test` 全绿
4. **硬性规则：变更必须完成归档（`openspec proposal archive <id>`）之后，才允许合入 main 分支。**
   未归档的 `openspec/changes/` 条目不视为完成，main 只接受已归档规范对应的代码。

## 项目约定

- Rust edition 2024，数据导向设计（ECS/SoA），热路径避免堆分配
- unsafe 代码必须收敛并带 `# Safety` 文档
- 性能敏感代码需要 benchmark（cargo bench）
- 提交信息采用 conventional commits：`feat/fix/docs/refactor/perf/test/chore`
- 详细规范工作流见技能：`.agents/skills/openspec/`（DSH 自动发现；其他 AI 工具请手动加载该技能）
