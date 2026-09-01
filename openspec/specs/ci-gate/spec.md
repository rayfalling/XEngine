# ci-gate Specification

## Purpose
TBD - created by archiving change dev-ci. Update Purpose after archive.
## Requirements
### Requirement: 触发器
CI 工作流 SHALL 在 `pull_request`（创建/更新任何 MR）与 `push(main)`（受保护分支推送）时运行；触发器字段 SHALL 精确匹配这两个事件。

#### Scenario: MR 打开触发
- **WHEN** 创建或更新 pull_request
- **THEN** CI 工作流开始运行，检查结果返回至 MR 状态

#### Scenario: main 推送触发
- **WHEN** 向 main 分支推送提交
- **THEN** CI 工作流运行，作为持续回归检查

### Requirement: 平台矩阵
CI SHALL 在 `ubuntu-latest` 与 `windows-latest` 两个平台运行全部检查；任一平台失败 MUST 视作整体失败。

#### Scenario: 双平台运行
- **WHEN** MR 触发 CI
- **THEN** ubuntu 与 windows 各运行一次完整检查集

### Requirement: 检查集
CI MUST 运行三项检查：`cargo test --workspace`（全部单测/集成测试通过）、`cargo clippy --workspace --all-targets -- -D warnings`（无警告）、`cargo fmt --all -- --check`（格式一致）。任一检查失败 MUST 导致对应 job 失败。

#### Scenario: 测试失败阻塞
- **WHEN** 某平台 cargo test 失败
- **THEN** 该平台 job 失败，MR 无法合入

#### Scenario: clippy 警告视为错误
- **WHEN** clippy 产生任何警告
- **THEN** job 失败（-D warnings）

### Requirement: 运行缓存
CI SHALL 使用 cargo 缓存（rust-toolchain 组件 + cargo 注册表/构建缓存，如 `Swatinem/rust-cache`）加速后续运行；缓存 MUST NOT 影响检查结果正确性。

#### Scenario: 二次运行命中缓存
- **WHEN** 相同依赖树再次运行
- **THEN** 命中缓存并显著缩短编译时间，且检查结果一致

### Requirement: 最小权限
workflow SHALL 声明 `permissions: contents: read`；不得请求写入权限。

#### Scenario: 权限收敛
- **WHEN** workflow 运行
- **THEN** 仅持有仓库内容读取权限

### Requirement: 合并门禁与自动合入
MR 合并条件 MUST 为矩阵全部通过；`main` 分支 SHALL 启用 required status checks（两矩阵 job 名）；`auto-merge` SHALL 开启（CI 全绿后自动合入）。检查未通过时 MUST 禁止合并。

#### Scenario: 未全绿不可合并
- **WHEN** 任一矩阵 job 未通过
- **THEN** MR 处于不可合并状态（required checks 未满足）

#### Scenario: 全绿自动合入
- **WHEN** 全部矩阵 job 通过且变更已归档
- **THEN** MR 自动合入 main（auto-merge 生效）

### Requirement: 公开仓库零成本
CI 使用 GitHub 标准托管 runner；公开仓库标准 runner MUST 免费，不产生计费。

#### Scenario: 免费验证
- **WHEN** 工作流在公开仓库运行
- **THEN** 无费用产生（标准 runner）

