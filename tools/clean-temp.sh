#!/usr/bin/env bash
# 清理测试留在**系统临时目录**里的 rds_* 目录。
#
# 背景：各 crate 的测试在 `env::temp_dir()` 下建 `rds_*` 目录且不清理（设计如此，
# 见 docs/architecture/runtime/data-paths.md §5，为避免 Rust 2024 的 `set_var` 并发问题
# 不打算逐个改测试）。2026-09-16 实测积压 2581 个 / 1.2 GB，吃的全是系统盘。
#
# 之后 `cargo test` 已把 TEMP 钉到仓库内的 `.rds/tmp`（`.cargo/config.toml`，force），
# 新垃圾不再落这儿；本脚本用于清**历史积压**，以及不经 cargo 跑出来的残留。
#
# 用法：
#   tools/clean-temp.sh                        # 只报告（默认 dry-run）
#   tools/clean-temp.sh --yes                  # 真删（默认只看 24h 前的）
#   tools/clean-temp.sh --older-than 0 --yes   # 连刚建的也删（先确认没有测试在跑）
#
# 安全边界：只处理 $TEMP 下的**顶层条目**（目录与文件都算：测试也会直接建
# `rds_b3_*.db` 这种文件）、名字必须以 `rds_` 开头、且早于阈值；$TEMP 本身、
# 以及其它程序的文件一律不碰。`RdataStation*`（旧版"数据目录不可用"时的回退
# 位置）**不在范围内**：那里可能是真实数据，要清请先自己 `ls` 确认。
set -uo pipefail

HOURS=24
APPLY=0
while [ $# -gt 0 ]; do
    case "$1" in
        --older-than) HOURS="${2:-24}"; shift 2 ;;
        --yes) APPLY=1; shift ;;
        -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
        *) echo "未知参数：$1（--help 看用法）" >&2; exit 2 ;;
    esac
done

TMP_ROOT="${TEMP:-${TMP:-/tmp}}"
if [ ! -d "$TMP_ROOT" ]; then
    echo "临时目录不存在：$TMP_ROOT" >&2
    exit 1
fi

CUTOFF=$(( $(date +%s) - HOURS * 3600 ))
CANDIDATES=()

for path in "$TMP_ROOT"/rds_*; do
    [ -e "$path" ] || continue
    # 双保险：路径必须仍在 $TMP_ROOT 下且以 rds_ 开头（glob 已保证，防手滑改坏）
    case "$path" in
        "$TMP_ROOT"/rds_*) ;;
        *) continue ;;
    esac
    mtime=$(stat -c %Y "$path" 2>/dev/null) || continue
    [ "$mtime" -lt "$CUTOFF" ] || continue
    CANDIDATES+=("$path")
done

echo "临时目录：$TMP_ROOT"
echo "阈值：早于 ${HOURS}h 的 rds_* 条目（目录与文件）"

if [ "${#CANDIDATES[@]}" -eq 0 ]; then
    echo "没有需要清理的目录。"
    exit 0
fi

# 分批统计：路径太多会撞命令行长度上限，200 个一批
TOTAL_KB=0
i=0
while [ "$i" -lt "${#CANDIDATES[@]}" ]; do
    chunk=("${CANDIDATES[@]:i:200}")
    kb=$(du -skc "${chunk[@]}" 2>/dev/null | tail -n 1 | cut -f1)
    TOTAL_KB=$((TOTAL_KB + ${kb:-0}))
    i=$((i + 200))
done

echo "命中：${#CANDIDATES[@]} 个，占用约 $((TOTAL_KB / 1024)) MB"

if [ "$APPLY" -eq 0 ]; then
    echo
    echo "以上为预览（未删除）。确认后执行：$0 --older-than "$HOURS" --yes"
    exit 0
fi

REMOVED=0
for path in "${CANDIDATES[@]}"; do
    if rm -rf -- "$path"; then
        REMOVED=$((REMOVED + 1))
    else
        echo "删除失败：$path" >&2
    fi
done
echo
echo "已删除 $REMOVED 个条目（约 $((TOTAL_KB / 1024)) MB）。"
