# 对比两轮全量测试逐目标结果（B 相对 A 的新增/删除/变化）
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File tools/diff-test-runs.ps1 [-LogA <路径>] [-LogB <路径>]
param([string]$LogA = '', [string]$LogB = '')
if (-not $LogA -or -not $LogB) {
    $root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
    $logs = @(Get-ChildItem (Join-Path $root '.rds\tmp') -Filter 'rds-integration-*.log' | Sort-Object LastWriteTime)
    if (-not $LogA) { $LogA = $logs[0].FullName }
    if (-not $LogB) { $LogB = $logs[-1].FullName }
}

function Get-Pairs($log) {
    $lines = @(Get-Content $log)
    $target = $null
    $pairs = @{}
    foreach ($ln in $lines) {
        $m = [regex]::Match($ln, '^\s+(Running|Doc-tests)\s+(.+)$')
        if ($m.Success) { $target = $m.Groups[2].Value.Trim(); continue }
        $r = [regex]::Match($ln, '^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored')
        if ($r.Success -and $target) {
            $pairs[$target] = [pscustomobject]@{
                status = $r.Groups[1].Value
                passed = [int]$r.Groups[2].Value
                failed = [int]$r.Groups[3].Value
                ignored = [int]$r.Groups[4].Value
            }
            $target = $null
        }
    }
    return $pairs
}

$pA = Get-Pairs $LogA
$pB = Get-Pairs $LogB
Write-Host ('A=' + $LogA)
Write-Host ('B=' + $LogB)
Write-Host ('A targets=' + $pA.Count + ' B targets=' + $pB.Count)

# B 相对 A：新增/删除/变化
$add = @($pB.Keys | Where-Object { -not $pA.ContainsKey($_) })
$del = @($pA.Keys | Where-Object { -not $pB.ContainsKey($_) })
$diff = @($pB.Keys | Where-Object {
    $pA.ContainsKey($_) -and (
        $pA[$_].passed -ne $pB[$_].passed -or
        $pA[$_].failed -ne $pB[$_].failed -or
        $pA[$_].ignored -ne $pB[$_].ignored
    )
})
Write-Host ('added_in_B=' + $add.Count + ' removed_from_B=' + $del.Count + ' changed=' + $diff.Count)
$add | ForEach-Object { Write-Host ('  + ' + $_) }
$del | ForEach-Object { Write-Host ('  - ' + $_) }
foreach ($k in $diff) {
    Write-Host ('  ~ ' + $k + ' : A(' + $pA[$k].passed + '/' + $pA[$k].failed + '/' + $pA[$k].ignored + ') B(' + $pB[$k].passed + '/' + $pB[$k].failed + '/' + $pB[$k].ignored + ')')
}
