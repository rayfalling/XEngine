# core-ecs Specification

## Purpose
TBD - created by archiving change core-ecs-foundation. Update Purpose after archive.
## Requirements
### Requirement: 实体句柄（世代句柄）
World SHALL 以 `(index: u32, generation: u32)` 唯一标识实体。创建 SHALL 返回新句柄；实体销毁后其 index 可被复用于新实体且 generation 必须递增；旧句柄（世代不匹配）对任何访问返回 `Err(StaleEntity)`，绝不允许未定义行为。generation 溢出时该槽位必须永久停用。所有句柄类操作 MUST 为 O(1)。

#### Scenario: 销毁后复用且世代递增
- **WHEN** 实体 A 被 destroy，随后同槽位创建实体 B
- **THEN** B 的 generation 大于 A 的 generation，且旧句柄对 B 访问返回 `Err(StaleEntity)`

#### Scenario: 槽位停用
- **WHEN** generation 达到 u32 最大值并再次复用
- **THEN** 该 index 不再分发新实体，且旧句柄访问返回 `Err(StaleEntity)`

### Requirement: 组件类型运行时注册（type-id 扩展）
World SHALL 提供组件类型注册表，以 `TypeId` 为键注册组件类型，注册项 MUST 含 `size`、`align`、`drop` 函数与可选的 `scriptable` 标记。组件类型必须允许运行时注册（编译器之外），脚本组件（`scriptable = true`，type-erased payload）必须走同一注册路径。重复注册同一 TypeId 必须返回错误。注册表驱动所有 create/add/查询路径。

#### Scenario: 运行时注册自定义组件类型
- **WHEN** 运行期注册一个自定义组件类型（含 drop 函数）
- **THEN** 该类型可立即用于 create/add/查询，实体销毁时 drop 函数被调用

#### Scenario: 脚本组件注册
- **WHEN** 运行期注册 `scriptable = true` 的组件类型（payload 为 type-erased 字节）
- **THEN** 该组件按与普通组件相同的生命周期语义存取，且保留 `scriptable` 标记供脚本运行时使用

#### Scenario: 重复注册报错
- **WHEN** 对同一 TypeId 二次注册
- **THEN** 返回注册错误，且第一次注册的类型与存储不受影响

### Requirement: 生命周期函数与语义
World SHALL 提供 `create`、`destroy`、`add`、`remove`、`get/get_mut`、`contains`、`iterate`、`query`、`clear` 生命周期函数（clean 定义：create 可带初始组件、允许空；destroy 销毁实体：组件 drop → 移入回收表 → 复用槽位；add 批量添加组件；remove 移除组件、实体保留）。语义 MUST 如下：重复 `add` 同一组件返回 `Err(InsertAlreadyExists)` 且状态不变；`remove` 不存在的组件 MUST no-op 幂等；世代不匹配句柄访问 MUST 返回 `Err(StaleEntity)`，而 `destroy` 对已销毁/失效句柄 MUST 幂等返回 `Ok`；`destroy`/`clear` MUST 按组件注册顺序执行 drop 且每个实体只 drop 一次；`clear` MUST 销毁全部实体并保留空 archetype 结构。

#### Scenario: 重复 add 报错
- **WHEN** 对已有组件 T 的实体再 add T
- **THEN** 返回 `Err(InsertAlreadyExists)`，实体组件状态保持原值

#### Scenario: remove 缺失组件幂等
- **WHEN** 对无组件 T 的实体 remove T
- **THEN** 返回 Ok 且实体状态不变

#### Scenario: stale 句柄语义
- **WHEN** 用世代不匹配句柄访问实体，或对其 destroy
- **THEN** 访问返回 `Err(StaleEntity)`，`destroy` 返回 Ok（幂等）

#### Scenario: drop 顺序确定性
- **WHEN** 实体含按注册序 A、B、C 三组件并被 destroy
- **THEN** drop 以 A→B→C 顺序各执行一次

#### Scenario: clear 全销毁
- **WHEN** 对含多实体的 World 调用 clear
- **THEN** 全部实体销毁（每个组件 drop 恰一次），archetype 结构与注册表保留，后续可继续 create

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

### Requirement: Archetype 组件存储（SoA）
World SHALL 按 archetype（实体组件类型集相同）组织实体。同一组件类型的列 MUST 为 SoA 连续存储；存储 MUST 为 type-erased（字节缓冲 + drop 函数）以支持运行时注册类型。实体组件变更 MUST 迁移到目标 archetype。列内存 MUST 缓存友好（连续、无逐实体间接跳转）。

#### Scenario: add 后 archetype 迁移
- **WHEN** 向实体 add 新组件类型
- **THEN** 实体出现在新 archetype，旧 archetype 中不再包含该实体，列连续存储

#### Scenario: 迭代连续
- **WHEN** 对某组件列迭代
- **THEN** 每个匹配实体的组件数据连续排列，无逐实体间接跳转

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

### Requirement: 资源（Res / ResMut）
World SHALL 以 `TypeId` 键存储单例资源。系统 MUST 可通过 Res（不可变）与 ResMut（可变）访问。读取未注册资源 MUST 返回错误（或略过并记录，不得 panic/UB）。资源生命周期与其数据一致（销毁时 drop）。

#### Scenario: 资源读写
- **WHEN** 系统通过 ResMut 写入、通过 Res 读取同一资源
- **THEN** 读到的为写入后的值

#### Scenario: 未注册资源
- **WHEN** 系统读取未注册资源
- **THEN** 返回读取错误而不是未定义行为

### Requirement: 性能预算
create/add/remove/destroy MUST 摊还 O(1)；single/join 迭代每匹配实体 MUST O(1)；`benches` 目录 MUST 提供占位基准（万级实体 create / 查询），稳定基线在后续变更建立。

#### Scenario: 基准消耗上限
- **WHEN** 运行占位基准（100k 实体 create + single 迭代）
- **THEN** 时间量与 O(1) 摊还复杂度一致（无随实体数增长的额外非线性开销）

