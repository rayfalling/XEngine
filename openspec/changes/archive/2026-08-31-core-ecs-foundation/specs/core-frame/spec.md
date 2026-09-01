## ADDED Requirements

### Requirement: 帧阶段语义（Unity 模型）
Engine SHALL 以 `Engine::tick` 驱动帧循环，每渲染帧执行：`FixedUpdate`（固定步长累积器，每帧 0..N 次）、`Update`（每渲染帧恰好 1 次，传入当前帧 dt）、`PostUpdate`（每渲染帧恰好 1 次）。Update/PostUpdate 为标准帧更新；FixedUpdate 为固定更新。tick 的输入 SHALL 为当前帧经过时间（含绝对时钟与帧 dt）。

#### Scenario: 慢帧多次 FixedUpdate
- **WHEN** 单帧耗时 100ms、固定步长 20ms
- **THEN** 该帧 FixedUpdate 执行 5 次，Update 与 PostUpdate 各执行 1 次，且执行顺序为 FixedUpdate → Update → PostUpdate

#### Scenario: 快帧零次 FixedUpdate
- **WHEN** 单帧耗时 5ms、固定步长 20ms
- **THEN** 该帧 FixedUpdate 执行 0 次，Update 与 PostUpdate 各执行 1 次

### Requirement: 帧率模式（限帧 / 不限帧）
Engine SHALL 支持限帧模式（目标帧率 F，Update/帧 dt 目标为 1/F；渲染帧率向目标对齐）与不限帧模式（不限制渲染帧率，Update dt 为实测帧时间）。两种模式 MUST 可切换；切换后 FixedUpdate 步长 MUST 恒定（固定步长默认 1/60s，可配置）。不限帧模式 MUST 对异常 dt 给出钳制策略（最大帧时间上限，防积分爆炸）。

#### Scenario: 限帧模式
- **WHEN** Engine 设置目标帧率为 60 且处于限帧模式
- **THEN** 每帧 Update 收到的 dt 约为 1/60s（16.7ms）

#### Scenario: 不限帧模式
- **WHEN** Engine 处于不限帧模式且实测帧间隔 12ms
- **THEN** Update 收到的 dt 为实测 12ms（不超过钳制上限）

#### Scenario: 模式切换
- **WHEN** 运行中从限帧 60 切换为不限帧再切回
- **THEN** 每次切换后 Update dt 遵循对应模式，FixedUpdate 步长不变

### Requirement: 函数式系统与阶段注册
系统 SHALL 以函数式定义（系统函数 + 参数：`Query`、`Res/ResMut`、`Commands`、`&mut World` 等），并由 `System` 包装提供调用与访问元数据。系统 MUST 注册到具体阶段（FixedUpdate / Update / PostUpdate）；注册后每帧由调度器按阶段顺序执行。系统函数在任何阶段 MUST 以参数化方式（而非手工枚举）执行逻辑。

#### Scenario: 注册系统按阶段调用
- **WHEN** 依次注册 Update 系统 U1、FixedUpdate 系统 F1
- **THEN** 每帧调用序为 F1（每固定步）→ U1（每帧）

### Requirement: 调度与自动拓扑排序
调度器 SHALL 对阶段内系统集合计算执行顺序：显式 `before`/`after` 依赖 MUST 被遵守；无显式依赖的系统 MUST 按注册顺序稳定执行（确定性）。调度器 MUST 检测"访问冲突"：两系统对同一组件/资源存在写-写或写-读冲突，且未通过显式排序消除时，MUST 在调度构建期报错并指明冲突系统对。无冲突下 MUST 允许任意顺序。

#### Scenario: 冲突构建期报错
- **WHEN** 系统 A 读组件 T、系统 B 写组件 T，且二者无 before/after 关系
- **THEN** 调度构建报错并给出 A、B 冲突对

#### Scenario: 显式排序消除冲突
- **WHEN** 对上述 A、B 添加 `B.after(A)`
- **THEN** 调度构建成功且执行顺序 A → B

#### Scenario: 无冲突顺序稳定
- **WHEN** 三个互不冲突的系统按 X、Y、Z 注册
- **THEN** 每帧执行顺序均为 X、Y、Z（确定性）

### Requirement: RenderSnapshot 接口（占位）
核心层 SHALL 定义渲染快照接口（帧末系统产出的渲染提交数据约定，如 `RenderSnapshot` 类型/ trait）；本变更 SHALL 仅定义接口与空数据，MUST NOT 实现任何设备平台图形逻辑，MUST NOT 依赖任何平台图形 API；设备层未来消费该接口。

#### Scenario: 接口可用
- **WHEN** Engine 完成一帧 tick
- **THEN** 帧末产出 `RenderSnapshot` 空实现数据，核心层无任何平台依赖（cargo tree 仅 std/项目内 crate 可验证）

### Requirement: 帧确定性
同一输入序列（相同 dt/固定步长/系统注册）下，系统调用顺序 SHALL 完全一致；FixedUpdate 次数由累积器数学决定，SHALL 可预测（单测可断言）。

#### Scenario: 确定性调用序列
- **WHEN** 用固定 dt 序列驱动 10 帧并记录系统调用日志
- **THEN** 两次运行日志完全一致
