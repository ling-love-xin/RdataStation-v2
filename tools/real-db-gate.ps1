# 真机门控测试：一次给定 5 个 RDS_TEST_* 变量，逐包跑集成测试并汇总「真机是否真的跑过」
#
# 为什么需要：跨源用例全部按环境变量门控，未设即 return（打印「真机链路未验证（不算失败）」）。
# 所以 `cargo test` 全绿**不等于**跨源已验证。本脚本把这件事变成日志里可数的事实：
# 每包输出 passed / failed / ignored / 跳过标记数，并给出结论。
#
# 用法（Windows PowerShell 7）：
#   $env:RDS_TEST_MYSQL_URL   = 'mysql://root:root@192.168.3.138:3306/mysql'
#   $env:RDS_TEST_PG_URL      = 'postgres://postgres:postgresql@192.168.3.138:5432/postgres'
#   $env:RDS_TEST_SQLITE_PATH = 'D:\FossilT\T.fossil'
#   $env:RDS_TEST_DUCKDB_PATH = 'D:\data\123'
#   $env:RDS_TEST_ORACLE_URL  = 'oracle://devuser:Dev2026123@192.168.3.138:1521/XEPDB1'
#   pwsh -NoProfile -File tools/real-db-gate.ps1
#
# 参数：
#   -Packages  要跑的包（默认 rds-engine / rds-database / rds-workbench）
#
# 跑法固定：`cargo test -p <包> -j 1 --no-fail-fast --tests -- --test-threads=1`。
#   * `--tests`（cargo 语义）＝「所有 test = true 的目标」，**含 lib 单测**，所以同时覆盖单测与集成；
#   * `--no-fail-fast` 保证某个 target 红了也跑完其余，结论是完整的；
#   * `--test-threads=1` 必须：联邦/内核用例共用按 pid 命名的临时底座目录，并行会抢同一文件。
#
# 退出码：有失败 = 1；否则 0。注意「全绿」不必然是「真机验收通过」：门控变量没设齐时
# 相关用例根本没跑（多数静默 return，日志里连跳过字样都没有），结论行会明说「未验证」。
#
# 已知噪音：`crates/engine/tests/analysis_kernel_probe.rs` 的用例共享进程单例，加速档会话
# attach 的 `rds_src` 没卸载，全量跑时后面的用例撞「别名已被占用」；单独 `--exact` 跑都通过。
# 这是测试隔离债（审计报告 P1-9），脚本单独计「已知噪音」，不混进真机语义失败。

[CmdletBinding()]
param(
    [string[]]$Packages = @('rds-engine', 'rds-database', 'rds-workbench')
)

$ErrorActionPreference = 'Continue'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$tmpDir = Join-Path $root '.rds\tmp'
if (-not (Test-Path $tmpDir)) { New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null }

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host 'cargo 不在 PATH：先确认 Rust 工具链可用（本机 CARGO_HOME 一般是 D:\Dev\Rust\.cargo）'
    exit 2
}

$stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
$summaryPath = Join-Path $tmpDir "real-db-gate-$stamp.log"
$transcript = New-Object System.Collections.Generic.List[string]

function Say([string]$text) {
    Write-Host $text
    [void]$transcript.Add($text)
}

$gates = @(
    'RDS_TEST_MYSQL_URL',
    'RDS_TEST_PG_URL',
    'RDS_TEST_SQLITE_PATH',
    'RDS_TEST_DUCKDB_PATH',
    'RDS_TEST_ORACLE_URL'
)

Say "real-db-gate $stamp"
Say "root=$root"
Say "CARGO_HOME=$env:CARGO_HOME"
Say ''
Say '--- 门控变量 ---'
foreach ($name in $gates) {
    $value = [Environment]::GetEnvironmentVariable($name)
    $state = if ($value) { '已设' } else { '未设（相关用例会跳过，不算失败）' }
    Say ('{0,-24} {1}' -f $name, $state)
}
$missing = @($gates | Where-Object { -not [Environment]::GetEnvironmentVariable($_) })
Say ''
if ($missing.Count -gt 0) {
    Say ("警告：{0} 个变量未设 → 日志里会出现「真机未验证」标记，不要当成已验证" -f $missing.Count)
}

