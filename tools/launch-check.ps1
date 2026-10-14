# 分离启动全量 check wrapper（避免长编译阻塞终端）
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File tools/launch-check.ps1
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$wrapper = Join-Path $PSScriptRoot 'run-check.cmd'
$pidfile = Join-Path $root '.rds\tmp\cargo_check.pid'
$p = Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', $wrapper -WorkingDirectory $root -WindowStyle Hidden -PassThru
$p.Id | Out-File -FilePath $pidfile -Encoding ascii
Write-Host ('launched check pid: ' + $p.Id)
Write-Host ('log: ' + (Join-Path $root '.rds\tmp\rds-check-*.log'))
