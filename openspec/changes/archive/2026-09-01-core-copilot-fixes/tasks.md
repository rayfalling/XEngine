## 1. 修复实现

- [x] 1.1 registry.rs：`ComponentDescriptor::of<T>` 恒用 `align_of::<T>()`（ZST 保真实对齐）
- [x] 1.2 storage.rs：`grow` OOM → `handle_alloc_error`（不再 UB）
- [x] 1.3 world.rs：`query2/query3` 返回 `WorldResult<()>`，同类型参数 → `Err(BorrowConflict)`
- [x] 1.4 command.rs + world.rs：`WorldCommand` 返回 `WorldResult`；`flush_commands` 返回首个错误
- [x] 1.5 schedule.rs：系统边界 flush 显式处理（契约注释）

## 2. 测试与规范

- [x] 2.1 新单测：query 同类型拒绝、Commands 错误传播（37 tests 全绿）
- [x] 2.2 `openspec validate core-copilot-fixes` 通过；归档（specs 合并 MODIFIED core-ecs）

## 3. 合入

- [ ] 3.1 推送分支、创建 MR；CI 两平台全绿
- [ ] 3.2 **no-FF 合入**（merge commit `--merge`，非 squash）；分支自动删除
