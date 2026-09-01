## MODIFIED Requirements

### Requirement: Commands 延迟操作队列
World SHALL 提供 `Commands` 延迟操作队列（`create`/`add`/`remove`/`destroy`/资源变更），供系统在不可变借用期间排队；Commands MUST 按入队顺序 flush，flush 时点 MUST 为系统边界（每个系统执行完毕后）；flush 后队列清空。Commands 的操作语义 MUST 与同步 API 一致——`flush_commands` MUST 返回 `WorldResult<()>`：全部命令按入队序执行，并返回其中**第一个**出现的错误（若存在）。

#### Scenario: 系统内 Commands 顺序 flush
- **WHEN** 系统执行期间依次入队 create、add、destroy
- **THEN** 系统结束后按入队顺序生效，且操作结果与同步 API 一致

#### Scenario: 队列清空
- **WHEN** flush 完成后再次查询待处理命令
- **THEN** 队列为空

#### Scenario: 错误传播
- **WHEN** 队列中含失败命令（例如对 stale 句柄 remove）
- **THEN** flush 返回对应 `WorldError`（如 `StaleEntity`），其余命令仍按序执行

### Requirement: 查询与迭代（single/join）
World SHALL 支持 single 组件迭代（`iterate`）与至多 3 个组件类型的交集（`query` join）迭代。查询结果 MUST 反映当前 archetype 集合（此后增删组件后不再出现/出现）。迭代 MUST 每匹配实体 O(1)。借用规则：查询中同一组件类型作为多个参数（如 `query2::<T,T>`）MUST 确定性返回 `Err(BorrowConflict)` 而非 panic/UB；调度层未显式排序的写-写/写-读冲突 MUST 在构建期拒绝。

#### Scenario: join 结果正确性
- **WHEN** 对实体集合执行 query(A & B)
- **THEN** 恰好返回同时含 A、B 的实体；实体移除 A 后不再出现在结果中

#### Scenario: 借用冲突拒绝
- **WHEN** 两个查询在同一系统内分别以可变与不可变访问同一组件且未显式排序
- **THEN** 调度构建拒绝并指出冲突系统

#### Scenario: 同类型参数拒绝
- **WHEN** 调用 `query2::<T, T>`（或 query3 任意两类型相同）
- **THEN** 返回 `Err(BorrowConflict)`，不 panic、不产生未定义行为
