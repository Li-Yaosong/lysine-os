# 贡献指南

感谢你对 LysineOS 项目的关注！本文档介绍如何参与项目开发。

---

## 快速开始

1. **Fork 仓库**：在 GitHub 上 fork `lysine-os/lysine-os`
2. **克隆到本地**：`git clone https://github.com/<your-username>/lysine-os.git`
3. **搭建开发环境**：参考 [构建指南](../build-guide.md)
4. **创建功能分支**：从 `main` 创建新分支
5. **开发并提交**：编写代码、测试、提交
6. **提交 PR**：推送到你的 fork，向 `main` 提交 Pull Request

---

## 分支命名规范

| 类型 | 格式 | 示例 |
|---|---|---|
| 功能分支 | `phase/{phase}-{name}` | `phase/0-project-setup` |
| 修复分支 | `fix/{description}` | `fix/gcc-build-error` |
| 文档分支 | `docs/{description}` | `docs/api-reference` |
| 重构分支 | `refactor/{scope}` | `refactor/error-handling` |

详见 [Git 工作流规范](../../.cursor/rules/git-workflow.mdc)。

---

## Commit 消息格式

采用 [Conventional Commits](https://www.conventionalcommits.org/) 规范，使用中文描述：

```
<类型>(<范围>): <简短描述>

[可选的详细描述]

[可选的脚注]
```

### 类型

| 类型 | 说明 |
|---|---|
| `feat` | 新功能 |
| `fix` | Bug 修复 |
| `docs` | 文档更新 |
| `style` | 代码格式（不影响功能） |
| `refactor` | 重构 |
| `test` | 测试 |
| `chore` | 杂项维护 |
| `perf` | 性能优化 |
| `ci` | CI 配置 |

### 范围

| 范围 | 说明 |
|---|---|
| `ribosome` | 构建引擎 |
| `parser` | mRNA 解析器 |
| `deps` | 依赖图引擎 |
| `sandbox` | 构建沙箱 |
| `package` | 打包器 |
| `lysin` | 包管理器 CLI |
| `compositor` | 合成器 |
| `shell` | 桌面 Shell |
| `ai` | AI 引擎 |
| `nucleus` | 软件仓库 |

### 示例

```
feat(parser): 添加 sources 字段签名验证支持

- 支持 GPG 签名验证
- 支持 minisign 签名验证

Closes #123
```

```
fix(sandbox): 修复 membrane 沙箱网络隔离问题

Fixes #456
```

---

## 代码规范

### Rust 代码风格

- 使用 `cargo fmt` 格式化代码，不接受未格式化的提交
- 使用 `cargo clippy` 检查，不允许 clippy 警告
- 公共 API 必须有 `///` 文档注释
- 错误处理使用 `thiserror` 定义错误类型，`anyhow` 用于应用层

### 文档注释

```rust
/// Parses an mRNA build recipe from a YAML string.
///
/// # Arguments
///
/// * `content` - The YAML content of the mRNA file
///
/// # Errors
///
/// Returns `ParserError::InvalidFormat` if the YAML is invalid.
pub fn parse_mrna(content: &str) -> Result<MrnaFile, ParserError> {
    // ...
}
```

### 命名体系

严格遵守项目的生物学隐喻命名，详见 [project.mdc](../../.cursor/rules/project.mdc)。

---

## PR 流程

### 提交前检查清单

- [ ] `cargo fmt --check --all` 通过
- [ ] `cargo clippy --all-targets -- -D warnings` 无警告
- [ ] `cargo test --all` 通过
- [ ] 新增代码有测试覆盖
- [ ] 公共 API 有文档注释
- [ ] commit 消息符合规范

### PR 标题

与 commit 消息格式相同：

```
feat(parser): 添加 sources 字段签名验证支持
```

### PR 描述模板

```markdown
## 概述

<!-- 简要描述此 PR 的目的 -->

## 变更内容

- [ ] 变更 A
- [ ] 变更 B

## 测试

- [ ] 单元测试通过
- [ ] 手动测试场景

## 相关 Issue

Closes #xxx
```

### Review 要求

- CI 检查必须通过（lint + check + test）
- 无未解决的 review 对话
- 至少 1 个 approval（项目初期）

---

## 文档贡献

文档使用中文编写（架构设计、教程、指南），代码注释使用英文。详见 [文档编写规范](../../.cursor/rules/documentation.mdc)。

文档文件放在 `docs/` 目录下，Markdown 格式，遵循以下规则：
- 中英文之间加空格
- 使用全角标点
- 代码块标注语言类型
- 表格前后各空一行

---

## 问题反馈

- 使用 GitHub Issues 报告 Bug 或提出功能请求
- Bug 报告请包含：复现步骤、预期行为、实际行为、环境信息
- 功能请求请描述使用场景和期望效果

---

## 许可证

贡献的代码将以 MIT 许可证发布。提交 PR 即表示你同意将贡献以 MIT 许可证授权给项目。
