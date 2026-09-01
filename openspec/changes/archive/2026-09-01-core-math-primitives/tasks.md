# Tasks

## 1. 骨架与 workspace 接线

- [x] 1.1 根 `Cargo.toml` members 增加 `crates/xengine-math`；`[workspace.package]` 沿用 edition 2024 / version 0.1.0
- [x] 1.2 创建 `crates/xengine-math/Cargo.toml`（lib `xengine_math`、零依赖、`[features] xmath_align64 = []`）与 `src/lib.rs`（模块声明 + `engine 约定文档`）
- [x] 1.3 `cargo tree` 确认零外部依赖

## 2. 数值 trait 与常量

- [x] 2.1 `trait FloatNum`（std 数值运算 + `sqrt/abs/min/max/from_f32` 等）实现于 f32/f64；`trait IntNum` 实现于 i32/i64；公开文档化（对应 C++ `is_same_v` SFINAE 的 Rust 等价物）
- [x] 2.2 常量（`ZERO/ONE/IDENTITY`）与 `EPSILON`；`Display`
- [x] 2.3 单测：trait 可用性（f32/i32 各自实例化）

## 3. Vector2/3/4 与别名

- [x] 3.1 `Vector2<T>/Vector3<T>/Vector4<T>`：repr(C)、字段 x,y,z,(w)、分量与标量运算、dot/cross(3D)/length/length_sqr/normalize_or_zero/distance/lerp/abs/min/max/perpendicular/approx_eq/is_finite
- [x] 3.2 别名：`Vector2F/Vector2I/Vector3F/Vector3I/Vector4F/Vector4I`；i32 变体屏蔽浮点专用方法（trait bound 控制）
- [x] 3.3 单测：运算正确性 + 边界（零向量 normalize 等）+ 布局断言（size/align/偏移）

## 4. Quaternion 与别名

- [x] 4.1 `Quaternion<T>`：identity/from_axis_angle/from_euler_yxz/mul/dot/conjugate/inverse/rotate_vec3/slerp/nlerp/from_mat4/to_mat4/from_to；(x,y,z,w)
- [x] 4.2 `pub type QuaternionF = Quaternion<f32>`
- [x] 4.3 单测：与 D3D 系标准 YXZ 欧拉约定一致（对照矩阵值）、`rotate_vec3(q,v)` == `to_mat4×v`、slerp 端点、conjugate/inverse、从两向量构造；布局锁 w 位置

## 5. Matrix3/4 与投影视矩阵

- [x] 5.1 `Matrix3<T>`：行主序、转置、逆（旋转=转置）、to_mat4/from_mat4
- [x] 5.2 `Matrix4<T>`：identity/zero/from_trs/from_quat/from_translation/from_scale/mul/transpose/inverse（含行列式）/mul_vec3/mul_vec4（w=0 透传）/look_at_lh/perspective_lh（深度 0..1）/ortho_lh/to_col_major/from_col_major
- [x] 5.3 别名 `Matrix3F/Matrix4F`
- [x] 5.4 单测：行向量×矩阵方向、平移位 m[3][0..2]、TRS 组合与逆一致、投影/视矩阵数值对照、行列互转往返、inv==transpose（旋转）

## 6. AABB

- [x] 6.1 `AABB<T>`（min/max + 额外可选顶点辅助）：from_min_max/union/intersects/contains；`pub type AABBF = AABB<f32>`（AABB 全大写，`#[allow(clippy::upper_case_acronyms)]`）
- [x] 6.2 单测：正常/退化（min>max）/空盒语义、union/包含、布局断言

## 7. 双布局对齐（16B / 64B feature）

- [x] 7.1 `repr(C)` + `#[cfg_attr(feature = "xmath_align64", repr(align(64)))]`/`repr(align(16))` 覆盖 Vector3/4、Quaternion、Matrix3/4、AABB；`Vector2<T>` 固定 align(8)
- [x] 7.2 单测：默认特征下 size/align/偏移断言；`--features xmath_align64` 下同断言（CI 矩阵覆盖）
- [x] 7.3 文档：64B 启用时 C++ 镜像头同步义务 + 数组 stride 成本说明

## 8. SIMD 预留 kernel

- [x] 8.1 `pub(crate) mod kernel`：显式 load/store 点 + 集中运算内核（标量当前）；文档固化"公开布局不被 SIMD 改变"
- [x] 8.2 后续 SIMD 替换的行为等价说明（文档 + 单测回归锚点）

## 9. 验证与基准

- [x] 9.1 `cargo test`（默认 + `--features xmath_align64`）全绿
- [x] 9.2 `cargo clippy --all-targets` + `cargo fmt --check` 通过
- [x] 9.3 `benches/` 占位：矩阵乘/点变换基线（自计时 harness=false，与现有 ecs bench 风格一致）
- [x] 9.4 `openspec change validate core-math-primitives` 通过
