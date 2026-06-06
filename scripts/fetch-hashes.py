#!/usr/bin/env python3
"""
从 mRNA 文件中提取源码 URL，用 curl 下载并计算真实 SHA-256 哈希，替换占位符。

用法:
    python3 scripts/fetch-hashes.py [--dry-run] [--proxy http://host:port]

默认代理: http://172.20.96.1:7890
"""

import hashlib
import os
import re
import subprocess
import sys
from pathlib import Path

PLACEHOLDER = "a" * 64
NUCLEUS_DIR = Path(__file__).parent.parent / "nucleus" / "core"
CACHE_DIR = Path("/tmp/lysine-os-hash-cache-v2")
DEFAULT_PROXY = "http://172.20.96.1:7890"


def find_mrna_files(base_dir: Path) -> list[Path]:
    return sorted(base_dir.rglob("*.mRNA"))


def extract_placeholder_lines(content: str) -> list[tuple[int, str]]:
    results = []
    lines = content.split("\n")
    for i, line in enumerate(lines):
        m = re.match(r'^(\s+- url:\s+)(.+)$', line)
        if m:
            url = m.group(2).strip()
            if i + 1 < len(lines):
                hash_match = re.match(r'^\s+hash:\s+sha256:(.+)$', lines[i + 1])
                if hash_match and hash_match.group(1).strip() == PLACEHOLDER:
                    results.append((i + 1, url))
    return results


def download_and_hash(url: str, proxy: str | None, max_retries: int = 3) -> str | None:
    url_hash = hashlib.md5(url.encode()).hexdigest()[:12]
    filename = url.split("/")[-1]
    cache_file = CACHE_DIR / f"{filename}.{url_hash}.sha256"

    if cache_file.exists():
        cached = cache_file.read_text().strip()
        if len(cached) == 64 and all(c in '0123456789abcdef' for c in cached):
            return cached

    for attempt in range(max_retries):
        try:
            cmd = [
                "curl", "-fsSL",
                "--max-time", "300",
                "--connect-timeout", "30",
                "--retry", "2",
            ]
            if proxy:
                cmd.extend(["--proxy", proxy])
            cmd.append(url)

            result = subprocess.run(cmd, capture_output=True, timeout=600)
            if result.returncode != 0:
                stderr = result.stderr.decode(errors="replace").strip()
                if "404" in stderr or "HTTP 4" in stderr:
                    print(f"  FAIL (HTTP error): {url}", file=sys.stderr)
                    return None
                print(f"  RETRY {attempt+1}/{max_retries}: curl exit {result.returncode}", file=sys.stderr)
                continue

            data = result.stdout
            if len(data) < 100:
                print(f"  WARN: suspiciously small ({len(data)} bytes), retrying...")
                continue

            sha256 = hashlib.sha256(data).hexdigest()
            CACHE_DIR.mkdir(parents=True, exist_ok=True)
            cache_file.write_text(sha256)
            return sha256

        except subprocess.TimeoutExpired:
            print(f"  RETRY {attempt+1}/{max_retries}: timeout", file=sys.stderr)
        except Exception as e:
            print(f"  RETRY {attempt+1}/{max_retries}: {e}", file=sys.stderr)

    print(f"  FAIL after {max_retries} retries: {url}", file=sys.stderr)
    return None


def main():
    args = sys.argv[1:]
    dry_run = "--dry-run" in args

    proxy = DEFAULT_PROXY
    for i, arg in enumerate(args):
        if arg == "--proxy" and i + 1 < len(args):
            proxy = args[i + 1]
        elif arg == "--no-proxy":
            proxy = None

    mrna_files = find_mrna_files(NUCLEUS_DIR)
    print(f"Scanning {len(mrna_files)} mRNA files...")
    print(f"Proxy: {proxy or 'disabled'}")

    if dry_run:
        print("[DRY RUN] No files will be modified.\n")

    work_items = []
    for filepath in mrna_files:
        content = filepath.read_text()
        placeholders = extract_placeholder_lines(content)
        if placeholders:
            work_items.append((filepath, content, placeholders))

    print(f"Files needing hash updates: {len(work_items)}\n")

    if not work_items:
        print("All hashes already filled!")
        return

    updated = 0
    partial = 0
    failed_list = []

    for filepath, content, placeholders in work_items:
        lines = content.split("\n")
        all_ok = True
        any_changed = False

        for line_idx, url in placeholders:
            print(f"  {filepath.name}: {url}")
            sha256 = download_and_hash(url, proxy)

            if sha256 is None:
                all_ok = False
                break

            old_line = lines[line_idx]
            indent_match = re.match(r'^(\s+hash:\s+sha256:)', old_line)
            if indent_match:
                lines[line_idx] = f"{indent_match.group(1)}{sha256}"
                any_changed = True
                print(f"  OK   {sha256[:16]}...")

        if all_ok and any_changed:
            if not dry_run:
                filepath.write_text("\n".join(lines))
            updated += 1
        elif any_changed:
            if not dry_run:
                filepath.write_text("\n".join(lines))
            partial += 1
        else:
            failed_list.append(filepath.name)

    action = "Would update" if dry_run else "Updated"
    print(f"\n{action} {updated} files, partial {partial}, failed {len(failed_list)}")
    if failed_list:
        for f in failed_list:
            print(f"  FAILED: {f}")


if __name__ == "__main__":
    main()
