# ribosome-store: 内容寻址存储设计

> 生物学隐喻：**vacuole**（液泡）是细胞中储存物质的结构。在 LysineOS 中，vacuole 是本地内容寻址存储，缓存构建产物和源码。

## 概述

`ribosome-store` 是 Ribosome 构建系统的内容寻址存储（Content-Addressable Storage, CAS）组件。它为构建系统提供三个核心能力：

1. **去重缓存**：相同内容的对象只存储一份，避免重复下载和重复构建
2. **可复现构建验证**：相同输入产生相同哈希，二次构建对比验证可复现性
3. **防篡改安全**：SHA-256 内容寻址，任何篡改都会导致哈希不匹配

## 存储模型

### 磁盘布局

```
/var/ribosome/vacuole/
├── objects/                          # Git-style 分片 blob 存储
│   ├── b9/                           # SHA-256 前 2 字符分片（共 256 个目录）
│   │   └── 4d27b9934d3e08a52e...    # 剩余 62 字符为文件名
│   ├── 3e/
│   └── ...
└── refs/                             # 命名引用（GC roots）
    ├── packages/                     # .prot 包引用
    │   └── gcc-14.2.0-1-x86_64     # 文件内容为对象 SHA-256
    └── sources/                      # 源码 tarball 引用
        └── gcc-14.2.0.tar.xz       # 文件内容为对象 SHA-256
```

### 对象寻址

每个对象以其 SHA-256 哈希为唯一标识：

```
SHA-256("hello world") = b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
                         ^                                               ^
                         shard (b9)                                      file name
路径: objects/b9/4d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
```

分片策略（前 2 字符 = 256 个目录）平衡了目录深度和单目录文件数。对于 LysineOS 核心约 80 个包的场景，每个分片平均不到 1 个文件；即使增长到 10000 个对象，每个分片也仅约 40 个文件。

### 引用机制

引用（refs）是 GC roots -- 任何被引用指向的对象都不会被垃圾回收。

| 命名空间 | 格式 | 用途 |
|---------|------|------|
| `packages` | `<name>-<version>-<release>-<arch>` | 已构建的 .prot 包 |
| `sources` | `<filename>` | 已下载的源码 tarball |

引用文件是纯文本，内容为裸 hex 格式的 SHA-256（不带 `sha256:` 前缀）。

## 核心 API

### `Sha256Digest`

类型安全的 SHA-256 摘要，封装 `[u8; 32]`：

```rust
let digest = Sha256Digest::from_bytes(b"hello world");
digest.hex();        // "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
digest.to_prefixed(); // "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
digest.shard();       // "b9"
```

### `VacuoleStore`

核心存储接口：

```rust
let store = VacuoleStore::open(Path::new("/var/ribosome/vacuole"))?;

// 写入
let digest = store.put_bytes(b"content")?;      // 从内存
let digest = store.put_file(&path)?;             // 从文件（流式，不加载到内存）

// 读取
let exists = store.contains(&digest)?;
let handle = store.get(&digest)?;                // Option<ObjectHandle>
let size = store.size(&digest)?;                  // Option<u64>

// 引用
store.add_ref("packages", "gcc-14.2.0-1-x86_64", &digest)?;
store.resolve_ref("packages", "gcc-14.2.0-1-x86_64")?;
store.remove_ref("packages", "gcc-14.2.0-1-x86_64")?;

// 快捷方法
store.add_package_ref("gcc", "14.2.0", 1, "x86_64", &digest)?;
store.add_source_ref("gcc-14.2.0.tar.xz", &digest)?;

// GC
let stats = store.gc()?;
```

## 写入流程

### 原子写入

所有写入操作遵循 **tmp + fsync + rename** 模式：

```
1. 计算内容的 SHA-256 哈希
2. 检查对象是否已存在（幂等）
3. 写入 objects/<shard>/<hash>.tmp.<pid>
4. fsync 确保数据落盘
5. rename 为最终文件名（POSIX 原子操作）
```

如果两个进程同时写入相同哈希，后者的 rename 会遇到 `AlreadyExists` 错误，此时安全地清理 tmp 文件即可。

### put_file 流式写入

对于大文件（如 500MB 的 GCC .prot 包），`put_file` 采用两阶段策略：

1. **Phase 1（哈希）**：流式读取源文件，边读边计算 SHA-256，不将整个文件加载到内存
2. **Phase 2（写入）**：如果对象不存在，重新 seek 到文件开头，流式复制到 CAS

