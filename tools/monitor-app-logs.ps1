# RdataStation app log monitor (ASCII-safe for PowerShell 5.1)
# Usage:
#   Start background polling:   powershell -NoProfile -ExecutionPolicy Bypass -File tools/monitor-app-logs.ps1 -Start
#   Report new/error lines:     powershell -NoProfile -ExecutionPolicy Bypass -File tools/monitor-app-logs.ps1 -Report
#   Stop monitor:               powershell -NoProfile -ExecutionPolicy Bypass -File tools/monitor-app-logs.ps1 -Stop
#   Recent N minutes summary:   powershell -NoProfile -ExecutionPolicy Bypass -File tools/monitor-app-logs.ps1 -Recent -Minutes 10
param(
    [switch]$Start,
    [switch]$Report,
    [switch]$Stop,
    [switch]$Recent,
    [int]$Minutes = 10
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$logsDir = Join-Path $root '.rds\logs'
$stateFile = Join-Path $root '.rds\tmp\monitor_state.txt'

function Get-AppLogPath {
    $today = Get-Date -Format 'yyyy-MM-dd'
    $todayFile = Join-Path $logsDir "app.$today"
    if (Test-Path $todayFile) { return $todayFile }
    $latest = Get-ChildItem $logsDir -Filter 'app.*' -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($latest) { return $latest.FullName }
    return $null
}

function Get-LastLineNumber($path) {
    if (-not (Test-Path $path)) { return 0 }
    $reader = [System.IO.StreamReader]::new($path)
    try {
        $count = 0
        while ($reader.ReadLine() -ne $null) { $count++ }
        return $count
    } finally { $reader.Dispose() }
}

if ($Start) {
    $logPath = Get-AppLogPath
    if (-not $logPath) { Write-Host 'ERROR: no app log found'; exit 1 }
    $startLine = Get-LastLineNumber $logPath
    Set-Content -Path $stateFile -Value "STARTED $(Get-Date -Format 'yyyy-MM-ddTHH:mm:ss') LOG=$logPath LINE=$startLine" -Encoding ascii
    Write-Host "monitor started: $logPath (from line $startLine)"
    exit 0
}

if ($Stop) {
    Remove-Item -Path $stateFile -ErrorAction SilentlyContinue
    Write-Host 'monitor stopped'
    exit 0
}

if ($Report) {
    if (-not (Test-Path $stateFile)) { Write-Host 'monitor not started'; exit 0 }
    $state = Get-Content $stateFile -Raw
    $startLine = 0
    if ($state -match 'LINE=(\d+)') { $startLine = [int]$Matches[1] }
    $logPath = Get-AppLogPath
    if (-not $logPath) { Write-Host 'app log missing'; exit 0 }
    $all = Get-Content $logPath
    $total = $all.Count
    $newCount = [Math]::Max(0, $total - $startLine)
    Write-Host "report: $newCount new lines since start (total $total)"
    $slice = if ($total -gt $startLine) { $all | Select-Object -Skip $startLine } else { @() }
    $errs = @($slice | Select-String -Pattern '\bERROR\b|\bPANIC\b|\bpanic\b')
    Write-Host ('ERROR/PANIC lines: ' + $errs.Count)
    $errs | Select-Object -First 30 | ForEach-Object { Write-Host ('  ' + $_.Line) }
    $slice | Set-Content -Path (Join-Path $root '.rds\tmp\monitor_tail.txt') -Encoding utf8
    Write-Host 'new lines saved to .rds/tmp/monitor_tail.txt'
    exit 0
}

if ($Recent) {
    $logPath = Get-AppLogPath
    if (-not $logPath) { Write-Host 'app log missing'; exit 0 }
    $cutoff = (Get-Date).AddMinutes(-$Minutes)
    $all = Get-Content $logPath
    $slice = @($all | Where-Object {
        if ($_ -match '^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})') {
            try { [datetime]::Parse($Matches[1]) -ge $cutoff } catch { $false }
        } else { $false }
    })
    Write-Host "last $Minutes minutes: $($slice.Count) lines"
    $errs = @($slice | Select-String -Pattern '\bERROR\b|\bPANIC\b|\bpanic\b')
    Write-Host ('ERROR/PANIC: ' + $errs.Count)
    $errs | Select-Object -First 40 | ForEach-Object { Write-Host ('  ' + $_.Line) }
    exit 0
}

Write-Host 'usage: -Start / -Report / -Stop / -Recent -Minutes N'
