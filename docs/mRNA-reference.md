# mRNA 构建描述文件语法参考

mRNA 是 LysineOS Ribosome 构建系统的核心配置格式，采用声明式 YAML。每个软件包对应一个 mRNA 文件，描述其元数据、依赖、源码、构建步骤和输出。

---

## 文件命名与位置

```
nucleus/{repo-section}/{package-name}/{version}.mRNA
```

示例：
- `nucleus/core/gcc/14.2.0.mRNA`
- `nucleus/devel/rust/1.90.0.mRNA`
- `nucleus/desktop/lysine-compositor/0.1.0.mRNA`

**命名规则**：
- 包名：小写，连字符分隔（kebab-case）
- 版本号：与上游版本号一致
- 文件扩展名：`.mRNA`（大写）

---

## 完整字段规范

### 顶层字段

| 字段 | 类型 | 必需 | 说明 |
|---|---|---|---|
| `api-version` | integer | 是 | mRNA 格式版本，当前为 `1` |
| `name` | string | 是 | 包名（小写，kebab-case） |
| `version` | string | 是 | 上游版本号 |
| `release` | integer | 是 | LysineOS 内部构建版本号（从 1 开始） |
| `description` | string | 是 | 一句话包描述 |
| `homepage` | url | 否 | 上游项目主页 |
| `license` | string | 是 | SPDX 许可证标识 |
| `maintainer` | string | 否 | 维护者信息 |
| `tags` | list | 否 | 标签列表 |

### depends — 依赖声明

| 子字段 | 类型 | 必需 | 说明 |
|---|---|---|---|
| `depends.build` | list | 否 | 构建时依赖 |
| `depends.runtime` | list | 否 | 运行时依赖 |
| `depends.check` | list | 否 | 测试时依赖 |

依赖格式支持版本约束：
```yaml
depends:
  build:
    - binutils >= 2.42
    - glibc >= 2.39
    - gmp
  runtime:
    - glibc >= 2.39
  check:
    - dejagnu
```

- `package` — 无版本约束（任何版本）
- `package >= X.Y` — 最低版本要求
- `package < X.Y` — 最高版本限制

### features — 功能开关

```yaml
features:
  default: [cxx, fortran]      # 默认启用的功能
  options:
    lto:
      description: Link-time optimization
      cflags: -flto=auto
      depends: [libfat-lto]
    go:
      description: Go language support
```

- `default`：默认启用的功能列表
- `options`：所有可选功能，每个功能可包含：
  - `description`：功能描述
  - `depends`：该功能额外需要的依赖
  - `cflags`：启用该功能时追加的编译标志

### sources — 源码获取

```yaml
sources:
  - url: https://ftp.gnu.org/gnu/gcc/gcc-14.2.0/gcc-14.2.0.tar.xz
    hash: sha256:a0b06c...
  - url: https://ftp.gnu.org/gnu/gcc/gcc-14.2.0/gcc-14.2.0.tar.xz.sig
    signature: gpg
    key-id: D39DC0E3
```

| 子字段 | 类型 | 必需 | 说明 |
|---|---|---|---|
| `url` | url | 是 | 源码下载地址 |
| `hash` | string | 推荐 | 格式 `sha256:HEX` |
| `signature` | string | 否 | 签名类型：`gpg` 或 `minisign` |
| `key-id` | string | 否 | 签名公钥 ID |

### patches — 补丁列表

```yaml
patches:
  - fix-build-with-glibc-2.40.patch
  - aarch64-fix.patch:
      condition: arch == "aarch64"
  - CVE-2024-1234.patch:
      severity: critical
```

补丁文件与 mRNA 放在同一目录下。`condition` 支持条件应用。

### build — 构建步骤

```yaml
build:
  prepare: |
    # 准备阶段
    mkdir build && cd build
    ../configure --prefix=/usr
  compile: |
    # 编译阶段
    make -j$(nproc)
  check: |
    # 测试阶段
    make -k check || true
  install: |
    # 安装阶段（必需）
    make DESTDIR="$DESTDIR" install
```

| 阶段 | 必需 | 说明 |
|---|---|---|
| `prepare` | 否 | 解压后、编译前的准备（配置、打补丁等） |
| `compile` | 否 | 编译源码 |
| `check` | 否 | 运行测试 |
| `install` | 是 | 将编译产物安装到 `$DESTDIR` |

### post-install / post-remove — 安装钩子

```yaml
post-install: |
  ldconfig
  systemctl enable myservice

post-remove: |
  ldconfig
```

在包安装到系统后或卸载后执行的命令。

### outputs — 子包拆分

```yaml
outputs:
  main:
    description: GNU Compiler Collection
  lib:
    description: GCC runtime libraries
    files:
      - /usr/lib/libgcc_s.so*
      - /usr/lib/libstdc++.so*
  dev:
    description: GCC development files
    files:
      - /usr/include/c++/**
      - /usr/lib/lib*.a
```

