# 全量集成测试结果校验（与报告判据一致）
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File tools/verify-integration-log.ps1 [-Log <cargo test 日志路径>]
# 通过标准：Running/Doc-tests 数 == test result 数（全配对）、0 个 test result: FAILED、0 个 error[
param([string]$Log = '')
if (-not $Log) {
    $root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
    $Log = Get-ChildItem (Join-Path $root '.rds\tmp') -Filter 'rds-integration-*.log' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
}
$lines = @(Get-Content $Log)

# 1) EXIT_CODE
$rc = ($lines | Select-String -Pattern '^EXIT_CODE=' | Select-Object -Last 1)
if ($rc) { Write-Host ('EXIT_CODE=' + $rc.Line) } else { Write-Host 'EXIT_CODE not found in log' }

# 2) 真实 FAILED（cargo 报告失败时会打印大写 FAILED 行）
$realFail = @($lines | Select-String -Pattern '^test result: FAILED')
Write-Host ('real_test_result_FAILED=' + $realFail.Count)
$realFail | ForEach-Object { Write-Host ('  ' + $_.Line) }

# 3) error: 行（编译错误等）
$errs = @($lines | Select-String -Pattern '^error(\[|:)' | Select-Object -First 5)
Write-Host ('compile_errors=' + $errs.Count)
$errs | ForEach-Object { Write-Host ('  ' + $_.Line) }

# 4) 配对：Running/Doc-tests <-> test result
$target = $null
$pairs = New-Object System.Collections.ArrayList
$runningCount = 0
$resultCount = 0
foreach ($ln in $lines) {
    $m = [regex]::Match($ln, '^\s+(Running|Doc-tests)\s+(.+)$')
    if ($m.Success) { $target = $m.Groups[2].Value.Trim(); $runningCount++ ; continue }
    $r = [regex]::Match($ln, '^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored')
    if ($r.Success) {
        $resultCount++
        if ($target) {
            $null = $pairs.Add([pscustomobject]@{
                target = $target
                status = $r.Groups[1].Value
                passed = [int]$r.Groups[2].Value
                failed = [int]$r.Groups[3].Value
                ignored = [int]$r.Groups[4].Value
            })
            $target = $null
        }
    }
}
Write-Host ('Running/Doc-tests lines=' + $runningCount)
Write-Host ('test result lines=' + $resultCount)
Write-Host ('paired_targets=' + $pairs.Count)
$tp = ($pairs | Measure-Object -Property passed -Sum).Sum
$tf = ($pairs | Measure-Object -Property failed -Sum).Sum
$ti = ($pairs | Measure-Object -Property ignored -Sum).Sum
Write-Host ('TOTAL passed=' + $tp + ' failed=' + $tf + ' ignored=' + $ti)
