# 轮询全量 check 是否结束，输出当前日志尾部与告警/错误计数
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File tools/poll-check.ps1 [-Log <路径>]
param([string]$Log = '')
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$pidfile = Join-Path $root '.rds\tmp\cargo_check.pid'
if (-not $Log) {
    $Log = Get-ChildItem (Join-Path $root '.rds\tmp') -Filter 'rds-check-*.log' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
}
if (Test-Path $pidfile) {
    $pidVal = (Get-Content $pidfile | Select-Object -First 1).Trim()
    if ($pidVal) {
        $proc = Get-Process -Id $pidVal -ErrorAction SilentlyContinue
        if ($proc) { Write-Host ('RUNNING pid=' + $pidVal) } else { Write-Host ('DONE pid=' + $pidVal) }
    }
} else { Write-Host 'NO PID FILE' }
if ($Log -and (Test-Path $Log)) {
    Write-Host ('--- log tail: ' + $Log + ' ---')
    Get-Content $Log -Tail 5 | ForEach-Object { Write-Host $_ }
    $rc = Select-String -Path $Log -Pattern '^CHECK_EXIT_CODE=' | Select-Object -Last 1
    if ($rc) { Write-Host $rc.Line }
    $w = @(Select-String -Path $Log -Pattern '^warning')
    $e = @(Select-String -Path $Log -Pattern '^error')
    Write-Host ('warnings=' + $w.Count + ' errors=' + $e.Count)
}
