## 1. 工作流定义

- [ ] 1.1 创建 `.github/workflows/ci.yml`：触发（pull_request + push main）、矩阵（ubuntu/windows）、三检查（test/clippy/fmt）、缓存（rust-toolchain + rust-cache）、最小权限（contents: read）
- [ ] 1.2 本地预跑验证：`cargo test`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check` 全部通过（当前 main 基线）

## 2. 门禁落地

- [ ] 2.1 `openspec validate dev-ci` 通过；`openspec archive dev-ci -y` 归档（specs 合并 ci-gate）
- [ ] 2.2 推送 `feat/dev-ci` 分支并创建 MR（关联归档变更）；`gh pr merge --auto` 开启自动合并
- [ ] 2.3 设置 main 分支保护：required checks = `test (ubuntu-latest)`、`test (windows-latest)`（enforce_admins: true）

## 3. 验证与文档

- [ ] 3.1 确认 MR CI 运行并矩阵全绿；确认合并门禁语义（未绿不可合并；绿后 auto-merge 生效）
- [ ] 3.2 文档同步（README 增加 CI 段落；AGENTS.md 质量门禁段落已随变更提交）
