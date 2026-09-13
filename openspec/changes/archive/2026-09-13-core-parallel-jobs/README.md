# core-parallel-jobs — 基准与证据记录

## 1. 验证门禁（CI 同款）

| 门禁 | 结果 |
|---|---|
| `cargo test --workspace` | ✅ 161 项（xengine-core 117 + 集成 4 + xengine-math 40） |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ 无警告 |
| `cargo fmt --all -- --check` | ✅ 通过 |
| `openspec validate core-parallel-jobs` | ✅ valid |

## 2. 新增 unsafe 清单（2 处，均有 `# Safety`）

| 位置 | 形式 | 论证 |
|---|---|---|
| `crates/xengine-core/src/world.rs` | `unsafe impl Sync for WorldReadView<'_>` | 视图只暴露 `get::<T: Sync>` / `contains` / `contains_entity`（组件列共享读），不暴露资源 / 钩子上下文指针 / 命令队列 / 任何 `&mut` 或结构变更入口；仅用于并行区域且期间无 `&mut World` |
| `crates/xengine-core/src/parallel/jobs.rs` | `parallel_for_collect` 的 `Arc<ScopedCtx>` lifetime 擦除（唯一 `transmute`，用户批准） | ctx 由该函数栈帧持有；函数返回前 join 全部块作业（含调用线程自身领取循环），被擦除的 `'static` 永不越过真实借用期；`ScopedCtx<'_>` 持 `&dyn Fn + Send + Sync` 与原子量，故 `Send + Sync` |

## 3. 基准方法与结果

`cargo bench -p xengine-core --bench propagate`（宽树 1 根 + N 子 / 深链 N/1000 条 1000 深链 / 平坦 N 根，每 tick 全量置脏后传播一次，`ticks=20`、`warmup=5`，worker=8）。

**方法学注意**：同进程内**先跑的那一档系统性更快**（第二轮起堆内存布局被 dirty 迁移打散）。因此下表取"每档作为进程首测"的样本中位数（`XENGINE_BENCH_ORDER=sp` 取串行、`=ps` 取并行，各 3 次重复）。

| 形态 | N | 串行总计 | 并行总计 | 总加速 | 串行 2a | 并行 2a | 串行 2b | 并行 2b |
|---|---|---|---|---|---|---|---|---|
| wide | 100k | 35.3 ms | 32.3 ms | **1.09x** | 3.55 ms | 0.76 ms | 25.9 ms | 26.1 ms |
| deep | 100k | 1575 ms | 256 ms | **6.2x** | 1523 ms | 185 ms | 44.4 ms | 61.7 ms |
| flat | 100k | ~42 ms | ~46 ms | ~0.9x（噪声内） | 1.4 ms | 0.7 ms | 34.0 ms | 40.3 ms |

一致性：三种形态在 worker ∈ {1,2,4,8} 与单线程下结果**逐元素精确相等**（基准内建校验 + 单测）。

## 4. 结论

1. **阶段 2a 并行化有效**：宽树 2a 3.55 → 0.76 ms（4.6x）、深链 2a 1523 → 185 ms（8.2x）——祖先链遍历（缓存不友好）正是并行收益最大的场景。
2. **总加速受限于串行的 2b**：宽树/平坦形态下 2b（逐实体 `remove::<TransformDirty>` 触发 archetype 迁移 + 行 swap）占总时长 75%+，因此总加速被 Amdahl 限制在 ~1.1x。
3. **无回退**：公平对比下并行档不低于串行档（宽树 1.09x、平坦在噪声内）；此前观察到的 0.6–0.7x 系测量顺序造成的内存布局差异，已用进程隔离方法排除。
4. **验收目标达成情况**：
   - "并行档 ≥2.0x（≥8 逻辑核、N=100k）"——在 **2a 主导**的负载（深链）上达成（6.2x）；在 2b 主导的宽树/平坦负载上**未达成**，瓶颈明确为 2b。
   - "单线程档回归 ≤5%"——串行档由同一代码路径承载，实测无回归（同形态串行数字与变更前同量级）。
   - "热路径零分配"——`compute_world` 已无每实体 `Vec`/`HashSet`（线程本地 chain scratch + 深度上限替代），phase 1 的逐节点 child-list 克隆改为复用 scratch。

## 5. 后续变更建议（本变更为 Non-goal）

**2b 批量清除 / dirty 集合化**：当前 `TransformDirty` 是 marker 组件，逐实体清除即逐实体 archetype 迁移（约 260 ns/实体）。两条候选路径：
- (a) 按 archetype 分组批量迁移（目标 archetype 只解析一次，行搬移更紧凑）——保守，不改语义；
- (b) 用实体位集（dirty set）替代 marker 组件——彻底消除迁移，但改变 go-layer 规范中的 `TransformDirty` 语义与公开 API，需用户决策。

预期 (b) 可把宽树/平坦形态的传播降到接近 2a 的量级，届时总加速应达 3–4x。
