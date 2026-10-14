#!/usr/bin/env bash
# 目录卫生检查：报告三处"规划外"条目（根目录 / .rds 顶层 / tools/）。
#
# 背景：开发过程产物（验证日志、一次性脚本、diff 输出、截图、反汇编）容易随手落在
#   根目录、.rds 顶层、tools/ 里——gitignore 只挡"不入库"，挡不住"落在哪"。
#   本脚本按白名单扫三处，任何偏离都报告："乱跑"第一时间被看见。
#   白名单与 data-paths.md（运行时目录）和 tools 归档约定一一对应；改约定先改文档再改这里。
#
# 用法：
#   tools/hygiene-check.sh          # 只报告（默认 dry-run）
#   tools/hygiene-check.sh --yes    # 把偏离项移入 .rds/tmp（两阶段删除：你确认后一次删光）
#
# 安全边界：只碰**规划外**条目；白名单内的一律不动。目标目录本身不动。
set -uo pipefail

APPLY=0
for arg in "$@"; do
    case "$arg" in
        --yes) APPLY=1 ;;
        -h|--help) sed -n '1,20p' "$0"; exit 0 ;;
        *) echo "未知参数：$arg（--help 看用法）" >&2; exit 2 ;;
    esac
done

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
STAGE_DIR="$REPO_ROOT/.rds/tmp"
MOVED=0
TOTAL=0

# ---- 白名单（与 data-paths.md §目录 一致；改文档后同步这里） ----
ROOT_FILES="AGENTS.md .gitignore .gitattributes Cargo.toml Cargo.lock LICENSE NOTICE README.md README.en.md rust-toolchain.toml rust-analyzer.toml"
ROOT_DIRS=".agents .cargo .delta .github assets crates dist docs examples target third_party tools vendor v1 .rds"
RDS_DIRS="config data extensions logs plugin-cache plugin-data plugins tmp .migrated-from-legacy"
# tools/ 白名单 = 可复用工具；hygiene-check.sh 自己也在里面
# 集成测试工具链（2026-10-14 归位）：run-*/launch-*/poll-* + 校验/分析/清理脚本，见 docs/reports/README.md
TOOLS_FILES="add_m1_tests.py build-instance-project.py check_logs_encoding.py clean-temp.sh cleanup-test-residue.ps1 count-warnings.ps1 diff-test-runs.ps1 extract-test-pairs.ps1 fetch-duckdb.sh gen_mock_generator_catalog.py hygiene-check.sh install-linux-deps.sh launch-check.ps1 launch-integration-test.ps1 package-release.sh poll-check.ps1 poll-integration-test.ps1 run-check.cmd run-integration-test.cmd scan-fffd.py target-guard.sh test-per-pkg.ps1 verify-instance-project.py verify-integration-log.ps1 zip-dir.ps1"
TOOLS_DIRS="_archive"

# 检查一处；参数：<标签> <路径> <白名单(空格分隔)> 是文件还是目录
check_entries() {
    local label="$1" dir="$2" allow="$3" kind="$4"
    [ -d "$dir" ] || { echo "跳过（不存在）：$dir"; return; }
    local hits=0
    for path in "$dir"/*; do
        [ -e "$path" ] || continue
        local name
        name="$(basename "$path")"
        # 隐藏文件单独处理（glob 不含点开头）
        case "$name" in .*) continue ;; esac
        if [ "$kind" = "file" ] && [ ! -f "$path" ]; then continue; fi
        if [ "$kind" = "dir" ] && [ ! -d "$path" ]; then continue; fi
        if echo "$allow" | grep -qw -- "$name"; then
            continue
        fi
        hits=$((hits + 1))
        TOTAL=$((TOTAL + 1))
        if [ "$APPLY" -eq 1 ]; then
            mkdir -p "$STAGE_DIR"
            if mv -- "$path" "$STAGE_DIR/" 2>/dev/null; then
                echo "  [移入 tmp] $label/$name"
                MOVED=$((MOVED + 1))
            else
                echo "  [失败] $label/$name"
            fi
        else
            echo "  [偏离] $label/$name"
        fi
    done
    if [ "$hits" -eq 0 ]; then
        echo "  ✓ 干净"
    fi
}

echo "目录卫生检查：$(date '+%F %T')"
echo "根：$REPO_ROOT"
echo

echo "── 根目录文件（应只含标准文件）"
check_entries "根" "$REPO_ROOT" "$ROOT_FILES" file
echo "── 根目录目录（应只含规划目录）"
check_entries "根" "$REPO_ROOT" "$ROOT_DIRS" dir
echo "── .rds 顶层（应只含 data-paths.md 规划的目录）"
check_entries ".rds" "$REPO_ROOT/.rds" "$RDS_DIRS" dir
echo "── tools/（应只含白名单工具 + _archive）"
check_entries "tools" "$REPO_ROOT/tools" "$TOOLS_FILES" file
check_entries "tools" "$REPO_ROOT/tools" "$TOOLS_DIRS" dir

echo
if [ "$APPLY" -eq 1 ]; then
    echo "已移入 $STAGE_DIR：$MOVED 个条目（确认后全删即可）"
else
    echo "发现偏离 $TOTAL 个（未动）。确认后执行：$0 --yes（移入 .rds/tmp 由你一次删光）"
fi
