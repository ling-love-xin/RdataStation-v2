# Continuous app log capture loop for UIST testing (ASCII-safe for PowerShell 5.1)
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File tools/tail-app-logs.ps1 [-Out <capture file>]
#   - Polls the app log every 2s and appends new lines to the capture file
#   - Also watches for new panic-*.log files (crash capture)
#   - End: Ctrl+C
param(
    [string]$Out = ''
)
$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$logsDir = Join-Path $root '.rds\logs'
if (-not $Out) { $Out = Join-Path $root ".rds\tmp\tail_capture_$(Get-Date -Format 'yyyyMMdd_HHmmss').txt" }

function Get-AppLogPath {
    $today = Get-Date -Format 'yyyy-MM-dd'
    $todayFile = Join-Path $logsDir "app.$today"
    if (Test-Path $todayFile) { return $todayFile }
    $latest = Get-ChildItem $logsDir -Filter 'app.*' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($latest) { return $latest.FullName }
    return $null
}

$logPath = Get-AppLogPath
if (-not $logPath) { Write-Host 'ERROR: no app log found'; exit 1 }

$lastCount = @(Get-Content $logPath -ErrorAction SilentlyContinue).Count
$knownPanics = @{}
Get-ChildItem $logsDir -Filter 'panic-*.log' -ErrorAction SilentlyContinue |
    ForEach-Object { $knownPanics[$_.Name] = $true }

Set-Content -Path $Out -Value "# tail capture started $(Get-Date -Format 'yyyy-MM-ddTHH:mm:ss') from $logPath (start line $lastCount)" -Encoding utf8
Write-Host "tail capture started: $logPath -> $Out"

while ($true) {
    Start-Sleep -Seconds 2
    $current = @(Get-Content $logPath -ErrorAction SilentlyContinue).Count
    if ($current -gt $lastCount) {
        $newLines = Get-Content $logPath | Select-Object -Skip $lastCount
        $stamp = Get-Date -Format 'yyyy-MM-ddTHH:mm:ss'
        Add-Content -Path $Out -Value ("--- [$stamp] +" + ($current - $lastCount) + " lines ---") -Encoding utf8
        Add-Content -Path $Out -Value $newLines -Encoding utf8
        $newErr = @($newLines | Select-String -Pattern '\bERROR\b|\bPANIC\b|\bpanic\b')
        foreach ($e in $newErr) {
            Write-Host ("[CAPTURE-ERR] " + $e.Line)
        }
        $lastCount = $current
    }
    Get-ChildItem $logsDir -Filter 'panic-*.log' -ErrorAction SilentlyContinue |
        Where-Object { -not $knownPanics.ContainsKey($_.Name) } |
        ForEach-Object {
            $knownPanics[$_.Name] = $true
            $stamp = Get-Date -Format 'yyyy-MM-ddTHH:mm:ss'
            $msg = "### [$stamp] NEW PANIC LOG: $($_.Name) ###"
            Write-Host $msg
            Add-Content -Path $Out -Value $msg -Encoding utf8
            Add-Content -Path $Out -Value (Get-Content $_.FullName -ErrorAction SilentlyContinue) -Encoding utf8
        }
}
