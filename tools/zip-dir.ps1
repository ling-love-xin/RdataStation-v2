param([Parameter(Mandatory = $true)][string]$Src, [Parameter(Mandatory = $true)][string]$Dst)

# 打包一个目录为 zip：Windows 上 Git-Bash 常没有 `zip`，而 `Compress-Archive`
# 写出的条目名用**反斜杠**分隔（在 Linux / macOS 上解出来是带反斜杠的怪文件名），
# 故这里用 .NET 的 ZipArchive 手工写条目，条目名一律用正斜杠。
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem
Add-Type -AssemblyName System.IO.Compression

$src = $Src.TrimEnd('\', '/')
if (-not (Test-Path -LiteralPath $src -PathType Container)) { throw "要打包的目录不存在：$src" }
if (Test-Path -LiteralPath $Dst) { Remove-Item -LiteralPath $Dst -Force }

$zip = [System.IO.Compression.ZipFile]::Open($Dst, 'Create')
try {
    $prefix = $src.Length + 1
    $leaf = Split-Path -Leaf $src
    Get-ChildItem -LiteralPath $src -Recurse -Force -File | ForEach-Object {
        # 条目名以**目录名**开头，解压后是一个完整的文件夹（不散在原地）
        $rel = $leaf + '/' + $_.FullName.Substring($prefix).Replace('\', '/')
        [void][System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
            $zip, $_.FullName, $rel, 'Optimal')
    }
} finally {
    $zip.Dispose()
}
Write-Output "zip-ok: $Dst"
