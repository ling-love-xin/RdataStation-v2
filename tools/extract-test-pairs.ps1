# 提取全量测试日志的逐目标配对结果（Running/Doc-tests <-> test result）
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File tools/extract-test-pairs.ps1 [-Log <路径>]
param([string]$Log = '')
if (-not $Log) {
    $root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
    $Log = Get-ChildItem (Join-Path $root '.rds\tmp') -Filter 'rds-integration-*.log' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
}
$lines = Get-Content $Log
$target = $null
$pairs = New-Object System.Collections.ArrayList
foreach ($ln in $lines) {
    $m = [regex]::Match($ln, '^\s+(Running|Doc-tests)\s+(.+)$')
    if ($m.Success) {
        $target = $m.Groups[2].Value.Trim()
        continue
    }
    $r = [regex]::Match($ln, '^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored')
    if ($r.Success -and $target) {
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
Write-Host ('log=' + $Log)
Write-Host ('paired_targets=' + $pairs.Count)
$tp = ($pairs | Measure-Object -Property passed -Sum).Sum
$tf = ($pairs | Measure-Object -Property failed -Sum).Sum
$ti = ($pairs | Measure-Object -Property ignored -Sum).Sum
Write-Host ('TOTAL passed=' + $tp + ' failed=' + $tf + ' ignored=' + $ti)
Write-Host '--- per target ---'
$i = 0
foreach ($p in $pairs) {
    $i++
    Write-Host ($i.ToString('000') + ' [' + $p.status + '] p=' + $p.passed + ' f=' + $p.failed + ' i=' + $p.ignored + '  ' + $p.target)
}
