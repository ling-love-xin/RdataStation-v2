# 驱动插件包 sidecar 一致性验收（sidecar_conformance 换靶子一键跑）
#
# 作用：把 crates/plugin/tests/sidecar_conformance.rs 的被测靶子从 Rust fixture
# 换成 tools/plugin-drivers 的真实插件壳（Python DB-API 壳，独立帧编解码实现），
# 在真实进程边界上跑六条裁决（握手 / 描述 / 大结果 Arrow / 真取消 / EOF 自退 / meta.*）。
#
# 用法（仓库根目录）：
#   powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\run-conformance.ps1            # sqlite（零外部依赖）
#   powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\run-conformance.ps1 -Driver postgresql `
#       -Params '{"host":"10.0.0.5","port":5432,"database":"demo","username":"me","password":"…"}'
#
# pyarrow：大结果 Arrow 附件（第③条）需要被测 Python 环境装了 pyarrow。
# 没有 pyarrow 时第③条会按设计降级为「截断 + notice」并失败——这正是验收要抓的。
# 若系统 pip 不可用，可 -CreateVenv 让脚本在 .rds/tmp/venv 建独立环境装 pyarrow。
#
# 过程产物（wrapper.cmd / venv）落在 .rds/tmp/ 与 $env:TEMP，不入库。

param(
    # 被测驱动：sqlite（默认，零依赖）| postgresql（需真库 + psycopg）
    [string]$Driver = "sqlite",
    # session.open 的连接参数（JSON 字符串）；sqlite 默认走 :memory:
    [string]$Params = "",
    # 被测 Python 解释器；默认探测：.rds/tmp/venv → PATH 的 python
    [string]$Python = "",
    # 大结果 / 慢查询的 SQL 与期望（留空按 Driver 取预设）
    [string]$SqlBig = "",
    [int]$ExpectRows = 0,
    [string]$SqlSlow = "",
    [int]$SlowSeconds = 5,
    # 在 .rds/tmp/venv 建独立环境并安装 pyarrow（系统 pip 不可用时的出路）
    [switch]$CreateVenv
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot | Split-Path -Parent
$shellPy = Join-Path $repoRoot "tools\plugin-drivers\postgres-py\rds_dbapi_shell.py"

if (-not (Test-Path $shellPy)) { throw "找不到壳脚本：$shellPy" }

# ---- 选 Python：显式指定 > venv > PATH ----
if (-not $Python) {
    $venvPy = Join-Path $repoRoot ".rds\tmp\venv\Scripts\python.exe"
    if (Test-Path $venvPy) { $Python = $venvPy } else { $Python = "python" }
}
if ($CreateVenv) {
    $venvDir = Join-Path $repoRoot ".rds\tmp\venv"
    if (-not (Test-Path (Join-Path $venvDir "Scripts\python.exe"))) {
        & python -m venv $venvDir
    }
    & (Join-Path $venvDir "Scripts\python.exe") -m pip install --no-cache-dir pyarrow
    $Python = Join-Path $venvDir "Scripts\python.exe"
}

# ---- pyarrow 预检（只提醒，不拦截：降级行为本身就是被验对象之一） ----
& $Python -c "import pyarrow" 2>$null
if ($LASTEXITCODE -ne 0) {
    Write-Warning "被测 Python 没装 pyarrow：第③条（大结果 Arrow）会降级截断并失败。可 -CreateVenv 自动建环境。"
}

# ---- 按 Driver 取预设 ----
if ($Driver -eq "sqlite") {
    if (-not $Params) { $Params = '{"driver":"sqlite"}' }
    if (-not $SqlBig) { $SqlBig = 'WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM s WHERE n<3000) SELECT n, n*2 AS double_n FROM s' }
    if (-not $ExpectRows) { $ExpectRows = 3000 }
    if (-not $SqlSlow) { $SqlSlow = 'WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x<80000000) SELECT count(*) AS cnt FROM c' }
} elseif ($Driver -eq "postgresql") {
    if (-not $SqlBig) { $SqlBig = 'SELECT n, n*2 AS double_n FROM generate_series(1, 3000) AS s(n)' }
    if (-not $ExpectRows) { $ExpectRows = 3000 }
    if (-not $SqlSlow) { $SqlSlow = 'SELECT pg_sleep(8)' }
} else {
    throw "不认识的 Driver：$Driver（支持 sqlite / postgresql）"
}

# ---- wrapper：conformance 的 spawn 契约是「单可执行文件、零参数」，脚本型壳要包一层 ----
$wrapper = Join-Path $env:TEMP "rds-sidecar-accept.cmd"
@"
@echo off
"$Python" "$shellPy" %*
"@ | Set-Content -Path $wrapper -Encoding ASCII

# ---- 跑 ----
$env:RDS_SIDECAR_BIN = $wrapper
$env:RDS_SIDECAR_DRIVER = $Driver
$env:RDS_SIDECAR_PARAMS = $Params
$env:RDS_SIDECAR_SQL_BIG = $SqlBig
$env:RDS_SIDECAR_EXPECT_ROWS = "$ExpectRows"
$env:RDS_SIDECAR_SQL_SLOW = $SqlSlow
$env:RDS_SIDECAR_SLOW_SECONDS = "$SlowSeconds"

Write-Host "=== sidecar conformance（靶子 = Python DB-API 壳，driver=$Driver，python=$Python） ==="
& cargo test -p rds-plugin --test sidecar_conformance -- --nocapture
exit $LASTEXITCODE
