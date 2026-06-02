# Nucleus - LysineOS 软件包仓库

Nucleus 是 LysineOS 的软件包仓库，存放所有 mRNA 构建描述文件。

## 仓库分层

| 目录 | 说明 | 示例 |
|---|---|---|
| `core/` | 核心系统包 | linux-kernel, glibc, systemd, gcc, binutils |
| `devel/` | 开发工具 | rust, python, nodejs, cmake, meson |
| `desktop/` | 桌面环境 | lysine-compositor, lysine-shell, wayland |
| `ai/` | AI 组件 | ollama, whisper-cpp, piper |
| `extra/` | 额外应用 | firefox, vscode, docker |
| `testing/` | 测试中的包 | 新提交或实验性包 |

## 文件命名规范

```
{package-name}/{version}.mRNA
```

示例：`core/gcc/14.2.0.mRNA`

## 当前状态

Phase 0 阶段，仓库目录结构已创建，mRNA 文件将在 Phase 1 开始填充。
