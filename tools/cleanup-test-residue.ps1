# 清理 .rds/tmp 下 cargo test 产生的测试残留（目录 + 垃圾文件），保留日志/脚本/diffs/sidecar
# 用法：powershell -NoProfile -ExecutionPolicy Bypass -File tools/cleanup-test-residue.ps1
$ErrorActionPreference = 'Stop'
$root = Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..')).Path '.rds\tmp'
$keep = @('diffs', 'sidecar')

$dirs = @(Get-ChildItem -Force $root -Directory)
$toDelete = @($dirs | Where-Object { $_.Name -notin $keep })
Write-Output ("dirs_total=" + $dirs.Count)
Write-Output ("dirs_to_delete=" + $toDelete.Count)
foreach ($d in $toDelete) {
    Remove-Item -LiteralPath $d.FullName -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Output ("dirs_deleted=" + $toDelete.Count)

# 测试垃圾文件（.db / .duckdb.wal / 截图）
$exts = @('.db', '.duckdb.wal', '.png', '.bmp')
$files = @(Get-ChildItem -Force $root -File)
$filesToDelete = @($files | Where-Object {
    $n = $_.Name.ToLower()
    foreach ($e in $exts) { if ($n.EndsWith($e)) { return $true } }
    return $false
})
Write-Output ("files_total=" + $files.Count)
Write-Output ("files_to_delete=" + $filesToDelete.Count)
foreach ($f in $filesToDelete) {
    Remove-Item -LiteralPath $f.FullName -Force -ErrorAction SilentlyContinue
}

$remain = @(Get-ChildItem -Force $root)
Write-Output ("remaining_items=" + $remain.Count)
$remain | Select-Object -First 40 | ForEach-Object { Write-Output ("  " + $_.Name) }
