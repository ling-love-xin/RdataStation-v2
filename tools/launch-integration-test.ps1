# 分离启动全量集成测试 wrapper（避免长编译阻塞终端）
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File tools/launch-integration-test.ps1
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$wrapper = Join-Path $PSScriptRoot 'run-integration-test.cmd'
$pidfile = Join-Path $root '.rds\tmp\cargo_test.pid'
$p = Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', $wrapper -WorkingDirectory $root -WindowStyle Hidden -PassThru
$p.Id | Out-File -FilePath $pidfile -Encoding ascii
Write-Host ('launched wrapper pid: ' + $p.Id)
Write-Host ('log: ' + (Join-Path $root '.rds\tmp\rds-integration-*.log'))
