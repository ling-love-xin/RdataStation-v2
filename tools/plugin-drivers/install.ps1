# rds 驱动插件包安装脚本：组装 oracle-jdbc / postgres-py / oracle-rust / mssql-rust 并安装到 RDS_HOME/plugins。
#
# 用法：powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\install.ps1 [-SkipOjdbc] [-SkipRust] [-RustDebug]
#   -SkipOjdbc：跳过 ojdbc 下载（离线环境；只装已就绪的 jar，oracle 插件仍可装但缺驱动）
#   -SkipRust：跳过两个 Rust 原生驱动的 cargo 构建（只装已有 bin/win-x64/agent.exe）
#   -RustDebug：Rust 驱动按 debug 档构建（默认 release）
#
# 产物原则：仓库不存驱动 jar / 壳 jar / 驱动二进制（.gitignore），全部运行时组装：
#   - Java 壳：tools/rds-jdbc-shell/build.ps1 构建 → 复制
#   - H2：从 Maven Central 下载（无证书验证用）
#   - ojdbc11：从 Maven Central 下载（Oracle 官方坐标；OTN 许可，见 oracle-jdbc/README.md）
#   - Python 壳：源码已在 postgres-py/ 内（无需下载）
#   - Rust 驱动：cargo build → 复制到 <包>/bin/win-x64/agent.exe
# 安装目标：<RDS_HOME>/plugins/<manifest-id>（RDS_HOME 开发期 = 仓库根/.rds，见 crates/paths）

param(
    [switch]$SkipOjdbc,
    [switch]$SkipRust,
    [switch]$RustDebug
)

$ErrorActionPreference = "Stop"
$root = "D:\RdataStation\RDS\RdataStation-v2"
$tools = Join-Path $root "tools"
$dist = Join-Path $tools "plugin-drivers"

# Maven Central 坐标（用 curl.exe：本机 pwsh 非交互模式下 Invoke-WebRequest 有朗读问题）
$OJDBC_GROUP = "com/oracle/database/jdbc"
$OJDBC_ARTIFACT = "ojdbc11"
$H2_GROUP = "com/h2database"
$H2_ARTIFACT = "h2"

# 从 Maven Central metadata 取最新版本（不写死版本号，避免过时）
function Get-LatestVersion([string]$groupId, [string]$artifactId) {
    $metaUrl = "https://repo1.maven.org/maven2/$groupId/$artifactId/maven-metadata.xml"
    $xml = & curl.exe -s --max-time 60 $metaUrl
    if ($LASTEXITCODE -ne 0 -or -not $xml) { throw "无法读取 Maven metadata：$metaUrl" }
    # curl 多行输出 → 数组；-match 数组模式不填充 $Matches，先并成标量
    $xmlText = $xml -join "`n"
    if ($xmlText -match "<release>([^<]+)</release>") { return $Matches[1] }
    if ($xmlText -match "<latest>([^<]+)</latest>") { return $Matches[1] }
    throw "metadata 里没有 release/latest：$metaUrl"
}

function Download-Jar([string]$groupId, [string]$artifactId, [string]$version, [string]$targetDir) {
    $url = "https://repo1.maven.org/maven2/$groupId/$artifactId/$version/$artifactId-$version.jar"
    $target = Join-Path $targetDir "$artifactId-$version.jar"
    if (Test-Path -LiteralPath $target) {
        Write-Output "已存在：$target"
        return $target
    }
    Write-Output "下载 $url ..."
    & curl.exe -sL --max-time 300 -o $target $url
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $target)) {
        throw "下载失败：$url"
    }
    Write-Output "下载完成：$target"
    return $target
}

# ---- 1. Java 壳构建 ----
$shellBuild = Join-Path $tools "rds-jdbc-shell\build.ps1"
if (-not (Test-Path $shellBuild)) { throw "未找到 Java 壳构建脚本：$shellBuild" }
& $shellBuild
$shellJar = Join-Path $tools "rds-jdbc-shell\lib\rds-jdbc-shell.jar"
if (-not (Test-Path $shellJar)) { throw "Java 壳构建产物缺失：$shellJar" }

# ---- 2. 组装 oracle-jdbc/lib ----
$oracleLib = Join-Path $dist "oracle-jdbc\lib"
New-Item -ItemType Directory -Force -Path $oracleLib | Out-Null
Copy-Item -LiteralPath $shellJar -Destination $oracleLib -Force

$h2Version = Get-LatestVersion $H2_GROUP $H2_ARTIFACT
Download-Jar $H2_GROUP $H2_ARTIFACT $h2Version $oracleLib | Out-Null

if (-not $SkipOjdbc) {
    $ojdbcVersion = Get-LatestVersion $OJDBC_GROUP $OJDBC_ARTIFACT
    Download-Jar $OJDBC_GROUP $OJDBC_ARTIFACT $ojdbcVersion $oracleLib | Out-Null
} else {
    Write-Output "已跳过 ojdbc 下载（-SkipOjdbc）：oracle 插件仍可安装，真连 Oracle 需手动放入 ojdbc jar"
}

# ---- 3. 构建两个 Rust 原生驱动 ----
if (-not $SkipRust) {
    $rustPackages = @("oracle-rust-plugin", "mssql-rust-plugin")
    foreach ($pkg in $rustPackages) {
        $build = Join-Path $dist "$pkg\build.ps1"
        if (-not (Test-Path -LiteralPath $build)) { throw "未找到 Rust 驱动构建脚本：$build" }
        $buildArgs = @()
        if ($RustDebug) { $buildArgs += "-Debug" }
        try {
            & $build @buildArgs
        } catch {
            throw "Rust 驱动构建失败：$pkg / $($_.Exception.Message)"
        }
    }
} else {
    Write-Output "已跳过 Rust 驱动构建（-SkipRust）：要求各包 bin/win-x64/agent.exe 已就绪"
}

# ---- 4. 安装到 RDS_HOME/plugins ----
# 注意：安装目录名 = manifest 里的 plugin.id（卸载用 paths::plugin_dir(id) 拼路径，
# 目录名与 id 不一致会导致卸载找不到目录）；源码目录是短名，安装目标用点分 id。
$homePlugins = Join-Path $root ".rds\plugins"
New-Item -ItemType Directory -Force -Path $homePlugins | Out-Null
$targets = @(
    @{ src = "oracle-jdbc"; id = "rds.driver.oracle" },
    @{ src = "postgres-py"; id = "rds.driver.postgres-py" },
    @{ src = "oracle-rust-plugin"; id = "rds.driver.oracle-rust" },
    @{ src = "mssql-rust-plugin"; id = "rds.driver.mssql-rust" }
)
foreach ($t in $targets) {
    $src = Join-Path $dist $t.src
    $dst = Join-Path $homePlugins $t.id
    if (Test-Path -LiteralPath $dst) {
        Write-Output "覆盖安装：$dst"
        Remove-Item -Recurse -Force -LiteralPath $dst
    }
    Copy-Item -Recurse -LiteralPath $src -Destination $dst
    Write-Output "已安装：$dst"
}

Write-Output "---- 插件包安装完成。宿主启动后扫描 .rds/plugins 即加载四个驱动。----"
