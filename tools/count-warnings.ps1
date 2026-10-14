# 统计 check 日志的告警/错误计数（零告警闸门）
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File tools/count-warnings.ps1 [-Log <cargo check 日志路径>]
param([string]$Log = '')
if (-not $Log) {
    $root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
    $Log = Get-ChildItem (Join-Path $root '.rds\tmp') -Filter 'rds-check-*.log' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
}
$lines = @(Get-Content $Log)
$w = @($lines | Select-String -Pattern '^warning')
$e = @($lines | Select-String -Pattern '^error')
Write-Host ('log=' + $Log)
Write-Host ('warnings=' + $w.Count)
Write-Host ('errors=' + $e.Count)
$w | Select-Object -First 5 | ForEach-Object { Write-Host ('  ' + $_.Line) }
$e | Select-Object -First 5 | ForEach-Object { Write-Host ('  ' + $_.Line) }
