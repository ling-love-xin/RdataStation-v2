# rds-jdbc-shell 构建脚本（无外部依赖，仅 JDK 21）
# 产物：lib/rds-jdbc-shell.jar
# 用法：powershell -ExecutionPolicy Bypass -File build.ps1
#
# 注：本机 javac/java 在 Oracle javapath（只有 4 个工具），jar 不在 PATH；
#     通过 java.home 探测真实 JDK 的 bin\jar.exe。

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$src = Join-Path $root "src"
$build = Join-Path $root "build\classes"
$out = Join-Path $root "lib\rds-jdbc-shell.jar"

# 探测 java.home（java -XshowSettings 走 stderr，用 cmd 合并输出再解析）
$javaHome = $null
foreach ($line in (cmd /c "java -XshowSettings:properties -version 2>&1")) {
    if ($line -match '^\s*java\.home\s*=\s*(.+)$') { $javaHome = $Matches[1].Trim() }
}
if (-not $javaHome) { throw "无法探测 java.home" }
$jarTool = Join-Path $javaHome "bin\jar.exe"
if (-not (Test-Path $jarTool)) { throw "JDK 缺少 bin\jar.exe：$jarTool" }

if (Test-Path $build) { Remove-Item -Recurse -Force $build }
New-Item -ItemType Directory -Force -Path $build | Out-Null

Push-Location $root
try {
    & javac -encoding UTF-8 -source 21 -target 21 -d $build (Get-ChildItem -Recurse -Filter *.java $src | ForEach-Object { $_.FullName })
    if ($LASTEXITCODE -ne 0) { throw "javac 失败" }
    & $jarTool cfe $out rds.jdbc.Main -C $build .
    if ($LASTEXITCODE -ne 0) { throw "jar 打包失败" }
    Write-Output "构建完成：$out"
} finally {
    Pop-Location
}
