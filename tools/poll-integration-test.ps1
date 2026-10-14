# 轮询全量集成测试是否结束，输出当前日志尾部与 FAILED
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File tools/poll-integration-test.ps1 [-Log <路径>]
param([string]$Log = '')
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$pidfile = Join-Path $root '.rds\tmp\cargo_test.pid'
if (-not $Log) {
    $Log = Get-ChildItem (Join-Path $root '.rds\tmp') -Filter 'rds-integration-*.log' | Sort-Object LastWriteTime -Descending | Select-Object -First 1 -ExpandProperty FullName
}
if (Test-Path $pidfile) {
    $pidVal = (Get-Content $pidfile | Select-Object -First 1).Trim()
    if ($pidVal) {
        $proc = Get-Process -Id $pidVal -ErrorAction SilentlyContinue
        if ($proc) {
            Write-Host ('RUNNING pid=' + $pidVal + ' cpu=' + [math]::Round($proc.CPU,1) + 's')
        } else {
            Write-Host ('DONE pid=' + $pidVal + ' (process exited)')
        }
    }
} else {
    Write-Host 'NO PID FILE'
}
if ($Log -and (Test-Path $Log)) {
    Write-Host ('--- log tail: ' + $Log + ' ---')
    Get-Content $Log -Tail 6 | ForEach-Object { Write-Host $_ }
    $rc = Select-String -Path $Log -Pattern '^EXIT_CODE=' | Select-Object -Last 1
    if ($rc) { Write-Host $rc.Line }
    $fc = Select-String -Path $Log -Pattern '^test result: FAILED' | Select-Object -First 3
    if ($fc) { Write-Host '--- FAILED lines ---'; $fc | ForEach-Object { Write-Host $_.Line } }
}
