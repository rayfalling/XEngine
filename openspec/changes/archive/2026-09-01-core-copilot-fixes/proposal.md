## Why

Copilot 代码评审（PR #1，Lite 档，5 条意见）指出正确性/UB 风险与 API 契约不一致，需在合入前解决：

1. ZST 显式对齐（`#[repr(align(N))]`）时描述符 align 错误 → 引用指针可能未对齐
2. `alloc/realloc` 未处理 OOM 返回 null → 立即 UB
3. `query2/query3` 同类型参数会运行时 panic → 应确定性拒绝
4. `Commands` 静默吞掉错误 → 违反"语义与同步 API 一致"
5. 格式/可读性小问题

## What Changes

- **registry.rs**：`ComponentDescriptor::of<T>` 恒用 `align_of::<T>()`（ZST 保真实对齐）
- **storage.rs**：`grow` 分配失败走 `handle_alloc_error`（abort 而非 UB）
- **world.rs**：
  - `query2/query3` 改为返回 `WorldResult<()>`；同类型参数 → `Err(BorrowConflict)`（确定性拒绝，不再 panic）
  - `flush_commands` 返回 `WorldResult<()>`：所有命令按序执行，返回**第一个**错误（与同步语义一致）
- **command.rs**：`WorldCommand` 闭包返回 `WorldResult`；错误在 flush 时浮现而非吞掉
- **schedule.rs**：系统边界 flush 结果显式处理（注释说明契约）
- 新增单测：`query 同类型拒绝`、`Commands 错误传播`（37 tests）
- 规范 delta：`core-ecs` 的「查询与迭代」「Commands 延迟操作队列」两条 Requirement 更新（MODIFIED）

## Capabilities

### New Capabilities
- 无

### Modified Capabilities
- `core-ecs`: 「查询与迭代」— 同一组件类型多参数查询 MUST 确定性返回 `Err(BorrowConflict)`（非 panic/UB）；「Commands 延迟操作队列」— flush MUST 返回首个错误，操作语义与同步 API 严格一致

## Impact

- API 变更（返回类型）：`query2/query3` → `WorldResult<()>`；`flush_commands` → `WorldResult<()>`（调用点已同步：bin demo/bench/测试）
- 修复 3 处 unsafe/UB 风险；无依赖变化
- 无 MR 门禁变化（CI 已覆盖两平台）

## Non-goals

- Copilot 评审之外的扩展评审项（类型化 bundle 宏等后续变更）
- 覆盖率/更多 lints

## Acceptance Criteria

- `cargo test` 全绿（含 2 个新单测）；clippy/fmt 干净
- `query2::<T,T>` / `query3` 任意重复类型返回 `Err(BorrowConflict)`
- `flush_commands` 对含 stale 操作的队列返回 `Err(StaleEntity)`，其余命令仍按序生效
- ZST `#[repr(align(16))]` 组件注册/存储/销毁无未对齐（单测覆盖）
