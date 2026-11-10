# 构建 SQL Server Rust 驱动二进制并放进插件包布局（bin/win-x64/agent.exe）。
# 用法：powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\mssql-rust-plugin\build.ps1 [-Debug]
param(
    [switch]$Debug
)

$ErrorActionPreference = "Stop"
$repo = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
$crate = Join-Path $repo "tools\plugin-drivers\mssql-rust"
$env:CARGO_TARGET_DIR = Join-Path $repo ".rds\target-gf"

$profile = if ($Debug) { "debug" } else { "release" }
$cargoArgs = @("build", "--manifest-path", (Join-Path $crate "Cargo.toml"))
if (-not $Debug) { $cargoArgs += "--release" }
$cargoLog = Join-Path $repo ".rds\tmp\mssql-release.log"
$cargoErr = Join-Path $repo ".rds\tmp\mssql-release.err.log"
$proc = Start-Process -FilePath "cargo" -ArgumentList $cargoArgs -Wait -PassThru -NoNewWindow `
    -RedirectStandardOutput $cargoLog -RedirectStandardError $cargoErr
$code = $proc.ExitCode
if ($code -ne 0) {
    Get-Content -LiteralPath $cargoErr -Tail 30 | Out-Host
    throw "cargo build 失败（EXIT=$code，完整日志：$cargoLog / $cargoErr）"
}

$binDir = Join-Path $PSScriptRoot "bin\win-x64"
New-Item -ItemType Directory -Force -Path $binDir | Out-Null
Copy-Item -LiteralPath (Join-Path $env:CARGO_TARGET_DIR "$profile\rds-mssql-sidecar.exe") `
    -Destination (Join-Path $binDir "agent.exe") -Force
Write-Output "已生成：$(Join-Path $binDir 'agent.exe')"
