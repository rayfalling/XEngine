## Context

PR #1 已合入（squash），Copilot 评审（Lite，5 条）在合入前给出正确性/契约问题。本次为小型修复变更：修 UB 风险 + 对齐 API 契约。

## Goals / Non-Goals

**Goals:**
- 修复 ZST 对齐、OOM UB（正确性）
- query 同类型参数确定性拒绝（Err 而非 panic）
- Commands 错误语义与同步 API 一致（flush 返回首个错误）
- 规范 delta（core-ecs 两条 Requirement 更新）

**Non-Goals:**
- 类型化 bundle 宏、时序优化、覆盖率

## Decisions

### D1 ZST 对齐：恒用 `align_of`（Copilot #1）
真实对齐始终正确（ZST 亦可 `repr(align)`）；列布局按 `align.max(1)` 分配，元素步长 `size.max(1)=1` 不影响 0 字节值。

### D2 OOM：`handle_alloc_error`（Copilot #2）
`alloc/realloc` 返回 null → 与标准分配器一致 abort（`handle_alloc_error`），消除 UB；不会出现半初始化列状态（abort 前无存活指针）。

### D3 query 拒绝：返回 `Err(BorrowConflict)`（Copilot #3）
替代 `assert_ne!` 运行时 panic；与 spec「借用冲突拒绝」语义一致（确定性错误路径）。API 变更 `-> WorldResult<()>`，调用点同步。

### D4 Commands 错误：闭包返回 `WorldResult`（Copilot #4）
队列闭包签名改为 `FnOnce(&mut World) -> WorldResult<()>`；flush 按序执行全部命令并返回第一个错误（顺序语义保持）；调度器 `run_stage` 显式忽略（注释契约，供显式 flush 调用者使用）。

## Risks / Trade-offs

- [API 破坏（返回值）] → 首次发布前损害极小；调用点（demo/bench/tests）已同步
- [ZST 对齐修复影响] → 现有测试（ZST 未覆盖对齐）新增 `repr(align)` 单测
- [flush 继续执行剩余命令] → 已文档化（第一个错误优先返回，顺序保持）

## Migration Plan

1. 修复 + 新单测 → 本地门禁全绿 → validate/archive
2. 分支 → MR（CI 两平台）→ 全绿后**no-FF（merge commit）合入**→ 自动删除分支

## Open Questions

- 无
