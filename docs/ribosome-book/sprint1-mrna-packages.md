# Sprint 1 mRNA 包选择与规格表

本文档定义 Sprint 1 中 10 个精选 nucleus 包的元数据规格，用于编写 `.mRNA` 文件并验证 `ribosome-parser` 能力覆盖。

**参考**：LFS 13.0 systemd 版章节编号；版本与 URL 对齐上游稳定发布。

---

## 实施顺序（按复杂度递增）

| 顺序 | 包名 | 复杂度 | 主要验证目标 |
|---|---|---|---|
| 1 | linux-api-headers | 简单 | 无依赖、仅 install |
| 2 | zlib | 简单 | 基本字段、单源码、check |
| 3 | bash | 简单 | patches |
| 4 | ncurses | 中等 | outputs（lib/dev） |
| 5 | coreutils | 中等 | 多阶段 build、check |
| 6 | glibc | 复杂 | features、outputs、post-install |
| 7 | binutils | 复杂 | 依赖、签名源、features |
| 8 | openssl | 复杂 | 多 features、多 outputs |
| 9 | gcc | 最复杂 | 全特性组合 |
| 10 | systemd | 复杂 | 大量依赖、features、post-install |

---

## 包规格明细

### 1. linux-api-headers

| 项 | 值 |
|---|---|
| LFS 章节 | Ch. 5 / 内核 API 头（与 LFS 11.x+ 流程对齐） |
| 版本 | 6.18.0（与计划内核版本一致） |
| 路径 | `nucleus/core/linux-api-headers/6.18.0.mRNA` |
| 上游 URL | `https://www.kernel.org/pub/linux/kernel/v6.x/linux-6.18.tar.xz` |
| hash | `sha256:` + 构建前从 kernel.org 校验和页获取 |
| depends.build | — |
| depends.runtime | — |
| mRNA 特性 | 最小顶层字段、`sources`、`build.install` |

---

### 2. zlib

| 项 | 值 |
|---|---|
| LFS 章节 | Ch. 8.6 |
| 版本 | 1.3.1 |
| 路径 | `nucleus/core/zlib/1.3.1.mRNA` |
| 上游 URL | `https://zlib.net/zlib-1.3.1.tar.xz` |
| hash | `sha256:16455bf0addbd0f1241910a512f7e7b72a7aff05932ad9a105eb061e9119bfe1` |
| depends.build | — |
| depends.runtime | — |
| mRNA 特性 | prepare/compile/check/install 四阶段 |

---

### 3. bash

| 项 | 值 |
|---|---|
| LFS 章节 | Ch. 8.2 |
| 版本 | 5.2.37 |
| 路径 | `nucleus/core/bash/5.2.37.mRNA` |
| 上游 URL | `https://ftp.gnu.org/gnu/bash/bash-5.2.37.tar.gz` |
| hash | 构建前从 GNU 镜像校验 |
| patches | `bash-5.2.37-upstream_fixes-1.patch`（示例名，与 mRNA 同目录） |
| depends.build | — |
| depends.runtime | `glibc` |
| mRNA 特性 | `patches` 简单列表 |

---

### 4. ncurses

| 项 | 值 |
|---|---|
| LFS 章节 | Ch. 8.7 |
| 版本 | 6.5 |
| 路径 | `nucleus/core/ncurses/6.5.mRNA` |
| 上游 URL | `https://invisible-mirror.net/archives/ncurses/ncurses-6.5.tar.gz` |
| depends.build | — |
| depends.runtime | `glibc` |
| outputs | `main`、`lib`、`dev`（头文件与静态库拆分） |
| mRNA 特性 | `outputs` + glob `files` |

---

### 5. coreutils

| 项 | 值 |
|---|---|
| LFS 章节 | Ch. 8.5 |
| 版本 | 9.5 |
| 路径 | `nucleus/core/coreutils/9.5.mRNA` |
| 上游 URL | `https://ftp.gnu.org/gnu/coreutils/coreutils-9.5.tar.xz` |
| depends.build | — |
| depends.runtime | `glibc` |
| mRNA 特性 | `build.check` 阶段 |

---

### 6. glibc

| 项 | 值 |
|---|---|
| LFS 章节 | Ch. 8.5 / 工具链 |
| 版本 | 2.39.4 |
| 路径 | `nucleus/core/glibc/2.39.4.mRNA` |
| 上游 URL | `https://ftp.gnu.org/gnu/glibc/glibc-2.39.4.tar.xz` |
| depends.build | `linux-api-headers` |
| depends.runtime | `linux-api-headers` |
| features | `nscd`、`profile` 等可选 |
| outputs | `main`、`dev`、`locale` |
| post-install | `ldconfig` |
| mRNA 特性 | features + outputs + post-install |

