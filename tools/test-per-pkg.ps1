# 按 unit / integration / doctest 归类统计全量测试日志
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File tools/test-per-pkg.ps1 [-Log <路径>]
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
    if ($m.Success) { $target = $m.Groups[2].Value.Trim(); continue }
    $r = [regex]::Match($ln, '^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored')
    if ($r.Success -and $target) {
        $null = $pairs.Add([pscustomobject]@{ target=$target; status=$r.Groups[1].Value; passed=[int]$r.Groups[2].Value; failed=[int]$r.Groups[3].Value; ignored=[int]$r.Groups[4].Value })
        $target = $null
    }
}
# 归类：unit 是 'unittests src\...'；doctest 是纯 crate 名（无路径）；integration 是 tests\ / src\bin\
$units = $pairs | Where-Object { $_.target -match '^unittests' }
$doctests = $pairs | Where-Object { $_.target -notmatch '\\.' -and $_.target -notmatch '^unittests' }
$integrations = $pairs | Where-Object { $_.target -match '^tests\\' -or $_.target -match '^src\\bin\\' }
Write-Host ('log=' + $Log)
Write-Host ('unit_targets=' + $units.Count + ' integration_targets=' + $integrations.Count + ' doctest_targets=' + $doctests.Count + ' total=' + $pairs.Count)
$sum = { param($list) $p = ($list | Measure-Object -Property passed -Sum).Sum; $f = ($list | Measure-Object -Property failed -Sum).Sum; $i = ($list | Measure-Object -Property ignored -Sum).Sum; "$p passed / $f failed / $i ignored" }
Write-Host ('unit:     ' + (& $sum $units))
Write-Host ('integ:    ' + (& $sum $integrations))
Write-Host ('doctest:  ' + (& $sum $doctests))
