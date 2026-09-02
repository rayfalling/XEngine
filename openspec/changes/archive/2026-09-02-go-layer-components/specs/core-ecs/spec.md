## MODIFIED Requirements

### Requirement: 组件类型运行时注册（type-id 扩展）
- OLD: World SHALL 提供组件类型注册表，以 `TypeId` 为键注册组件类型，注册项 MUST 含 `size`、`align`、`drop` 函数与可选的 `scriptable` 标记。组件类型必须允许运行时注册（编译器之外），脚本组件（`scriptable = true`，type-erased payload）必须走同一注册路径。重复注册同一 TypeId 必须返回错误。注册表驱动所有 create/add/查询路径。
- NEW: World SHALL 提供组件类型注册表，以 `TypeId` 为键注册组件类型，注册项 MUST 含 `size`、`align`、`drop` 函数与可选的 `scriptable` 标记。组件类型必须允许运行时注册（编译器之外），脚本组件（`scriptable = true`，type-erased payload）必须走同一注册路径。重复注册同一 TypeId 必须返回错误。注册表驱动所有 create/add/查询路径。组件类型 MAY 实现 `Component` trait（`pub trait Component: 'static`，见下）携带生命周期钩子；此时经 `register_component::<T: Component>()` 显式注册，描述符 `hooks` 字段（`Option<ComponentHooks>`，`ComponentHooks { on_add: Option<fn(*mut u8, *mut ())>, on_remove: Option<fn(*mut u8, *mut ())> }`，双参 = 组件数据指针 + 生命周期上下文指针）非空；未实现/未注册 hooks 必为 `None`（既有注册路径零影响）。

#### Scenario: 钩子生命周期触发（含上下文）
- **WHEN** 带钩子组件经 `add` 成功加入实体、随后 `remove::<T>`、再 `destroy`（存在绑定的生命周期上下文指针）
- **THEN** 每次 add 完成后在组件数据地址上调用一次 `on_add(ptr, ctx)`；remove 时对 T 删除**前**调用一次 `on_remove(ptr, ctx)`；destroy 时对每个组件在其 drop 前调用 `on_remove` 恰一次；`ctx` 恒为绑定的上下文指针（如场景指针）

#### Scenario: 迁移不触发钩子
- **WHEN** 实体经 `add`/`remove` 触发 archetype 迁移，其余未删除组件的行被 bitwise 移动
- **THEN** 被移动（未删除）组件 MUST NOT 触发 `on_remove`/`on_add`；仅新增组件触发 `on_add`、仅被删除组件触发 `on_remove`

#### Scenario: Commands 路径同样触发
- **WHEN** 系统内经 `Commands` 排队 add/remove/destroy 并在边界 flush
- **THEN** 钩子与同步 API 语义一致触发（flush 时按入队序，每个操作恰一次）

#### Scenario: 无钩子类型零影响
- **WHEN** 既有类型通过 `register::<T>()` 或自动注册路径注册并使用（hooks None）
- **THEN** 行为与旧版完全一致（不调用任何钩子函数），drop 语义不变

#### Scenario: 重复注册钩子路径
- **WHEN** 同一 TypeId 先 `register_component` 再 `register`（或反向）
- **THEN** 第二次注册返回重复注册错误；第一次的钩子/描述符保持

#### Scenario: 上下文绑定
- **WHEN** 世界绑定生命周期上下文指针（`World::bind_hook_context`，要求 `Pin<Box<Scene>>` 等稳定地址，单线程）
- **THEN** 之后所有钩子调用收到该指针；未绑定上下文时钩子调用得到 null 指针（实现 MUST 跳过调用或传 null，由 go 层保证绑定前不触发）
