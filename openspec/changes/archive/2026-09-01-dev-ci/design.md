## Context

仓库为公开仓库（GitHub Actions 标准 runner 免费）；治理决策：用户只做设计/架构决策，代码验证交由 CI（无需人工代码审查）；MR 合入门禁 = CI 全绿 + OpenSpec 变更已归档。当前无任何 CI 配置。

## Goals / Non-Goals

**Goals:**
- `.github/workflows/ci.yml`：pull_request + push(main) 触发；ubuntu/windows 矩阵；test/clippy/fmt 三检查；cargo 缓存；最小权限
- main 分支保护（required checks）+ MR Auto-merge：CI 绿后自动合入
- 治理文档同步（AGENTS.md / config.yaml）

**Non-Goals:**
- 发布流水线、覆盖率、bench 自动化、自托管 runner、设备层编译矩阵

## Decisions

### D1 检查集：test + clippy(-D warnings) + fmt（用户决策 A）
clippy 以警告为错误，fmt 强制一致——质量门禁比"仅 test"严格，防止未格式化/坏味道代码入库；本地同样命令保证可复现。

### D2 平台矩阵：ubuntu + windows（用户决策 A）
核心层跨平台约定（Windows 为首要开发平台，Linux 为 CI/部署平台）；矩阵 `fail-fast: false` 避免一个平台失败取消另一个（信息完整）。

### D3 缓存：`Swatinem/rust-cache@v2` + `dtolnay/rust-toolchain@stable`（用户决策 A）
两个市场标准 action（第三方可信度高、维护活跃）；缓存覆盖 registry + target，二次运行显著提速；缓存 key 变化由 action 自动管理。

### D4 自动合并：`gh pr merge --auto` + 分支保护（用户决策 A）
- 分支保护 PUT `/repos/{owner}/{repo}/branches/main/protection`：`required_status_checks` = 两个矩阵 job 名（`test (ubuntu-latest)`、`test (windows-latest)`）；`enforce_admins: false`（保留用户紧急直推？——按"禁止直推 main"约束取 true 更一致）
- Auto-merge 开启后 CI 全绿自动合并；失败时 MR 保持可审查状态

### D5 触发：pull_request（所有）+ push(main)
非推送分支保护：任何 MR 都必须通过；push main 作为合并后回归防线。

### D6 权限：`permissions: contents: read`
workflow 最小权限；无需 token 写权限。

## Risks / Trade-offs

- [第三方 action 供应链] → 锁定 major 版本（@stable/@v2），仅用于工具链与缓存，无代码执行敏感权限
- [矩阵双倍运行时] → 缓存缓解；公开仓库标准 runner 免费，成本为 0
- [clippy/fmt 首次严格让存量代码失败] → 本变更同时本地预跑三检查并保证当前 main 全绿后入库
- [分支保护与 auto-merge 需仓库管理权限] → owner 身份具备；API 操作失败时回退为手动合并并提示

## Migration Plan

1. 提交 workflow + 治理文档（分支）
2. 本地预跑：`cargo test`、`cargo clippy -D warnings`、`cargo fmt --check` 全绿
3. `openspec validate` → `openspec archive` → 推送分支 → 创建 MR
4. MR CI 运行（workflow 来自分支，可验证执行）；CI 绿 → `gh pr merge --auto` 开启自动合并
5. 设置 main 分支保护（required checks = 矩阵 job 名）
6. 回滚：删除 branch protection 设置 + 关闭 auto-merge 即可

## Open Questions

- 覆盖率（codecov）后续变更引入
- bench 定期自动运行（schedule workflow）后续变更
