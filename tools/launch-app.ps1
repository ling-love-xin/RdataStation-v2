# Launch rds-app for manual UI testing (detached, ASCII-safe for PowerShell 5.1)
# Sets RDS_HOME/TEMP to match .cargo/config.toml [env] (cargo does not inject env
# when the exe is launched directly).
$root = 'D:\RdataStation\RDS\RdataStation-v2'
$env:RDS_HOME = $root + '\.rds'
$env:TEMP = $root + '\.rds\tmp'
$env:TMP = $root + '\.rds\tmp'
$env:TMPDIR = $root + '\.rds\tmp'
$exe = Join-Path $root 'target\debug\rds-app.exe'
if (-not (Test-Path $exe)) { Write-Host "ERROR: exe not found: $exe"; exit 1 }
$p = Start-Process -FilePath $exe -WorkingDirectory $root -PassThru
$p.Id | Out-File -FilePath (Join-Path $root '.rds\tmp\rds-app.pid') -Encoding ascii
Write-Host ('launched rds-app pid: ' + $p.Id)
