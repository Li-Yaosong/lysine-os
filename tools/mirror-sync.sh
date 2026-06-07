#!/bin/bash
# mirror-sync.sh — 下载所有 mRNA 源码到本地镜像目录
#
# 用法:
#   ./tools/mirror-sync.sh                          # 下载到 ./mirror-sources/
#   ./tools/mirror-sync.sh /path/to/output          # 下载到指定目录
#   ./tools/mirror-sync.sh --dry-run                # 只打印要下载的文件，不实际下载
#
# 下载完成后，可以用任意静态文件服务器托管该目录:
#   python3 -m http.server 8080          # 简单测试
#   nginx / caddy                        # 生产环境
#   GitHub Pages                         # 免费 CDN
#
# 然后设置环境变量使用镜像:
#   export RIBOSOME_MIRROR=http://localhost:8080
#   ribosome fetch nucleus/core/
#
# CI 中:
#   env: RIBOSOME_MIRROR: https://your-mirror.example.com/sources

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

OUTPUT_DIR="${1:-./mirror-sources}"
DRY_RUN=false

if [[ "$OUTPUT_DIR" == "--dry-run" ]]; then
    DRY_RUN=true
    OUTPUT_DIR="./mirror-sources"
fi

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC} $1"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Parse all source URLs from mRNA files
parse_urls() {
    "$PROJECT_ROOT/ribosome/target/release/ribosome-cli" fetch "$PROJECT_ROOT/nucleus/core/" \
        --cache-dir /tmp/ribosome-mirror-sync-cache 2>/dev/null || true

    # Extract URLs with hashes from mRNA files
    grep -r 'url:' "$PROJECT_ROOT/nucleus/core/" --include='*.mRNA' -h \
        | sed 's/.*url: *//' | sed 's/^ *//' | sort -u
}

# Build ribosome if needed
ensure_ribosome() {
    local bin="$PROJECT_ROOT/ribosome/target/release/ribosome-cli"
    if [[ ! -x "$bin" ]]; then
        info "Building ribosome..."
        cd "$PROJECT_ROOT/ribosome" && cargo build --release --bin ribosome-cli
    fi
}

# Extract filename from URL
url_to_filename() {
    local url="$1"
    basename "$(echo "$url" | sed 's/?.*//')"
}

main() {
    echo ""
    echo "  ╔═══════════════════════════════════════════╗"
    echo "  ║      LysineOS Mirror Sync                 ║"
    echo "  ╚═══════════════════════════════════════════╝"
    echo ""

    ensure_ribosome

    mkdir -p "$OUTPUT_DIR"

    # Collect all source URLs from mRNA files
    local urls=()
    while IFS= read -r line; do
        [[ -n "$line" ]] && urls+=("$line")
    done < <(grep -rh 'url:' "$PROJECT_ROOT/nucleus/core/" --include='*.mRNA' \
             | sed 's/^[[:space:]]*url:[[:space:]]*//' | sed 's/[[:space:]]*$//' | sort -u)

    local total=${#urls[@]}
    info "Found $total source URLs in mRNA files"
    info "Output directory: $OUTPUT_DIR"

    if [[ "$DRY_RUN" == "true" ]]; then
        echo ""
        info "Dry run - listing files to download:"
        for url in "${urls[@]}"; do
            local fname
            fname=$(url_to_filename "$url")
            echo "  $fname <- $url"
        done
        exit 0
    fi

    local downloaded=0
    local skipped=0
    local failed=0

    echo ""
    for url in "${urls[@]}"; do
        local fname
        fname=$(url_to_filename "$url")
        local target="$OUTPUT_DIR/$fname"

        # Skip if already downloaded
        if [[ -f "$target" ]]; then
            ((skipped++)) || true
            continue
        fi

        printf "  [%d/%d] %-40s ... " "$((downloaded + skipped + failed + 1))" "$total" "$fname"

        if curl -fsSL --connect-timeout 30 --max-time 600 -o "$target.tmp" "$url" 2>/dev/null; then
            mv "$target.tmp" "$target"
            echo "OK"
            ((downloaded++)) || true
        else
            rm -f "$target.tmp"
            echo "FAILED"
            warn "  Failed: $url"
            ((failed++)) || true
        fi
    done

    echo ""
    info "Mirror sync complete: $downloaded downloaded, $skipped existing, $failed failed"
    info "Files in $OUTPUT_DIR: $(ls "$OUTPUT_DIR" | wc -l)"

    if [[ $failed -gt 0 ]]; then
        warn "Some downloads failed. Re-run to retry."
        exit 1
    fi

    echo ""
    echo "Next steps:"
    echo "  1. Serve the mirror directory:"
    echo "     cd $OUTPUT_DIR && python3 -m http.server 8080"
    echo ""
    echo "  2. Set mirror and fetch:"
    echo "     export RIBOSOME_MIRROR=http://localhost:8080"
    echo "     ribosome fetch nucleus/core/"
    echo ""
    echo "  3. Or upload to your server / GitHub Pages / S3 / etc."
}

main "$@"
