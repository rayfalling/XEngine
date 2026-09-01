## Why

渲染层与 GO 层（Transform 等组件）的每个数学约定都必须先锁定：向量/四元数/矩阵的**内存布局、行/列主序、坐标系、转换顺序**是 FFI 契约（核心层 ↔ C++ 设备层镜像）的根，任何漂移都会在设备层造成数据错位。当前 workspace 没有任何数学类型（`xengine-core` 零依赖），GO 层无法定义 `Transform`。本变更建立自研、零依赖、D3D 风格（行主序/行向量/左手系/forward=+Z）的数学基元库，与 NeoX `math3d` 的成熟约定对齐（分析参考：NeoX `math3d` 自研包 + cocos2d-x math + DirectXMath 语义）。

## What Changes

- 新增 workspace crate **`crates/xengine-math`**（lib `xengine_math`，纯 Rust、零外部依赖、std only）
- 泛型基元（对应 NeoX `_Vector3<T>` + typedef 结构，编译期静态实例化）：
  - `Vector2<T>` / `Vector3<T>` / `Vector4<T>`（分量运算、dot/cross、normalize 等）
  - `Quaternion<T>`（(x,y,z,w)，轴角/欧拉/矩阵互转、旋转、slerp/nlerp）
  - `Matrix3<T>` / `Matrix4<T>`（**行主序**、行向量×矩阵、平移在 `m[3][0..2]`、TRS/视/投影构造）
  - `AABB<T>`（包围盒：union/相交/包含）
- **完全限定公开名**（类型别名）：`Vector2F`、`Vector3F`、`Vector4F`（f32）；`Vector2I`、`Vector3I`、`Vector4I`（i32）；`QuaternionF`、`Matrix3F`、`Matrix4F`、`AABBF`（全大写缩写）。类型名**不简写**（无 `Vec3f`/`QuatF`/`AabbF`）
- 数学约定锁定：行主序、行向量×矩阵（`transform_point(v, A·B)` 先 A 后 B）、左手系、forward=+Z、四元数 `w` 后置、欧拉 **YXZ**（与 DXMath `XMQuaternionRotationRollPitchYaw` 语义一致）、`perspective_lh` 深度 0..1（D3D 式）
- **布局契约**：`#[repr(C)]` + 默认 `align(16)`；`cargo feature "xmath_align64"`（默认关闭，仅本 crate）切换 `align(64)`；两套布局均有 size/align/字段偏移单测锁定；启用 64B 时 C++ 镜像须同步
- 数值安全语义：`normalize_or_zero`（零向量→零）、`approx_eq(eps)`、`is_finite`、透视除法 w=0 行为与 DXMath 一致（透传）
- **SIMD 预留**：公开结构体字段即布局契约；SIMD 只允许发生在 crate 内部 `kernel` 模块的 load→运算→store 中（后续 feature，本变更仅预留下层）
- 每个公开类型/函数配套单元测试；`cargo test` 全绿；性能敏感运算（矩阵乘等）带 bench 占位

## Capabilities

### New Capabilities
- `core-math`: 数学基元与约定——泛型向量/四元数/矩阵/AABB、D3D 风格数学约定、repr(C)+16B/64B 双布局 FFI 契约、SIMD 预留与单测锁定项

### Modified Capabilities
- 无（`xengine-core` 保持零改动；本变更不触碰 ECS）

## Impact

- 仓库结构：workspace 新增 member `crates/xengine-math`；`xengine` bin 暂不依赖
- 依赖：零外部依赖（std only）；不引入 glam/nalgebra（沿用项目自研 + 零依赖约定）
- 层 / 后端：核心层（100% Rust）；设备层（D3D12/Metal/Vulkan）本变更**只**定义数据契约，C++ 镜像头与消费在渲染层变更实现
- API：全新库，无既有公开 API 破坏
- 性能预算：标量 MVP；矩阵乘/点变换 O(1) 常数开销（benchmark 基线记录）；SIMD 由基准驱动后续启用

## Acceptance Criteria

- `cargo test`（含 `--features xmath_align64` 双布局）全绿；每个公开类型/运算有单测
- `cargo tree` 显示 `xengine-math` 零外部依赖
- 布局锁定单测：`size_of/align_of` + 字段偏移（Mat4F 平移行、QuaternionF `w` 位置）在 **16B 默认与 64B feature** 两套下均断言正确
- 数学约定单测：行向量×矩阵方向、平移位、identity forward=+Z、`look_at_lh`/`perspective_lh`(深度 0..1)/`ortho_lh` 位置、`quat↔mat↔euler(YXZ)` 往返 < 1e-5、`rotate_vec3(q,v)`==对应 Mat4F 结果、旋转矩阵 `inverse==transpose`、TRS 组合逆一致、`slerp` 端点=输入（DXMath 同语义）
- `normalize_or_zero(0)==零`、`is_finite`、`approx_eq` 行为单测；透视除法 w=0 行为锁定
- clippy + fmt 检查通过
