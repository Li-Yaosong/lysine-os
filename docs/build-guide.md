# LysineOS 构建指南

本指南介绍如何搭建 LysineOS 开发环境并构建 Ribosome 构建系统。

---

## 前置条件

- **操作系统**：Ubuntu 22.04+ 或 Debian 12+
- **Docker**（推荐方式）
- **Rust**：1.90+（如不使用 Docker）
- **Git**

---

## 开发环境搭建

### 方案一：Docker 开发容器（推荐）

使用 Docker 容器可以获得完全一致的开发环境，无需在宿主机安装额外依赖。

**前置条件**：安装 [Docker](https://docs.docker.com/get-docker/) 和 [VS Code](https://code.visualstudio.com/) 或 [Cursor](https://cursor.sh/)。

#### 使用 VS Code / Cursor Dev Container

1. 安装扩展：`ms-vscode-remote.remote-containers`
2. 在 VS Code / Cursor 中打开项目根目录
3. 按 `F1`，选择 `Dev Containers: Reopen in Container`
4. 等待容器构建完成（首次约 3-5 分钟）
5. 容器内已包含完整的 Rust 工具链和开发依赖

#### 手动使用 Docker

```bash
# 构建开发镜像
docker build -t lysine-dev -f .devcontainer/Dockerfile .

# 运行容器（挂载项目目录）
docker run --rm -it \
  -v $(pwd):/workspace \
  lysine-dev

# 在容器内构建
cd /workspace/ribosome && cargo build
```

### 方案二：本地安装

使用项目提供的脚本自动安装所有依赖：

```bash
# 运行开发环境搭建脚本
bash scripts/dev-setup.sh
```

该脚本会：
1. 检测操作系统并安装系统包
2. 安装或更新 Rust 工具链（1.90+）
3. 安装 rustfmt、clippy、rust-analyzer 等组件
4. 拉取 Cargo 依赖

**手动安装（Ubuntu/Debian）**：

```bash
# 系统依赖
sudo apt-get update
sudo apt-get install -y \
  curl build-essential pkg-config \
  libarchive-dev libsodium-dev libbtrfs-dev \
  binutils bison gawk m4 texinfo \
  pandoc git jq ripgrep

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
rustup component add rustfmt clippy rust-analyzer rust-src
```

---

## 构建 Ribosome

### 首次构建

```bash
cd ribosome
cargo build
```

### 开发构建（增量编译）

```bash
# 使用 cargo-watch 实现保存即编译
cargo install cargo-watch
cd ribosome
cargo watch -x check
```

### Release 构建

```bash
cd ribosome
cargo build --release
```

构建产物位于 `ribosome/target/release/`，包含两个可执行文件：
- `ribosome` — 构建引擎 CLI
- `lysin` — 包管理器 CLI

---

## 运行测试

```bash
# 运行所有测试
cd ribosome
cargo test --all

# 运行特定 crate 的测试
cargo test -p ribosome-parser
```

---

## 代码质量检查

```bash
# 格式化代码
cd ribosome
cargo fmt --all

# Lint 检查
cargo clippy --all-targets -- -D warnings

# 一键检查（格式化 + lint）
cargo fmt --check --all && cargo clippy --all-targets -- -D warnings
```

---

## 项目结构

```
ribosome/
├── Cargo.toml              # Workspace 根配置
├── crates/
│   ├── ribosome-cli/       # 构建引擎 CLI（bin）
│   ├── ribosome-core/      # 核心构建引擎（lib）
│   ├── ribosome-parser/    # mRNA YAML 解析器（lib）
│   ├── ribosome-deps/      # genome 依赖图引擎（lib）
│   ├── ribosome-sandbox/   # membrane 沙箱管理（lib）
│   ├── ribosome-package/   # .protein 打包/解包（lib）
│   ├── ribosome-repository/# nucleus 仓库管理（lib）
│   ├── ribosome-snapshot/  # mitosis 快照管理（lib）
│   └── lysin/              # 包管理器 CLI（bin）
└── tests/                  # 集成测试
```

---

## 常见问题

### cargo build 失败：找不到 libbtrfs

确保已安装 `libbtrfs-dev`（Ubuntu/Debian）或 `btrfs-progs-devel`（Fedora）。如果使用 Docker 开发容器则无需手动安装。

### Rust 版本过低

```bash
rustup update stable
rustup default stable
```

LysineOS 要求 Rust 1.90 或更高版本。

### Windows 上无法直接构建

本项目依赖 Linux 特有的系统调用（namespace、cgroup、Btrfs 等），Windows 上无法直接构建。请使用 Docker 开发容器或 WSL2。