$results = @()
foreach ($pkg in $Packages) {
    $logPath = Join-Path $tmpDir "real-db-gate-$stamp-$pkg.log"
    # `--no-fail-fast`：某个 target 红了也要跑完其余 target，否则只拿到第一段结果；
    # `--test-threads=1`：engine 的联邦/内核用例共用「按 pid 命名」的临时底座目录
    # （`.rds\tmp\rds_fed_bases_<pid>`），并行跑会互相抢文件（IO Error / catalog 写冲突）。
    $cargoArgs = @('test', '-p', $pkg, '-j', '1', '--no-fail-fast', '--tests')
    $cargoArgs += @('--', '--test-threads=1')

    Say ''
    Say ("--- {0} ---" -f $pkg)
    Say "log=$logPath"
    Say ('cmd = cargo {0}' -f ($cargoArgs -join ' '))

    # cargo 的 Compiling 走 stderr，PowerShell 会把它判成 NativeCommandError（$LASTEXITCODE 不可靠）；
    # 结论以日志里的 `test result:` 行为准。
    & cargo @cargoArgs 2>&1 | Out-File -FilePath $logPath -Encoding utf8
    $text = Get-Content $logPath -Encoding utf8 -Raw

    # 已知噪音（P1-9）：`analysis_kernel_probe` 的用例共享进程单例，加速档会话 attach 的
    # `rds_src` 没卸载，全量跑时后面的用例都会撞「别名已被占用」。单独 --exact 跑都通过，
    # 这不是产品回退，所以单独计数，别把它算成真机语义失败。
    $resultMatches = [regex]::Matches($text, 'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored')
    $targets = 0; $passed = 0; $failed = 0; $ignored = 0
    foreach ($m in $resultMatches) {
        $targets++
        $passed += [int]$m.Groups[2].Value
        $failed += [int]$m.Groups[3].Value
        $ignored += [int]$m.Groups[4].Value
    }
    if ($targets -eq 0) {
        Say '没有解析到 test result（编译失败或包里没有集成测试）'
    }
    $skipped = ([regex]::Matches($text, '未设 RDS_TEST_|真机链路未验证')).Count
    $knownNoise = ([regex]::Matches($text, '别名 rds_src 已被连接')).Count
    Say ('targets={0} passed={1} failed={2} ignored={3} 跳过标记={4} 已知噪音={5}' -f $targets, $passed, $failed, $ignored, $skipped, $knownNoise)
    $results += [pscustomobject]@{
        pkg = $pkg; targets = $targets; passed = $passed
        failed = $failed; ignored = $ignored; skipped = $skipped
        noise = $knownNoise; log = $logPath
    }
}

Say ''
Say '--- 汇总 ---'
foreach ($r in $results) {
    Say ('{0,-14} targets={1,-3} passed={2,-4} failed={3,-3} ignored={4,-3} 跳过={5,-3} 已知噪音={6}' -f `
        $r.pkg, $r.targets, $r.passed, $r.failed, $r.ignored, $r.skipped, $r.noise)
}

$totalFailed = ($results | Measure-Object -Property failed -Sum).Sum
$totalNoise = ($results | Measure-Object -Property noise -Sum).Sum
$totalSkipped = ($results | Measure-Object -Property skipped -Sum).Sum
$totalPassed = ($results | Measure-Object -Property passed -Sum).Sum
$gatesOff = $missing.Count
Say ''
if ($totalFailed -gt 0 -and $totalNoise -ge $totalFailed) {
    Say "结论：有失败（$totalFailed），但全部命中已知噪音（analysis_kernel_probe 用例间单例污染，P1-9）——先修隔离再据本脚本判产品"
    $exitCode = 1
} elseif ($totalFailed -gt 0) {
    Say "结论：有失败（$totalFailed），其中 $totalNoise 条是已知噪音，其余看各包日志（真机语义已跑过，属真实问题）"
    $exitCode = 1
} elseif ($gatesOff -gt 0) {
    # 门控变量未设＝相关用例根本没跑（多数用例静默 return，日志里连「跳过」字样都没有），
    # 所以「failed=0」在这里只证明「跑到的没有红」，不证明跨源真机语义成立。
    Say "结论：全绿（passed=$totalPassed），但 $gatesOff 个门控变量未设——跨源真机语义**未验证**（不少用例静默跳过，日志里不会有跳过字样）"
    $exitCode = 0
} elseif ($totalSkipped -gt 0) {
    Say "结论：全绿，但变量都设了却仍有 $totalSkipped 处「真机未验证」标记——用例自身跳过了，回看日志确认哪条"
    $exitCode = 0
} else {
    Say "结论：全绿（passed=$totalPassed）且 5 个门控变量全设、无跳过标记——跨源真机验收通过"
    $exitCode = 0
}

Say "summary=$summaryPath"
$transcript | Out-File -FilePath $summaryPath -Encoding utf8
exit $exitCode
