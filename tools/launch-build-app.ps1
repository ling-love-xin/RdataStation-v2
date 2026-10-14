# 分离启动 rds-app 重建（避免长编译阻塞终端）
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$wrapper = Join-Path $PSScriptRoot 'run-build-app.cmd'
$pidfile = Join-Path $root '.rds\tmp\cargo_build_app.pid'
$p = Start-Process -FilePath 'cmd.exe' -ArgumentList '/c', $wrapper -WorkingDirectory $root -WindowStyle Hidden -PassThru
$p.Id | Out-File -FilePath $pidfile -Encoding ascii
Write-Host ('launched build-app wrapper pid: ' + $p.Id)
