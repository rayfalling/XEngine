# core-math Specification

## Purpose
核心层数学基元与约定：泛型向量/四元数/矩阵/AABB 类型及其完全限定别名，D3D 风格（行主序、行向量×矩阵、左手系、forward=+Z、欧拉 YXZ）数学约定，`#[repr(C)]` + 16B/64B 双布局 FFI 契约与单测锁定项，SIMD 预留接口与数值安全语义。供 GO 层（Transform 等组件）与渲染/设备层消费。

## Requirements
### Requirement: 泛型数学基元与完全限定命名
`xengine_math` SHALL 提供泛型基元 `Vector2<T>`、`Vector3<T>`、`Vector4<T>`、`Quaternion<T>`、`Matrix3<T>`、`Matrix4<T>`、`AABB<T>`（Rust 泛型对应 C++ 模板，编译期静态实例化），并 MUST 提供完整命名类型别名：`Vector2F/Vector3F/Vector4F`、`Vector2I/Vector3I/Vector4I`（i32）、`QuaternionF`、`Matrix3F/Matrix4F`、`AABBF`。公开类型名 MUST 使用全名（禁止 `Vec3f`/`QuatF`/`AabbF` 简写）。i32 变体 MUST 支持布局与分量运算，MUST NOT 提供浮点专用运算（sqrt/normalize）。

#### Scenario: 完整别名存在
- **WHEN** 引用公开类型 `xengine_math::Vector3F`、`xengine_math::QuaternionF`、`xengine_math::Matrix4F`、`xengine_math::AABBF`、`xengine_math::Vector3I`
- **THEN** 均解析为对应泛型实例（`Vector3<f32>` / `Quaternion<f32>` / `Matrix4<f32>` / `AABB<f32>` / `Vector3<i32>`）

#### Scenario: 浮点专用运算不适用于整数变体
- **WHEN** 对 `Vector3I` 调用 `length/normalize` 或 `Quaternion<f32>` 外的四元数插值
- **THEN** 编译错误（仅在数值 trait bound 内提供），i32 变体保留分量运算与布局

### Requirement: 数学约定（D3D 风格，D3D 风格）
所有矩阵 MUST 为**行主序**；向量变换 MUST 为**行向量×矩阵**；`transform_point(v, A·B)` MUST 等价于先应用 A 后应用 B；坐标系 MUST 为**左手系**且 identity 旋转下 forward=+Z；四元数分量 MUST 为 (x,y,z,w)（w 后置）；欧拉角转换 MUST 使用 **YXZ** 顺序并与 D3D 系标准欧拉约定一致（绕 Y→X→Z 内旋，pitch/yaw/roll）。`Matrix4` 平移 MUST 落在 `m[3][0..2]`。

#### Scenario: 行向量×矩阵方向
- **WHEN** `v = transform_point(v0, A·B)`
- **THEN** 结果与 `transform_point(transform_point(v0, A), B)` 一致（先 A 后 B）

#### Scenario: 平移位置
- **WHEN** `Matrix4F::from_trs(pos, q, s)` 或 `from_translation(t)` 构造
- **THEN** `m[3][0..2] == t`（行主序），其余行为零/单位组合

#### Scenario: identity forward=+Z
- **WHEN** `transform_vec3((0,0,1), Matrix4F::identity())`
- **THEN** 结果仍为 `(0,0,1)`（左手系 forward=+Z）

#### Scenario: 四元数与欧拉
- **WHEN** `QuaternionF::from_euler_yxz(p, y, r)` 后 `to_mat4`/`to_mat3`
- **THEN** 与 D3D 系标准 YXZ 欧拉约定对应矩阵逐元素一致（< 1e-6）

### Requirement: 布局与对齐（FFI 契约）
所有公开数学类型 MUST 为 `#[repr(C)]`。默认对齐 MUST 为 16 字节（`Vector2<T>` 为 8）。`cargo feature "xmath_align64"`（默认关闭，仅 `xengine-math`）MUST 将全类型对齐切换为 64 字节。两套布局 MUST 均有 `size_of/align_of` 与关键字段偏移单测锁定（`Matrix4` 平移行、`Quaternion` w 位置、`AABB` min/max）。文档 MUST 说明：启用 64B 时 C++ 镜像头须同步布局。

