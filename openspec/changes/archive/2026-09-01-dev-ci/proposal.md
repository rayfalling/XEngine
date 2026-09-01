## Why

用户决策：**不再亲自审核代码，只做设计决策与架构规划**；代码质量验证交由 CI 自动化。当前 MR #1 的合入门禁依赖人工检查，需要将"cargo test 全绿 + 归档"固化为 GitHub Actions 机器门禁，并支持 CI 绿后自动合入，实现无人值守的纯净 AI 开发流。公开仓库使用标准 runner 免费（$0）。

## What Changes

- 新增 `.github/workflows/ci.yml`（GitHub Actions）：
  - 触发：`pull_request`（全部 MR）+ `push(main)`（回归保护）
  - 平台矩阵：`ubuntu-latest` + `windows-latest`
  - 检查：`cargo test --workspace`（门禁）+ `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all -- --check`
  - 缓存：`dtolnay/rust-toolchain@stable` + `Swatinem/rust-cache@v2`（cargo 增量）
  - 权限：`contents: read`（最小权限）
- 门禁语义：MR 合并条件 = CI 全绿（矩阵全部通过）+ 变更已归档；**启用 GitHub Auto-merge**（CI 绿后自动合入）；main 分支保护 required checks 指向两矩阵 job
- AGENTS.md / openspec config.yaml 增补"质量与合入（CI 门禁）"治理段

## Capabilities

### New Capabilities
- `ci-gate`: GitHub Actions 自动化验证门禁：触发、平台矩阵、检查集（test/clippy/fmt）、cargo 缓存、MR 自动合入与 main 分支保护

### Modified Capabilities
- 无

## Non-goals

- 发布/部署流水线、bench 自动化（保留本地 cargo bench）
- 覆盖率上报（codecov）与测试基线趋势
- 自托管/Larger runner（免费约束下不必要）
- 多平台编译产物（设备层 C++/ObjC 构建矩阵，待设备层变更）

## Impact

- 新增 `.github/workflows/ci.yml`（workflow 权限最小）
- 仓库治理：MR 合入门禁由人工 → 机器；Auto-merge + 分支保护生效
- 无 crate 依赖变化；CI 成本为 0（公开仓库标准 runner）

## Acceptance Criteria

- 打开/更新 MR 触发 CI；push main 也触发
- 两平台矩阵均运行 test/clippy/fmt 三检查；任一失败 → 检查失败并阻塞合入
- 缓存生效（二次运行命中 cargo registry/target）
- MR 状态：CI 未全绿不可合并；全绿后（auto-merge 开启）自动合入 main
- main 分支保护：required checks = 两个矩阵 job`
