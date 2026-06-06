#!/bin/bash
# QEMU/KVM 测试脚本
# 用于自动化验证构建结果是否可引导

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# 默认配置
VM_IMAGE="${VM_IMAGE:-/var/ribosome/test-vm.qcow2}"
VM_KERNEL="${VM_KERNEL:-/var/ribosome/test-kernel}"
VM_ROOTFS="${VM_ROOTFS:-/var/ribosome/test-rootfs}"
VM_MEMORY="${VM_MEMORY:-2G}"
VM_DISK_SIZE="${VM_DISK_SIZE:-20G}"

# 命令行参数
ACTION="${1:-test}"
VERBOSE="${VERBOSE:-false}"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

error() {
    echo -e "${RED}[ERROR]${NC} $1"
    exit 1
}

check_kvm() {
    if [ ! -e /dev/kvm ]; then
        warn "KVM not available (/dev/kvm missing)"
        warn "Tests will run with QEMU software emulation (slower)"
        return 1
    fi
    
    if [ ! -w /dev/kvm ]; then
        warn "No write permission on /dev/kvm"
        warn "Try: sudo chmod 666 /dev/kvm"
        return 1
    fi
    
    info "KVM acceleration available"
    return 0
}

check_qemu() {
    if ! command -v qemu-system-x86_64 &> /dev/null; then
        error "qemu-system-x86_64 not found. Install: sudo apt install qemu-system-x86"
    fi
    
    if ! command -v qemu-img &> /dev/null; then
        error "qemu-img not found. Install: sudo apt install qemu-utils"
    fi
    
    info "QEMU tools available"
}

create_disk_image() {
    info "Creating VM disk image: $VM_IMAGE"
    
    if [ -f "$VM_IMAGE" ]; then
        warn "Disk image already exists, removing..."
        rm -f "$VM_IMAGE"
    fi
    
    qemu-img create -f qcow2 "$VM_IMAGE" "$VM_DISK_SIZE"
    info "Disk image created: $VM_IMAGE ($VM_DISK_SIZE)"
}

prepare_rootfs() {
    info "Preparing minimal rootfs: $VM_ROOTFS"
    
    if [ -d "$VM_ROOTFS" ]; then
        warn "Rootfs directory already exists, removing..."
        rm -rf "$VM_ROOTFS"
    fi
    
    mkdir -p "$VM_ROOTFS"
    
    # 基础目录结构
    mkdir -p "$VM_ROOTFS"/{bin,sbin,lib,lib64,usr/{bin,sbin,lib,libexec},etc,var,proc,sys,dev,tmp,root}
    
    # 简单测试脚本
    echo '#!/bin/sh
echo "LysineOS VM Test Started"
echo "Kernel version: $(uname -r)"
echo "Memory: $(free -m | head -2)"
echo "Disk: $(df -h | head -2)"
echo ""
echo "Test PASSED!"
echo "Shutting down..."
poweroff -f
' > "$VM_ROOTFS/bin/init"
    chmod +x "$VM_ROOTFS/bin/init"
    
    info "Minimal rootfs prepared"
}

run_vm_test() {
    info "Starting VM test..."
    
    local KVM_FLAG=""
    if check_kvm; then
        KVM_FLAG="-enable-kvm"
    fi
    
    # 内核启动参数
    local CMDLINE="root=/dev/sda console=ttyS0 panic=1 init=/bin/init"
    
    info "Booting VM with kernel: $VM_KERNEL"
    info "Kernel cmdline: $CMDLINE"
    
    # 运行 VM
    timeout 60 qemu-system-x86_64 \
        $KVM_FLAG \
        -m "$VM_MEMORY" \
        -cpu host \
        -drive file="$VM_IMAGE",format=qcow2,if=virtio \
        -kernel "$VM_KERNEL" \
        -append "$CMDLINE" \
        -nographic \
        -no-reboot \
        2>&1 | tee /tmp/qemu-test-output.log
    
    # 检查输出
    if grep -q "Test PASSED" /tmp/qemu-test-output.log; then
        info "VM test PASSED!"
        return 0
    else
        error "VM test FAILED - expected 'Test PASSED' in output"
        return 1
    fi
}

run_vm_interactive() {
    info "Starting interactive VM session..."
    
    local KVM_FLAG=""
    if check_kvm; then
        KVM_FLAG="-enable-kvm"
    fi
    
    qemu-system-x86_64 \
        $KVM_FLAG \
        -m "$VM_MEMORY" \
        -cpu host \
        -drive file="$VM_IMAGE",format=qcow2,if=virtio \
        -kernel "$VM_KERNEL" \
        -append "root=/dev/sda console=ttyS0" \
        -nographic
}

show_help() {
    echo "QEMU/KVM 测试脚本 for LysineOS"
    echo ""
    echo "用法: $0 [ACTION]"
    echo ""
    echo "Actions:"
    echo "  test       运行自动化 VM 测试 (默认)"
    echo "  create     创建磁盘镜像和 rootfs"
    echo "  run        交互式运行 VM"
    echo "  check      检查 QEMU/KVM 环境"
    echo "  help       显示帮助信息"
    echo ""
    echo "环境变量:"
    echo "  VM_IMAGE       VM 磁盘镜像路径 (默认: /var/ribosome/test-vm.qcow2)"
    echo "  VM_KERNEL      内核镜像路径 (默认: /var/ribosome/test-kernel)"
    echo "  VM_ROOTFS      rootfs 目录 (默认: /var/ribosome/test-rootfs)"
    echo "  VM_MEMORY      VM 内存大小 (默认: 2G)"
    echo "  VM_DISK_SIZE   磁盘大小 (默认: 20G)"
    echo "  VERBOSE        详细输出 (默认: false)"
}

# 主程序
main() {
    check_qemu
    
    case "$ACTION" in
        test)
            create_disk_image
            prepare_rootfs
            run_vm_test
            ;;
        create)
            create_disk_image
            prepare_rootfs
            info "VM environment created successfully"
            ;;
        run)
            run_vm_interactive
            ;;
        check)
            check_kvm
            check_qemu
            info "Environment check complete"
            ;;
        help|--help|-h)
            show_help
            ;;
        *)
            error "Unknown action: $ACTION. Use 'help' for usage."
            ;;
    esac
}

main "$@"