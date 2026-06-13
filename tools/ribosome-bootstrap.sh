#!/bin/bash
# ribosome-bootstrap.sh — LysineOS 一键引导脚本
# 从 WSL2 宿主系统出发，编译 ribosome -> 下载源码 -> bootstrap 构建 LFS 基础系统
#
# 用法:
#   ./tools/ribosome-bootstrap.sh              # 全流程
#   ./tools/ribosome-bootstrap.sh --phase cross-toolchain  # 只构建交叉工具链
#   ./tools/ribosome-bootstrap.sh --fetch-only  # 只下载源码
#   ./tools/ribosome-bootstrap.sh --test        # 构建 + QEMU 测试

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# 默认配置
BOOTSTRAP_BASE="${BOOTSTRAP_BASE:-/var/ribosome/bootstrap}"
BUILD_ROOT="${BUILD_ROOT:-$BOOTSTRAP_BASE/build}"
CACHE_DIR="${CACHE_DIR:-$BOOTSTRAP_BASE/cache}"
NUCLEUS_DIR="${NUCLEUS_DIR:-$PROJECT_ROOT/nucleus/core}"
RIBOSOME_BIN="${RIBOSOME_BIN:-$PROJECT_ROOT/ribosome/target/debug/ribosome-cli}"
JOBS="${JOBS:-$(nproc)}"
CONTINUE_ON_ERROR="${CONTINUE_ON_ERROR:-false}"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC} $1"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }
phase() { echo -e "\n${CYAN}=== $1 ===${NC}"; }

# 解析参数
PHASE=""
FETCH_ONLY=false
RUN_TEST=false
COMPILE_ONLY=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --phase)
            PHASE="$2"
            shift 2
            ;;
        --fetch-only)
            FETCH_ONLY=true
            shift
            ;;
        --test)
            RUN_TEST=true
            shift
            ;;
        --compile-only)
            COMPILE_ONLY=true
            shift
            ;;
        --continue-on-error)
            CONTINUE_ON_ERROR=true
            shift
            ;;
        --help|-h)
            echo "LysineOS Bootstrap 引导脚本"
            echo ""
            echo "用法: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --phase <phase>         只构建指定阶段 (cross-toolchain|temp-tools|base-system|kernel)"
            echo "  --fetch-only            只下载源码不构建"
            echo "  --compile-only          只编译 ribosome 不运行 bootstrap"
            echo "  --test                  构建完成后运行 QEMU 测试"
            echo "  --continue-on-error     遇到错误继续构建"
            echo "  --help                  显示帮助信息"
            echo ""
            echo "环境变量:"
            echo "  BOOTSTRAP_BASE    基础目录 (默认: /var/ribosome/bootstrap)"
            echo "  NUCLEUS_DIR       mRNA 文件目录 (默认: nucleus/core)"
            echo "  JOBS              并行任务数 (默认: \$(nproc))"
            exit 0
            ;;
        *)
            error "Unknown option: $1. Use --help for usage."
            ;;
    esac
done

