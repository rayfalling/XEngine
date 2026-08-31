---
name: openspec
description: OpenSpec 规范驱动开发流程（XEngine 项目），覆盖提案→审阅→实施→验证→归档全流程，含"变更必须归档后才能合入 main"的硬性规则。Use when starting, applying, validating or archiving OpenSpec change proposals in this repository, when opening a feature/bugfix that must follow the spec workflow, or when asked to check a change against openspec/specs before merging.
---

# OpenSpec 规范驱动开发（XEngine）

## 硬性规则（Hard Rule）⚠️

> **任何 OpenSpec 变更必须完成 `openspec proposal archive <ID>`（归档）之后，才允许合入 main 分支。**
>
> main 分支只接受：规范已归档进 `openspec/specs/`、实现通过 `openspec proposal validate` 校验、
> `cargo test` 全绿的代码。未归档的 `openspec/changes/` 条目 = 未完成，不得开始 PR/合入流程。

## 目录结构

- `openspec/specs/` — 长期生效的系统规范（归档产物落在这里）
- `openspec/changes/` — 进行中的变更提案（`archive/` 存放历史归档）
- `openspec/config.yaml` — 项目上下文（Rust edition 2024、数据导向设计、性能基准约定、conventional commits；AI 生成提案时自动读取）

## 标准流程（每项功能必须走完）

1. **propose** — `openspec proposal create <id>`，从模板生成变更
   - 补全：`context`、变更的 `specs`（新增/修改哪些规范文件）、`tasks`、`Non-goals`、验收标准
   - 变更聚焦单一能力；性能相关变更写明性能预算
2. **review** — `openspec proposal list` / `openspec proposal show <id>` 确认内容
3. **implement** — `openspec proposal apply <id>` 导出任务清单，按 tasks 实现
   - Rust 约定：数据导向设计（SoA/ECS）、热路径避免堆分配、unsafe 需 `# Safety` 文档、
     `cargo test` 全绿、性能敏感代码带 benchmark（cargo bench）
4. **validate** — `openspec proposal validate <id>`：规范与实现一致性校验，通过后进入下一阶段
5. **archive** — `openspec proposal archive <id>`：合并 specs 到 `openspec/specs/`，变更移入 `changes/archive/`
6. **merge** — 归档完成后（且仅此后）才创建 PR 合入 main

## 命令速查

```pwsh
openspec proposal create <id>       # 新变更提案
openspec proposal list              # 列出进行中变更（含状态）
openspec proposal show <id>         # 查看提案详情
openspec proposal validate [<id>]   # 校验（不带 id 校验全部）
openspec proposal apply <id>        # 导出实施任务
openspec proposal archive <id>      # 归档（合入 main 的前置条件）
openspec proposal --help            # 全部子命令
```

## 常见陷阱

- 直接改代码而不建提案 → 违反流程，必须先回补 proposal（或标记为任务拆分完毕）
- 实现完成但未 validate 就提交 → 校验补做后才允许提 PR
- 把"已实现"等同于"已归档" → archive 是合入 main 的唯一通行证
- 归档操作不可逆（移入 archive 并合并 specs），归档前确认 validate 通过