#### Scenario: 默认 16B 布局锁定
- **WHEN** 未启用 feature 编译
- **THEN** `align_of::<Vector3F>()==16`、`align_of::<Matrix4F>()==16`，`Matrix4F` size==64，`Vector3F` 字段偏移 x=0,y=4,z=8

#### Scenario: 64B feature 布局锁定
- **WHEN** 启用 `xmath_align64` 编译
- **THEN** `align_of::<Matrix4F>()==64`（其余类型同理），偏移断言仍成立，`size_of` 为 64 的倍数

### Requirement: Phase 1 运算集
`xengine_math` SHALL 提供以下运算（各类型）：向量（分量 ±×÷ 与标量、`dot/cross/length/length_sqr/normalize_or_zero/distance/lerp/abs/min/max/perpendicular/approx_eq/is_finite`）；四元数（`identity/from_axis_angle/from_euler_yxz/mul/dot/conjugate/inverse/rotate_vec3/slerp/nlerp/from_mat4/to_mat4/from_to`）；矩阵（`identity/zero/from_trs/from_quat/from_translation/from_scale/mul/transpose/inverse/行列式/mul_vec3（点变换）/mul_vec4（齐次，无除法）/transform_vec3（方向）/look_at_lh/perspective_lh（深度 0..1, D3D 式）/ortho_lh`）；`AABB`（`from_min_max/union/intersects/contains`）。MUST 提供常量（`ZERO/ONE/IDENTITY`）与 `Display`。

#### Scenario: 投影矩阵 D3D 深度
- **WHEN** `perspective_lh(fovy, aspect, near, far)` 构造
- **THEN** 深度映射为 [0,1]（行主序索引：`m[2][2]=far/(far-near)`、`m[2][3]=1`、`m[3][2]=-(near·far)/(far-near)`），与 D3D12 常量缓冲约定一致

#### Scenario: 视矩阵前向
- **WHEN** `look_at_lh(eye, target, up)` 构造
- **THEN** 前向量为 target-eye 归一化（+Z 语义），相机空间 forward=+Z

### Requirement: 数值安全语义
向量规定零向量 `normalize_or_zero` MUST 返回零向量（不产生 NaN）；`approx_eq` MUST 使用 epsilon 比较；`is_finite` MUST 正确判定；`mul_vec4(v, m)` MUST 为齐次坐标变换 `v·M`（结果 4 分量线性组合，w 分量透传，**不执行透视除法**，w=0 不产生除零也不 panic）；点变换 `transform_point`/`mul_vec3`（隐含 w=1，含平移）与方向变换 `transform_vec3`（w=0，不含平移）语义 MUST 明确区分。

#### Scenario: 零向量归一化
- **WHEN** `normalize_or_zero((0,0,0))`
- **THEN** 结果为 `(0,0,0)`，无 NaN

#### Scenario: 齐次变换 w=0 无除零
- **WHEN** `mul_vec4((1,2,3,0), m)` 
- **THEN** 结果为 `v·M` 的线性组合（w 分量照常参与、无除法、不 panic）；`transform_vec3` 不含平移（方向语义）而 `transform_point` 含平移（点语义）

### Requirement: SIMD 预留（公开布局隔离）
公开结构体字段 MUST 即布局契约；SIMD 实现 MUST 只发生在 crate 内部 `kernel` 模块（显式 load/store 包围），MUST NOT 修改公开字段序或对齐（除非布局变更随 feature 明示）；后续 SIMD 启用 MUST 保持公开行为与标量实现一致（单测锁定）。

#### Scenario: SIMD 后行为不变
- **WHEN** 后续任一运算内核切换为 SIMD 实现
- **THEN** 公开标量 API 的结果与切换前完全一致（单测回归锁定）

### Requirement: 已知互转与单一化
数学库 MUST 仅暴露一种内置主序（行主序）；对列主序消费方（未来 Vulkan 适配）MUST 提供显式互转 API（`to_col_major` / `from_col_major`），不提供隐式转换；转置函数 MUST 保持行主序语义。

#### Scenario: 行列互转
- **WHEN** `m.to_col_major()` 后 `from_col_major()` 往返
- **THEN** 得到原矩阵（< 1e-6）

