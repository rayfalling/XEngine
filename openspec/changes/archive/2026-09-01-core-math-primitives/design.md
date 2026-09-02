## Context

GO 层（Transform）与渲染层从字段定义到 FFI 消费都依赖数学基元；此前的探索：ECS/帧调度已归档（core-ecs/core-frame），渲染层明确"先规划 GO/Component 再实现"。本变更定义 `core-math` 能力：类型 + 约定 + 布局契约。参考 D3D 系业界标准约定（与主流游戏引擎数学库的通用形态一致）。

## Goals / Non-Goals

**Goals:**
- 可落地的 `crates/xengine-math`（纯 Rust、零外部依赖、edition 2024，workspace member）
- 泛型基元 + 完全限定别名（`Vector2F/Vector2I/…/QuaternionF/Matrix3F/Matrix4F/AABBF`，AABB 全大写）
- D3D 风格约定（行主序/行向量×矩阵/左手系/forward=+Z/欧拉 YXZ/平移 m[3][0..2]）单测锁定
- `#[repr(C)]` 16B 默认 + `xmath_align64` feature（64B）双布局契约与锁定
- Phase 1 运算集（proposal 所列），SIMD 预留 kernel 层（不引入 unsafe/平台分派）
- 每公开类型/运算有单测；`cargo test`（含 feature）全绿；矩阵乘等 bench 占位

**Non-Goals:**
- 不引入任何第三方数学库（glam/nalgebra 否决：项目零依赖约定 + FFI 布局可控性）
- 设备层（D3D12/Metal/Vulkan）C++ 镜像头与消费：后续渲染层变更
- SIMD 实现（本变更仅预留 kernel 接口；由 benchmark 驱动）
- Phase 2 运算：`Plane`/`Frustum`/`Ray` 相交/`Float16`/`Orthogonalize`/`AddScaled` 等（渲染层变更按需引入）
- 反射/序列化耦合（数学库零反射依赖，解耦做法：反射/桥接放置在上层）
- 运行时动态类型（脚本层用 enum/对象包装，数学核心保持静态实例化）

## Decisions

### D1 泛型 vs 具体类型（用户决策：用泛型 + 完全限定别名）
实现为 `Vector3<T>` 等泛型 struct（Rust 泛型 = C++ 模板对应物，编译期 monomorphization），公开别名 `pub type Vector3F = Vector3<f32>` 等（对应业界 template + typedef 模式）。数值操作经 crate 内置 `trait FloatNum`（对应 C++ `is_same_v<float>` SFINAE；`min_specialization` 不稳定故不特化）：`impl<T: FloatNum> Vector3<T>` 提供 length/normalize；i32 变体只布局/分量运算。**备选**：宏生成具体类型（无 trait 成本但双份实现难维护）、具体类型手写（×）、立即引入 num-traits 依赖（违反零依赖，×）。

### D2 行主序/行向量/左手/forward=+Z（用户决策）
与 D3D 系约定对齐：行主序、`transform_point` 行向量左乘、左手系 forward=+Z、四元数 w 后置、欧拉 YXZ ≡ D3D 系标准 roll/pitch/yaw 内旋顺序、`perspective_lh` 深度 0..1。**备选**：列主序右手（Vulkan 原生，D3D12 主平台需适配，×）。行/列消费差异用 `to_col_major/from_col_major` 显式互转吸收（Vulkan 适配留给设备层）。

### D3 布局对齐：repr(C) + 16B 默认 / 64B feature（用户决策）
默认 `align(16)`（SSE/AVX 常量缓冲）；`xmath_align64` feature（默认关、仅本 crate）→ `align(64)`（对齐业界"可选高对齐"配置模式）。**不默认 64B**：数组 stride 变大、缓存利用率下降；64B 仅高对齐需求场景。**定稿**：除 `Vector2<T>` 固定 `align(8)`（2D 非热路径）外，其余类型（Vector3/4、Quaternion、Matrix3/4、AABB）随 feature 在 16/64 切换；两套布局均单测锁定 size/align/偏移；文档明示 C++ 镜像同步义务。

### D4 SIMD 预留 kernel
公开字段=布局契约（不把 SIMD 类型融进公开布局；union 双访问对 FFI 无益，只需字段序一致）。crate 内部 `kernel` 模块：运算集中、显式 load/store 点；后续 SIMD 走"内部替换、公开语义与测试不变"。对齐路径（SSE 需 16B、AES/AVX512 需 64B）与 `xmath_align64` 联动预留。

### D5 命名
完全限定（不简写）：`Vector2F/Vector2I/Vector3F/Vector3I/Vector4F/Vector4I/QuaternionF/Matrix3F/Matrix4F/AABBF`。AABB 全大写（`#[allow(clippy::upper_case_acronyms)]` 于该类，保持最终命名）。

### D6 MVP 边界
Phase 1 = proposal/spec 所列；`Plane/Frustum/Ray/Float16` 等留渲染层变更（按需）；bench 占位纳入（矩阵乘/点变换，记录 O(1) 常量基线）。

## Risks / Trade-offs

- [泛型 + trait（FloatNum）抽象成本] → trait 仅集中于数值运算（sqrt 等），公开文档化；单测覆盖 f32 与 i32 两实例化
- [16B/64B 双布局 FFI 漂移] → 两套布局单测锁定 + 文档 XEngine C++ 镜像同步义务；默认 16B 是唯一"生产"布局
- [行/列主序误解] → 约定写入文档（行向量 × 矩阵，先 A 后 B）+ `to_col_major` 显式 API；单测锁定方向
- [标量 MVP 性能] → kernel 预留 + benchmark 基线；SIMD 后续由性能报告驱动
- [命名含 I/F 后缀编译权重] → 仅别名，无重复实现（泛型唯一实现）

## Migration Plan

1. 根 `Cargo.toml` workspace members 增加 `crates/xengine-math`
2. 骨架：lib（零依赖、`xengine_math`）
3. 按 D1..D6 实现类型/运算/布局；单测伴随
4. `cargo test`（含 `--features xmath_align64`）全绿；clippy/fmt；bench 占位
5. 回滚：独立分支 + MR 前不触碰 main；失败丢弃分支

## Open Questions

- 无（用户已定：命名/通用型/对齐/约定/钩子均拍板；`Component` 钩子形态属 GO 层变更）
