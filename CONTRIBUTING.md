# 参与 Promptr 开发

感谢贡献！Promptr 将 DSL、持久化格式和机器输出视为长期接口。修改应保持分层依赖清晰，
同时用可复现的测试证明行为。

## 开发环境

- Rust `1.88` 或更高版本；
- Rust 2024 edition；
- `rustfmt` 与 `clippy` 组件；
- Git。

```bash
rustup toolchain install 1.88.0 --profile minimal --component rustfmt,clippy
rustup override set 1.88.0
cargo build
```

## 修改原则

1. **保持依赖方向。** `domain` 不感知数据库或 UI；`application` 通过端口协调用例；
   `infrastructure` 实现端口；`interface` 负责宿主和呈现；`language` 只承担 DSL 前端。
2. **一个语义路径。** 不要在 CLI、REPL 或 TUI 中复制领域校验或写 SQL。
3. **消除特殊分支。** 优先调整数据模型，让边界情况进入正常路径，而不是堆叠条件判断。
4. **不破坏机器契约。** 变更规范 XML、JSON 字段、诊断代码或退出码前，必须明确记录兼容
   影响并补充契约测试。
5. **聚焦修改。** 不把无关格式整理或重构混入功能补丁。

## 验证

提交前至少运行与 CI 相同的检查：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

测试用于验证、实验和复现，不应把测试专用状态或分支引入生产路径。修复缺陷时，请先添加
能复现问题的最小测试；涉及持久化时同时覆盖成功提交和失败回滚。

## 数据库与配置迁移

- 迁移历史只追加（append-only）：已发布的迁移编号、名称、SQL 和校验和不得改写或复用。
- 新迁移必须可从上一已发布模式升级，并在同一事务中更新迁移账本。
- 不用“删除数据库再试”替代迁移路径；真实用户数据是兼容性边界。
- 配置迁移须保留 TOML 注释、顺序和无关格式，并在改写前生成备份。
- 为旧模式、当前模式和比程序更新的模式分别提供测试与可操作诊断。

## 标准输出契约

标准输出（stdout）是公开机器接口，而不是日志流：

- `--format json` 必须只写一个版本化 JSON 文档；成功和失败都不得前后夹杂其他文本；
- `--format raw` 只允许全部结果均为 `OUTPUT` XML，并只写规范 XML 字节；
- 提示、进度、日志和 human/raw 诊断写标准错误（stderr）；
- 非交互模式不得打开编辑器、确认提示或分页器；
- 事务结果只在提交成功后发布，回滚时丢弃缓冲输出。

任何改变字段名、序列化、换行、XML 转义、诊断代码或退出码的补丁，都应视为兼容性变更，
并包含精确字节或结构断言。

## 提交与合并请求

使用原子提交：一个提交完成一个可独立审阅、可独立验证的意图。采用 Conventional Commits
（约定式提交），例如：

```text
feat(dsl): add reachable fragment search
fix(sqlite): preserve output on successful commit
docs: document raw stdout contract
test(config): reproduce migration comment loss
```

不要提交构建产物、编辑器状态或临时数据库。提交前确认：

```bash
git status --short
git diff --check
```

合并请求（pull request, PR）应说明：

- 问题、明确范围与不在范围内的内容；
- 设计选择及对架构依赖的影响；
- 对 DSL、数据库、配置、JSON/XML 和退出码的兼容性影响；
- 执行过的验证命令及结果；
- 已知风险、后续工作或迁移说明。

若行为规范需要改变，请在同一补丁中更新 `docs/`，必要时新增架构决策记录
（architecture decision record, ADR），不要让实现和规范静默分叉。
