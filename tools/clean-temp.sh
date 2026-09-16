#!/usr/bin/env bash
# 清理测试留下的临时条目。
#
# 背景：各 crate 的测试在 `env::temp_dir()` 下建临时目录 / 文件且不清理（设计如此，
# 见 docs/architecture/runtime/data-paths.md §5——逐个改测试要引 `set_var`，
# Rust 2024 下是 unsafe 且与并行测试争用）。
#
# 现在有两处会积，**策略不同**：
#   1. 仓库内 `<repo>/.rds/tmp`——`cargo test` 已把 TEMP 钉到这里（`.cargo/config.toml`），
#      新垃圾都落这儿。这是**我们自己的 scratch 目录**，所以清它里面的**所有**条目
#      （前缀五花八门：`rds_` / `rd_` / `rdata_`，还有 cargo / rustc 自己的临时文件）。
#   2. 系统临时目录（`$TEMP`）——历史积压，以及不经 cargo 直接跑测试二进制的残留。
#      那是**公共目录**，只碰已知的测试前缀：`rds_*` / `rd_*` / `rdata_*`。
#
# 用法：
#   tools/clean-temp.sh                        # 只报告（默认 dry-run）
#   tools/clean-temp.sh --yes                  # 真删（默认只看 24h 前的）
#   tools/clean-temp.sh --older-than 0 --yes   # 连刚建的也删（先确认没有测试在跑）
#
# 安全边界：只处理目标目录下的**顶层条目**、且早于阈值；目标目录本身一律不动。
# `RdataStation*`（旧版"数据目录不可用"时的回退位置）**不在范围内**：那里可能是真实数据。
set -uo pipefail

HOURS=24
APPLY=0
while [ $# -gt 0 ]; do
    case "$1" in
        --older-than) HOURS="${2:-24}"; shift 2 ;;
        --yes) APPLY=1; shift ;;
        -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
        *) echo "未知参数：$1（--help 看用法）" >&2; exit 2 ;;
    esac
done

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# 目标清单：`<目录>|<模式>`；模式 `all` = 该目录下所有顶层条目，否则空格分隔的 glob
TARGET_SPECS=(
    "$REPO_ROOT/.rds/tmp|all"
    "${TEMP:-${TMP:-/tmp}}|rds_* rd_* rdata_*"
)

CUTOFF=$(( $(date +%s) - HOURS * 3600 ))
GRAND_KB=0
GRAND_REMOVED=0
GRAND_HITS=0

echo "阈值：早于 ${HOURS}h 的条目"
echo

for SPEC in "${TARGET_SPECS[@]}"; do
    TMP_ROOT="${SPEC%%|*}"
    PATTERNS="${SPEC#*|}"

    if [ ! -d "$TMP_ROOT" ]; then
        echo "跳过（不存在）：$TMP_ROOT"
        echo
        continue
    fi

    CANDIDATES=()
    for pattern in $PATTERNS; do
        if [ "$pattern" = "all" ]; then
            globs=("$TMP_ROOT"/*)
        else
            globs=("$TMP_ROOT"/$pattern)
        fi
        for path in "${globs[@]}"; do
            [ -e "$path" ] || continue
            case "$path" in
                "$TMP_ROOT"/*) ;;
                *) continue ;;
            esac
            mtime=$(stat -c %Y "$path" 2>/dev/null) || continue
            [ "$mtime" -lt "$CUTOFF" ] || continue
            CANDIDATES+=("$path")
        done
    done

    echo "目标：$TMP_ROOT"

    if [ "${#CANDIDATES[@]}" -eq 0 ]; then
        echo "  没有需要清理的条目。"
        echo
        continue
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
    GRAND_KB=$((GRAND_KB + TOTAL_KB))
    GRAND_HITS=$((GRAND_HITS + ${#CANDIDATES[@]}))

    if [ "$APPLY" -eq 0 ]; then
        echo "  命中 ${#CANDIDATES[@]} 个，占用约 $((TOTAL_KB / 1024)) MB（未删除）"
        echo
        continue
    fi

    REMOVED=0
    for path in "${CANDIDATES[@]}"; do
        if rm -rf -- "$path"; then
            REMOVED=$((REMOVED + 1))
        else
            echo "  删除失败：$path" >&2
        fi
    done
    GRAND_REMOVED=$((GRAND_REMOVED + REMOVED))
    echo "  已删除 $REMOVED 个条目（约 $((TOTAL_KB / 1024)) MB）"
    echo
done

echo "合计：命中 $GRAND_HITS 个 / 约 $((GRAND_KB / 1024)) MB；已删 $GRAND_REMOVED 个"
if [ "$APPLY" -eq 0 ] && [ "$GRAND_HITS" -gt 0 ]; then
    echo
    echo "以上为预览（未删除）。确认后执行：$0 --older-than "$HOURS" --yes"
fi