# -------------------------------------------------------
# Phase 0: 环境检查
# -------------------------------------------------------
check_environment() {
    phase "Phase 0: 环境检查"

    # 检查 Rust 工具链
    if ! command -v cargo &> /dev/null; then
        error "cargo not found. Install Rust: https://rustup.rs/"
    fi
    info "Rust toolchain: $(rustc --version)"

    # 检查构建工具
    local missing=()
    for cmd in gcc make patch diffutils; do
        if ! command -v "$cmd" &> /dev/null; then
            missing+=("$cmd")
        fi
    done

    if [[ ${#missing[@]} -gt 0 ]]; then
        warn "Missing build tools: ${missing[*]}"
        warn "Install: sudo apt install build-essential patch diffutils"
    fi

    # 检查磁盘空间 (至少 30GB)
    local available
    available=$(df -BG "$BOOTSTRAP_BASE" 2>/dev/null | awk 'NR==2{print $4}' | tr -d 'G')
    if [[ -n "$available" ]] && (( available < 30 )); then
        warn "Disk space low: ${available}GB available (recommend 30GB+)"
    else
        info "Disk space OK: ${available:-?}GB available"
    fi

    info "Bootstrap base: $BOOTSTRAP_BASE"
    info "Nucleus dir:    $NUCLEUS_DIR"
    info "Jobs:           $JOBS"
}

# -------------------------------------------------------
# Phase 1: 编译 Ribosome
# -------------------------------------------------------
compile_ribosome() {
    phase "Phase 1: 编译 Ribosome"

    if [[ -x "$RIBOSOME_BIN" ]]; then
        info "Ribosome binary already exists: $RIBOSOME_BIN"
        info "Recompile with: --compile-only"
        if [[ "$COMPILE_ONLY" != "true" ]]; then
            return 0
        fi
    fi

    cd "$PROJECT_ROOT/ribosome"

    info "Compiling ribosome (debug profile)..."
    cargo build --bin ribosome-cli 2>&1 | tail -5

    if [[ ! -x "$RIBOSOME_BIN" ]]; then
        error "Ribosome compilation failed"
    fi

    info "Ribosome compiled: $($RIBOSOME_BIN --version 2>/dev/null || echo 'unknown version')"
}

# -------------------------------------------------------
# Phase 2: 下载源码
# -------------------------------------------------------
fetch_sources() {
    phase "Phase 2: 下载源码"

    mkdir -p "$CACHE_DIR"

    info "Fetching sources from $NUCLEUS_DIR..."
    "$RIBOSOME_BIN" fetch "$NUCLEUS_DIR" --cache-dir "$CACHE_DIR"

    info "Source fetch complete"
}

# -------------------------------------------------------
# Phase 3: Bootstrap 构建
# -------------------------------------------------------
run_bootstrap() {
    phase "Phase 3: Bootstrap 构建"

    mkdir -p "$BUILD_ROOT" "$CACHE_DIR"

    local args=(
        --nucleus-dir "$NUCLEUS_DIR"
        --build-root "$BUILD_ROOT"
        --cache-dir "$CACHE_DIR"
    )

    if [[ "$CONTINUE_ON_ERROR" == "true" ]]; then
        args+=(--continue-on-error)
    fi

    if [[ -n "$PHASE" ]]; then
        args+=(--phase "$PHASE")
        info "Running bootstrap phase: $PHASE"
    else
        info "Running full bootstrap (all phases)"
    fi

    "$RIBOSOME_BIN" bootstrap "${args[@]}"

    info "Bootstrap complete!"
}

# -------------------------------------------------------
# Phase 4: 验证
# -------------------------------------------------------
verify_result() {
    phase "Phase 4: 验证"

    local sysroot="$BOOTSTRAP_BASE/sysroot"

    # 检查 sysroot 是否存在
    if [[ -d "$sysroot" ]]; then
        info "Sysroot exists: $sysroot"

        # 检查关键文件
        local checks=0
        local passed=0

        checks+=1
        if [[ -d "$sysroot/usr" ]]; then
            info "  [OK] /usr directory exists"
            passed+=1
        else
            warn "  [MISSING] /usr directory"
        fi

        checks+=1
        if [[ -L "$sysroot/bin" ]]; then
            info "  [OK] /bin -> usr/bin symlink"
            passed+=1
        else
            warn "  [MISSING] /bin symlink"
        fi

        checks+=1
        if [[ -L "$sysroot/lib" ]]; then
            info "  [OK] /lib -> usr/lib symlink"
            passed+=1
        else
            warn "  [MISSING] /lib symlink"
        fi

        checks+=1
        if [[ -L "$sysroot/tools" ]]; then
            info "  [OK] /tools symlink exists"
            passed+=1
        else
            warn "  [MISSING] /tools symlink"
        fi

        checks+=1
        if [[ -d "$sysroot/etc" ]]; then
            info "  [OK] /etc directory exists"
            passed+=1
        else
            warn "  [MISSING] /etc directory"
        fi

        info "Verification: $passed/$checks checks passed"
    else
        warn "Sysroot not found at $sysroot"
        warn "This is expected if only cross-toolchain/temp-tools phases were built"
    fi

    # 检查交叉工具链
    local tools="$BOOTSTRAP_BASE/tools"
    if [[ -d "$tools" ]] && [[ -d "$tools/bin" ]]; then
        local tool_count
        tool_count=$(find "$tools/bin" -type f 2>/dev/null | wc -l)
        info "Cross-toolchain: $tool_count tools in $tools/bin"
    else
        warn "Cross-toolchain not found at $tools"
    fi
}

# -------------------------------------------------------
# Phase 5: QEMU 测试
# -------------------------------------------------------
run_qemu_test() {
    phase "Phase 5: QEMU 测试"

    local sysroot="$BOOTSTRAP_BASE/sysroot"
    local kernel=""
    for f in \
        "$BUILD_ROOT"/linux-kernel-*/pkg/boot/vmlinuz-* \
        "$BUILD_ROOT"/linux-kernel-*/src/arch/x86/boot/bzImage; do
        if [[ -f "$f" ]]; then
            kernel="$f"
            break
        fi
    done

    if [[ -z "$kernel" ]]; then
        warn "Kernel image not found in $BUILD_ROOT (searched vmlinuz-* and bzImage)"
        warn "Skipping QEMU test (kernel phase may not have been built)"
        return 0
    fi

    info "Kernel image found: $kernel"

    if ! command -v qemu-system-x86_64 &> /dev/null; then
        warn "qemu-system-x86_64 not found, skipping QEMU test"
        warn "Install: sudo apt install qemu-system-x86"
        return 0
    fi

    "$SCRIPT_DIR/qemu-test.sh" test
}

# -------------------------------------------------------
# 主流程
# -------------------------------------------------------
main() {
    echo -e "${CYAN}"
    echo "  ╔═══════════════════════════════════════════╗"
    echo "  ║      LysineOS Bootstrap 引导脚本          ║"
    echo "  ║      Ribosome Build System                ║"
    echo "  ╚═══════════════════════════════════════════╝"
    echo -e "${NC}"

    check_environment
    compile_ribosome

    if [[ "$COMPILE_ONLY" == "true" ]]; then
        info "Compile-only mode, exiting"
        exit 0
    fi

    fetch_sources

    if [[ "$FETCH_ONLY" == "true" ]]; then
        info "Fetch-only mode, exiting"
        exit 0
    fi

    run_bootstrap
    verify_result

    if [[ "$RUN_TEST" == "true" ]]; then
        run_qemu_test
    fi

    echo ""
    echo -e "${GREEN}All done!${NC}"
    echo ""
    echo "Next steps:"
    echo "  1. chroot into sysroot: sudo chroot $BOOTSTRAP_BASE/sysroot /bin/bash"
    echo "  2. Run QEMU test:       $SCRIPT_DIR/qemu-test.sh test"
    echo "  3. Build remaining pkgs: $RIBOSOME_BIN bootstrap --phase base-system"
}

main "$@"