未在 `files` 中列出的文件归入 `main` 包。

---

## 构建脚本内置变量

| 变量 | 说明 | 示例值 |
|---|---|---|
| `$DESTDIR` | 安装目标目录 | `/var/ribosome/build/gcc-14.2.0/pkg` |
| `$SRCDIR` | 源码解压目录 | `/var/ribosome/build/gcc-14.2.0/src` |
| `$BUILDDIR` | 构建目录 | `/var/ribosome/build/gcc-14.2.0/build` |
| `$NPROC` | CPU 核心数 | `16` |
| `$ARCH` | 目标架构 | `x86_64` |
| `$FEATURE_`* | 功能开关（大写） | `$FEATURE_LTO=1` |
| `$CFLAGS` | C 编译器标志 | `-O2 -pipe -march=x86-64-v3` |
| `$CXXFLAGS` | C++ 编译器标志 | `-O2 -pipe` |
| `$LDFLAGS` | 链接器标志 | `-Wl,--as-needed` |
| `$PREFIX` | 安装前缀 | `/usr` |
| `$PKG_NAME` | 当前包名 | `gcc` |
| `$PKG_VERSION` | 当前版本 | `14.2.0` |
| `$PKG_RELEASE` | 内部版本号 | `1` |

---

## 功能开关用法

功能开关允许同一个 mRNA 支持多种构建配置。

### 在构建脚本中判断

```bash
if [ "$FEATURE_LTO" = "1" ]; then
    extra_flags="$extra_flags --enable-lto"
fi
```

### 通过 CLI 控制

```bash
ribosome build gcc --features=lto,go
ribosome build gcc --no-default-features --features=cxx
```

### 在 ribosome.conf 中设置

```toml
[features]
global = ["lto", "strip"]
per-package.gcc = ["dlang", "go"]
```

---

## 子包拆分规则

- 未在任何 output 中声明的文件归入 `main`
- `files` 支持 glob 模式（`*`、`**`）
- 每个 output 生成独立的 `.protein` 包
- 子包包名格式：`{name}-{output}-{version}`（如 `gcc-lib-14.2.0`）

---

## 完整示例

### 简单包：zlib

```yaml
api-version: 1
name: zlib
version: 1.3.1
release: 1

description: Compression library
homepage: https://zlib.net/
license: Zlib

sources:
  - url: https://zlib.net/zlib-1.3.1.tar.xz
    hash: sha256:b3E4D4E4B4E4D4E4D4E4D4E4D4E4D4E4D4E4D4E4D4E4D4E4D4E4D4E4D4E4D4

build:
  prepare: |
    ./configure --prefix=/usr
  compile: |
    make -j$(nproc)
  check: |
    make check
  install: |
    make DESTDIR="$DESTDIR" install
```

### 复杂包：gcc（含功能开关和子包）

```yaml
api-version: 1
name: gcc
version: 14.2.0
release: 1

description: GNU Compiler Collection
homepage: https://gcc.gnu.org/
license: GPL-3.0-or-later

depends:
  build:
    - binutils >= 2.42
    - glibc >= 2.39
    - gmp >= 6.3
    - mpfr >= 4.2
    - mpc >= 1.3
  runtime:
    - glibc >= 2.39
    - binutils >= 2.42

features:
  default: [cxx, fortran, objc]
  options:
    dlang:
      description: D language support
      depends: [gdc]
    go:
      description: Go language support
    lto:
      description: Link-time optimization
      cflags: -flto=auto

sources:
  - url: https://ftp.gnu.org/gnu/gcc/gcc-14.2.0/gcc-14.2.0.tar.xz
    hash: sha256:a0b06c7b2a0e37e2e8b58b7f0c2c9c5c3b1a0d1e2f3a4b5c6d7e8f9a0b1c2d3

patches:
  - fix-build-with-glibc-2.40.patch

build:
  prepare: |
    mkdir -v build && cd build
    ../configure --prefix=/usr \
      --enable-languages=${features// /,} \
      --disable-multilib --disable-werror \
      --with-system-zlib
  compile: |
    make -j$(nproc)
  check: |
    make -j$(nproc) -k check || true
  install: |
    make DESTDIR="$DESTDIR" install
    rm -rf "$DESTDIR/usr/lib/gcc"/*/{include-fixed/bits,install-tools}

post-install: |
  ldconfig

outputs:
  main:
    description: GNU Compiler Collection
  lib:
    description: GCC runtime libraries
    files:
      - /usr/lib/libgcc_s.so*
      - /usr/lib/libstdc++.so*
  dev:
    description: GCC development files
    files:
      - /usr/include/c++/**
      - /usr/lib/lib*.a

maintainer: LysineOS Team <team@lysine-os.org>
tags: [compiler, development, core]
```
