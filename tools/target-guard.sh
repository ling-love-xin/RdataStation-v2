#!/usr/bin/env bash
# target/ 体积守门：**到阈值提醒**，`--clean` 顺手删掉可再生的无用文件。
#
# 背景：DuckDB 改动态链接后（docs/architecture/dependencies/duckdb-linking.md）`target/`
# 的最大头不再是 C++ 内核，而是**每个可执行文件的 PDB**（本仓 debug 二进制上百 MB 级，
# 每次链接重写）与**增量编译缓存**。两者都不影响正确性，删了下次构建自然重建。
#
# 用法：
#   tools/target-guard.sh                 # 只报告（超阈值退出码 1，可挂到日常命令后面）
#   tools/target-guard.sh --clean         # 先清无用文件，再报告
#   tools/target-guard.sh --warn-gb 40    # 换阈值（默认 60 GB）
#   tools/target-guard.sh --dry-run       # 与 --clean 同路，但只打印将删什么
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="$ROOT/target"
WARN_GB=60
CLEAN=0
DRY=0

while [ $# -gt 0 ]; do
  case "$1" in
    --clean) CLEAN=1 ;;
    --dry-run) DRY=1 ;;
    --warn-gb) WARN_GB="${2:-60}"; shift ;;
    -h|--help) sed -n '2,16p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "不认识的参数：$1" >&2; exit 2 ;;
  esac
  shift
done

[ -d "$TARGET" ] || { echo "没有 ${TARGET}（还没构建过）"; exit 0; }

# 删除清单：都能再生，且不参与增量判断（删了最多下次多编一点）
DELETABLE_LABELS=("增量编译缓存（incremental）" "调试符号（*.pdb）" "旧的 DuckDB 下载缓存")

# shellcheck disable=SC2317  # 由 --clean 分支调用
do_clean() {
  if [ "$DRY" = "1" ]; then
    echo "将删除（--dry-run）："
  else
    echo "清理可再生的无用文件："
  fi

  # 1) incremental：路径形如 target/<profile>/incremental 或 target/<triple>/<profile>/incremental
  while IFS= read -r dir; do
    [ -n "$dir" ] || continue
    echo "  - $dir"
    [ "$DRY" = "1" ] || rm -rf "$dir"
  done < <(find "$TARGET" -maxdepth 4 -type d -name incremental 2>/dev/null)

  # 2) PDB：本仓 debug 目标每次链接都会重写，删掉不破坏任何产物
  while IFS= read -r pdb; do
    [ -n "$pdb" ] || continue
    size=$(du -sm "$pdb" 2>/dev/null | cut -f1 || echo 0)
    echo "  - ${pdb}（${size} MB）"
    [ "$DRY" = "1" ] || rm -f "$pdb"
  done < <(find "$TARGET" -maxdepth 3 -type f -name '*.pdb' 2>/dev/null)

  # 3) 旧版 libduckdb-sys 的下载缓存（本仓改走 third_party/，留着只是占地方）
  if [ -d "$TARGET/duckdb-download" ]; then
    echo "  - $TARGET/duckdb-download"
    [ "$DRY" = "1" ] || rm -rf "$TARGET/duckdb-download"
  fi
}

[ "$CLEAN" = "1" ] && do_clean

# 报告：总量 + 最大几个子目录（给「下一步删哪」的判断依据）
TOTAL_MB=$(du -sm "$TARGET" 2>/dev/null | cut -f1)
TOTAL_GB=$((TOTAL_MB / 1024))
echo
echo "target/ 体积：${TOTAL_GB} GB（${TOTAL_MB} MB；阈值 ${WARN_GB} GB）"
echo "占用最大的几项："
du -sm "$TARGET"/* 2>/dev/null | sort -rn | head -5 | while read -r size path; do
  printf '  %6s MB  %s\n' "$size" "${path#"$TARGET"/}"
done

if [ "$TOTAL_GB" -ge "$WARN_GB" ]; then
  echo
  echo "⚠️  已到阈值：跑 tools/target-guard.sh --clean 清掉「${DELETABLE_LABELS[*]}」；"
  echo "    仍需更多空间时用 cargo clean -p <crate> 定向重建，最后手段才是 cargo clean。"
  exit 1
fi