---

### 7. binutils

| 项 | 值 |
|---|---|
| LFS 章节 | Ch. 8.18 |
| 版本 | 2.42 |
| 路径 | `nucleus/core/binutils/2.42.mRNA` |
| 上游 URL | `https://ftp.gnu.org/gnu/binutils/binutils-2.42.tar.xz` |
| 第二源（签名） | `.sig` + `signature: gpg` + `key-id` |
| depends.build | `glibc >= 2.39` |
| depends.runtime | `glibc >= 2.39` |
| features | `gold`、`ld` |
| mRNA 特性 | 版本约束依赖、签名元数据 |

---

### 8. openssl

| 项 | 值 |
|---|---|
| LFS 章节 | Ch. 8.12 |
| 版本 | 3.4.0 |
| 路径 | `nucleus/core/openssl/3.4.0.mRNA` |
| 上游 URL | `https://github.com/openssl/openssl/releases/download/openssl-3.4.0/openssl-3.4.0.tar.gz` |
| depends.build | `perl` |
| depends.runtime | `glibc` |
| features | `shared`、`docs` |
| outputs | `main`、`lib`、`dev` |
| mRNA 特性 | 多 options、多 outputs |

---

### 9. gcc

| 项 | 值 |
|---|---|
| LFS 章节 | Ch. 8.19 |
| 版本 | 14.2.0 |
| 路径 | `nucleus/core/gcc/14.2.0.mRNA` |
| 上游 URL | `https://ftp.gnu.org/gnu/gcc/gcc-14.2.0/gcc-14.2.0.tar.xz` |
| depends.build | `binutils >= 2.42`、`glibc >= 2.39`、`gmp`、`mpfr`、`mpc` |
| depends.runtime | `glibc >= 2.39`、`binutils >= 2.42` |
| features | `cxx`、`fortran`、`objc`、`lto`、`go` |
| outputs | `main`、`lib`、`dev` |
| patches | glibc 兼容补丁（示例） |
| post-install | `ldconfig` |
| mRNA 特性 | **全字段覆盖**（Sprint 1 标杆包） |

---

### 10. systemd

| 项 | 值 |
|---|---|
| LFS 章节 | Ch. 8 / 系统层 |
| 版本 | 257 |
| 路径 | `nucleus/core/systemd/257.mRNA` |
| 上游 URL | `https://github.com/systemd/systemd/archive/v257.tar.gz` |
| depends.build | `glibc`、`gcc`、`openssl`、`python3`、`meson`、`ninja` 等 |
| depends.runtime | `glibc`、`libcap`、`libxcrypt` 等 |
| features | `udev`、`resolved`、`networkd` |
| post-install | 占位脚本（Sprint 1 仅校验语法） |
| mRNA 特性 | 长依赖列表、features、post-install |

---

## mRNA 特性覆盖矩阵

| 特性 | zlib | bash | coreutils | glibc | binutils | gcc | linux-api-headers | ncurses | openssl | systemd |
|---|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| api-version | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| depends + 版本约束 | | ✓ | ✓ | ✓ | ✓ | ✓ | | ✓ | ✓ | ✓ |
| features | | | | ✓ | ✓ | ✓ | | | ✓ | ✓ |
| outputs | | | | ✓ | | ✓ | | ✓ | ✓ | |
| patches | | ✓ | | | | ✓ | | | | |
| post-install | | | | ✓ | | ✓ | | | | ✓ |
| sources 签名 | | | | | ✓ | | | | | |
| build.check | ✓ | | ✓ | | | ✓ | | | | |

---

## hash 获取说明

Sprint 1 实现阶段：

1. 对标记「构建前校验」的包，在 WSL2 使用 `curl -L` 下载后执行 `sha256sum`
2. 将结果写入 mRNA 的 `sources[].hash` 字段
3. `ribosome check` 仅验证 **格式**，不联网校验内容（Sprint 2+ 可增加）

---

## 验收

- [ ] 10 个 `.mRNA` 文件位于 `nucleus/core/{name}/{version}.mRNA`
- [ ] `ribosome check nucleus/core/` 全部 `[OK]`
- [ ] `ribosome graph nucleus/core/` 输出无环 DAG 的 DOT
