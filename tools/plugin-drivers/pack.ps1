# 把一个插件包打成可发布的 zip（压缩包根 = plugin.toml 所在层），包里附带 checksums.json。
#
# 用法：
#   pwsh -File tools\plugin-drivers\pack.ps1 -Plugin oracle-jdbc
#   pwsh -File tools\plugin-drivers\pack.ps1 -Plugin oracle-jdbc -OutDir D:\dist -Name oracle-1.0.zip
#
# 为什么要暂存目录：checksums 必须覆盖"将来装进安装目录的那批字节"。源码目录里有
# .git / __pycache__ / 编辑器残留，install.ps1 与 installer::copy_dir 都会跳过它们——
# 所以先 robocopy 到干净暂存目录（同一套排除清单），在暂存目录里生成校验，再压缩。
# 源码目录因此不会多出 checksums.json 这种未跟踪产物。

param(
    [Parameter(Mandatory = $true)][string]$Plugin,
    [string]$OutDir = "",
    [string]$Name = ""
)

$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "_checksums.ps1")

$root = Get-PluginDriversRepoRoot
$dist = Join-Path $root "tools\plugin-drivers"

# 源目录：先当路径（相对 / 绝对），再当 tools\plugin-drivers 下的包名
$src = if (Test-Path -LiteralPath $Plugin -PathType Container) {
    (Resolve-Path -LiteralPath $Plugin).Path
} else {
    Join-Path $dist $Plugin
}
$manifestPath = Join-Path $src "plugin.toml"
if (-not (Test-Path -LiteralPath $manifestPath)) {
    throw "不是插件包（缺 plugin.toml）：$src"
}

# id / version 只用来命名产物；读不到就报错，不猜
$toml = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8
$pluginSection = if ($toml -match "(?s)\[plugin\](.*?)(\r?\n\[|\z)") { $Matches[1] } else {
    throw "plugin.toml 里没有 [plugin] 段：$manifestPath"
}
$id = if ($pluginSection -match '(?m)^id\s*=\s*"([^"]+)"') { $Matches[1] } else {
    throw "plugin.toml 的 [plugin] 段里没有 id：$manifestPath"
}
$version = if ($pluginSection -match '(?m)^version\s*=\s*"([^"]+)"') { $Matches[1] } else {
    throw "plugin.toml 的 [plugin] 段里没有 version：$manifestPath"
}
$hasExternalJars = ($toml -match '(?m)^driver_jars\s*=')

$outDir = if ($OutDir) { [IO.Path]::GetFullPath($OutDir) } else { Join-Path $root ".rds\tmp\packages" }
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

# 暂存目录带时间戳：不删旧目录，也就不会误删别的东西
$stamp = Get-Date -Format "yyyyMMdd-HHmmss"
$staging = Join-Path $root ".rds\tmp\pack-staging\$id-$stamp"
New-Item -ItemType Directory -Force -Path $staging | Out-Null

$rcArgs = @($src, $staging, '/E') + (Get-DevResidueRobocopyArgs)
& robocopy @rcArgs | Out-Null
if ($LASTEXITCODE -ge 8) { throw "复制到暂存目录失败（robocopy $LASTEXITCODE）：$src → $staging" }

# 发布包没有 checksums 等于放弃完整性校验：这里没过就整体失败，不像安装可以跳过
$tool = Get-ChecksumTool
if (-not (New-PackageChecksums $staging $tool)) {
    throw "没有 checksums 生成工具，拒绝打出不带校验清单的发布包（先 cargo build -p rds-plugin --bin rds-plugin-checksums）"
}

$zipName = if ($Name) { $Name } else { "$id-$version.zip" }
$zip = Join-Path $outDir $zipName
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force }

# 用 Get-ChildItem 而不是 "$staging\*"：通配符会漏掉隐藏文件（.gitignore 之类）
Get-ChildItem -LiteralPath $staging -Force | Compress-Archive -DestinationPath $zip -CompressionLevel Optimal
$size = (Get-Item -LiteralPath $zip).Length
$archive = [IO.Compression.ZipFile]::OpenRead($zip)
try { $entries = $archive.Entries.Count } finally { $archive.Dispose() }

Write-Output "已打包：$zip（$size 字节，$entries 个条目，包根 = plugin.toml）"
if ($hasExternalJars) {
    Write-Output "注意：该包声明了 driver_jars（插件目录外的 jar），zip 里不含它们——分发时要一并给，checksums.json 已把这些外部 jar 算进去"
}