这意味着大文件最多读取两次，但内存占用始终为 O(1)（64 KiB 缓冲区）。

### 幂等性

所有 `put_*` 操作天然幂等：如果对象已存在，直接返回摘要，不做任何 I/O。这使得并行构建安全 -- 多个构建进程可以同时 `put` 相同内容。

## 读取流程

```
1. 从 Sha256Digest 计算 shard 和 file_name
2. 构造路径: objects/<shard>/<file_name>
3. 检查文件是否存在
4. 如果存在，返回 ObjectHandle（可打开文件读取）
```

读取是直接文件 I/O，零开销，不经过任何数据库层。

## 去重策略

### 包级别去重

相同内容的 .prot 包只存储一份。当 `ribosome build` 产出 .prot 包后，调用 `store.put_file(&prot_path)` 将其存入 CAS。如果同一个包被多次构建且内容相同，CAS 中只有一份拷贝。

### 源码级别去重

源码 tarball 下载后调用 `store.put_file(&tarball_path)` 存入 CAS。如果不同机器或不同构建需要相同的源码，直接从 CAS 读取，跳过下载。

### 文件级别去重（预留）

.prot 包的 `META/manifest.txt` 已包含每个文件的 SHA-256 哈希。未来可以将 FILES/ 下的每个文件单独存入 CAS，实现安装时的单文件去重。当前版本暂不实现此功能。

## 垃圾回收

GC 采用 **标记-清除** 算法：

```
1. 标记：遍历 refs/ 下所有引用文件，收集可达的 SHA-256 集合
2. 清除：遍历 objects/ 下所有分片目录
   - 对象在可达集合中 -> 保留
   - 对象不在可达集合中 -> 删除
   - 文件名包含 .tmp -> 删除（清理残留临时文件）
```

GC 是手动触发的（`store.gc()`），不自动运行。典型使用场景：

- `ribosome clean` 时调用
- 缓存空间不足时手动调用
- 定时任务（低峰期运行）

## 并发安全模型

VacuoleStore 设计为多进程安全：

| 场景 | 安全保证 |
|------|---------|
| 多进程同时 `put` 相同内容 | 幂等，最终只有一个对象文件 |
| 多进程同时 `put` 不同内容 | 不同路径，无冲突 |
| 一个进程 `put`，另一个进程 `get` | `get` 要么看到旧状态（不存在），要么看到新状态（完整文件），不会看到半写状态 |
| 一个进程 `gc`，另一个进程 `put` | GC 可能在 put 完成前删除对象。建议在构建空闲时运行 GC |

原子性由 POSIX `rename` 系统调用保证：目标路径要么不存在，要么指向完整的对象文件。

## 公共哈希函数

`ribosome-store` 提供统一的 `hash_file()` 函数：

```rust
pub fn hash_file(path: &Path) -> Result<String>;  // 返回 "sha256:<hex>"
```

此函数替代了之前在 `ribosome-package` 和 `ribosome-repository` 中的重复实现。其他 crate 应通过 `ribosome-store` 使用此函数。

## 集成点

### ribosome-core（构建引擎）

- `BuildConfig.cache_dir` 对应 vacuole 根目录
- 构建完成后，将 .prot 包通过 `store.put_file()` 存入 CAS
- 通过 `store.add_package_ref()` 添加引用
- 源码下载后通过 `store.put_file()` 缓存

### ribosome-package（打包器）

- `hash_file()` 改用 `ribosome_store::hash_file()`
- 消除内部重复的哈希实现

### lysin（包管理器）

- 安装前通过 `store.resolve_package_ref()` 检查缓存
- 命中时通过 `store.get()` 读取 .prot 包，跳过下载
- 安装后添加本地引用

## 未来演进

以下功能当前不实现，为后续 Phase 预留：

1. **SQLite 查询索引层**：加速反向查询（"哪些包包含文件 X"）、LRU 时间戳
2. **LRU 缓存驱逐**：基于访问时间的自动淘汰，配合缓存大小限制
3. **仓库级 CAS 同步**：本地 vacuole 与远程 nucleus 之间的增量同步
4. **文件级去重**：将 .prot 包内文件单独存入 CAS，实现安装级去重
5. **压缩存储**：对象存储时使用 zstd 压缩，减少磁盘占用
