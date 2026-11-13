# rds 驱动插件包安装脚本：组装 postgres-py / oracle-rust / mssql-rust 并安装到 RDS_HOME/plugins。
#
# 用法：powershell -ExecutionPolicy Bypass -File tools\plugin-drivers\install.ps1 [-WithJdbcOracle] [-SkipOjdbc] [-SkipRust] [-RustDebug] [-DryRun]
#   -WithJdbcOracle：改装 JDBC 版 Oracle（rds.driver.oracle），**不进 Rust 版**。默认装
#                    Rust 版 Oracle：两版都贡献驱动 id `oracle`，同时安装会在加载期被
#                    跨插件冲突检测拒绝（两个插件都不可用）。
#   -SkipOjdbc：跳过 ojdbc 下载（离线环境；只装已就绪的 jar，oracle 插件仍可装但缺驱动）
#   -SkipRust：跳过两个 Rust 原生驱动的 cargo 构建（只装已有 bin/win-x64/agent.exe）
#   -RustDebug：Rust 驱动按 debug 档构建（默认 release）
#   -DryRun：只打印将构建 / 安装的目标，不执行构建、下载、删除或复制
#
# 产物原则：仓库不存驱动 jar / 壳 jar / 驱动二进制（.gitignore），全部运行时组装：
#   - Java 壳：tools/rds-jdbc-shell/build.ps1 构建 → 复制
#   - H2：从 Maven Central 下载（无证书验证用）
#   - ojdbc11：从 Maven Central 下载（Oracle 官方坐标；OTN 许可，见 oracle-jdbc/README.md）
#   - Python 壳：源码已在 postgres-py/ 内（无需下载）
#   - Rust 驱动：cargo build → 复制到 <包>/bin/win-x64/agent.exe
# 安装目标：<RDS_HOME>/plugins/<manifest-id>（RDS_HOME 开发期 = 仓库根/.rds，见 crates/paths）
# 复制用 robocopy 并排除开发残留（.git / target / __pycache__ 等，与 layout::is_dev_residue 同口径）：
# 安装目录是只读产物，残留文件进去既不干净，运行期还会被重写、抖动载荷指纹。

param(
    [switch]$WithJdbcOracle,
    [switch]$SkipOjdbc,
    [switch]$SkipRust,
    [switch]$RustDebug,
    [switch]$DryRun,
    # 安装目标覆盖（默认 <repo>/.rds/plugins）；仅供冒烟测试 / 多环境安装使用。
    [string]$PluginsDir = ""
)

$ErrorActionPreference = "Stop"
# 从脚本位置反推仓库根：避免硬编码盘符在换机器 / 换目录后装到错误位置。
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$tools = Join-Path $root "tools"
$dist = Join-Path $tools "plugin-drivers"
$homePlugins = if ($PluginsDir) { [IO.Path]::GetFullPath($PluginsDir) } else { Join-Path $root ".rds\plugins" }

# 冲突预检放在任何构建 / 下载之前：目标目录里的 Rust 版 Oracle 与 JDBC 版贡献同一个
# 驱动 id `oracle`，共存会让加载期直接拒绝两者。两版互斥：默认装 Rust 版，`-WithJdbcOracle`
# 只装 JDBC 版；发现另一版已在目标目录时直接拒绝，脚本不替用户删。
$rustOracleDir = Join-Path $homePlugins 'rds.driver.oracle-rust'
$jdbcOracleDir = Join-Path $homePlugins 'rds.driver.oracle'
if ($WithJdbcOracle -and (Test-Path -LiteralPath $rustOracleDir)) {
    if ($DryRun) {
        Write-Output "[DryRun] 检测到冲突：$rustOracleDir 已存在，实际安装会被拒绝"
    } else {
        throw "拒绝安装：$rustOracleDir 已存在，与 JDBC 版 Oracle 的驱动 id 撞车。请先移走该目录再重试。"
    }
}
if ((-not $WithJdbcOracle) -and (Test-Path -LiteralPath $jdbcOracleDir)) {
    if ($DryRun) {
        Write-Output "[DryRun] 检测到冲突：$jdbcOracleDir 已存在，默认安装 Rust 版会被拒绝"
    } else {
        throw "拒绝安装：$jdbcOracleDir 已存在（JDBC 版 Oracle），与默认安装的 Rust 版驱动 id 撞车。请先移走该目录，或改用 -WithJdbcOracle 只装 JDBC 版。"
    }
}

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

# ---- 1/2. JDBC 版 Oracle（仅在显式 -WithJdbcOracle 时组装） ----
# 默认不装的原因见文件头：JDBC 与 Rust 两版都贡献驱动 id `oracle`，同时安装会被
# 加载期跨插件冲突检测拒绝。当前主线是 Rust 原生驱动，JDBC 版只在回退时使用。
if ($WithJdbcOracle) {
    if ($DryRun) {
        Write-Output "[DryRun] 将构建 Java 壳并下载 h2 / ojdbc11 到 oracle-jdbc/lib"
    } else {
        # 1. Java 壳构建
        $shellBuild = Join-Path $tools "rds-jdbc-shell\build.ps1"
        if (-not (Test-Path $shellBuild)) { throw "未找到 Java 壳构建脚本：$shellBuild" }
        & $shellBuild
        $shellJar = Join-Path $tools "rds-jdbc-shell\lib\rds-jdbc-shell.jar"
        if (-not (Test-Path $shellJar)) { throw "Java 壳构建产物缺失：$shellJar" }

        # 2. 组装 oracle-jdbc/lib
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
    }
} else {
    Write-Output "默认不安装 JDBC 版 Oracle（-WithJdbcOracle 可显式开启）：它与 Rust 版驱动 id 撞车"
}

# ---- 3. 构建两个 Rust 原生驱动 ----
if (-not $SkipRust) {
    $rustPackages = @("oracle-rust-plugin", "mssql-rust-plugin")
    foreach ($pkg in $rustPackages) {
        $build = Join-Path $dist "$pkg\build.ps1"
        if (-not (Test-Path -LiteralPath $build)) { throw "未找到 Rust 驱动构建脚本：$build" }
        if ($DryRun) {
            Write-Output "[DryRun] 将构建 $pkg（$build）"
            continue
        }
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
# Oracle 两版互斥：JDBC 与 Rust 版都贡献驱动 id `oracle`，一次只能装其中一版。

# 生成 checksums.json 的工具：优先用已编译的二进制（快），没有就退回 `cargo run`。
# 两者都没有时跳过——缺该文件不阻断加载（老包兼容口径），只是少一层完整性。
function Get-ChecksumTool {
    $candidates = @(
        (Join-Path $root ".rds\target-gf\debug\rds-plugin-checksums.exe"),
        (Join-Path $root ".rds\target-gf\release\rds-plugin-checksums.exe"),
        (Join-Path $root "target\debug\rds-plugin-checksums.exe"),
        (Join-Path $root "target\release\rds-plugin-checksums.exe")
    ) | Where-Object { Test-Path -LiteralPath $_ }
    if ($candidates) {
        return @{ exe = ($candidates | Select-Object -First 1) }
    }
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        return @{ cargo = $true }
    }
    return $null
}

# 装的字节就是校验对象：复制完再在**目标目录**生成，源码目录不会多出未跟踪文件。
function New-PackageChecksums([string]$dir, $tool) {
    if (-not $tool) {
        Write-Output "跳过 checksums.json（未找到 rds-plugin-checksums，也没有 cargo）：$dir"
        return
    }
    if ($tool.exe) {
        & $tool.exe $dir
        if ($LASTEXITCODE -ne 0) { throw "生成 checksums.json 失败：$dir" }
        return
    }
    Push-Location $root
    try {
        & cargo run -q -p rds-plugin --bin rds-plugin-checksums -- $dir
        if ($LASTEXITCODE -ne 0) { throw "生成 checksums.json 失败：$dir" }
    } finally {
        Pop-Location
    }
}

$checksumTool = Get-ChecksumTool

$targets = @(
    @{ src = "postgres-py"; id = "rds.driver.postgres-py" },
    @{ src = "mssql-rust-plugin"; id = "rds.driver.mssql-rust" }
)
if ($WithJdbcOracle) {
    $targets = @(@{ src = "oracle-jdbc"; id = "rds.driver.oracle" }) + $targets
} else {
    $targets = @($targets[0], @{ src = "oracle-rust-plugin"; id = "rds.driver.oracle-rust" }, $targets[1])
}
if (-not $DryRun) {
    New-Item -ItemType Directory -Force -Path $homePlugins | Out-Null
}
foreach ($t in $targets) {
    $src = Join-Path $dist $t.src
    $dst = Join-Path $homePlugins $t.id
    if ($DryRun) {
        Write-Output "[DryRun] 将安装 $src → $dst，并在安装后生成 checksums.json"
        continue
    }
    if (Test-Path -LiteralPath $dst) {
        Write-Output "覆盖安装：$dst"
        Remove-Item -Recurse -Force -LiteralPath $dst
    }
    # 用 robocopy 而不是 Copy-Item：后者会把 __pycache__ / .git / target 一起搬进安装目录。
    # 安装目录是只读产物（layout 规则 6），Python 运行期还会重写 __pycache__，载荷指纹跟着变，
    # 可能触发无谓的自动重启。排除清单与 layout::is_dev_residue 同一套。
    $rcArgs = @(
        $src, $dst, '/E',
        '/XD', '.git', '.svn', '.hg', '.rds', '.idea', '.vscode', 'target', 'node_modules',
        '__pycache__', '.pytest_cache', '.mypy_cache',
        '/XF', '.DS_Store', 'Thumbs.db',
        '/NFL', '/NDL', '/NJH', '/NJS', '/NP'
    )
    & robocopy @rcArgs | Out-Null
    # robocopy：0 = 没变化、1-7 = 复制成功（有文件被拷），≥8 才是失败
    if ($LASTEXITCODE -ge 8) { throw "复制插件目录失败（robocopy $LASTEXITCODE）：$src → $dst" }
    New-PackageChecksums $dst $checksumTool
    Write-Output "已安装：$dst"
}

if ($DryRun) {
    Write-Output "---- DryRun 结束：未构建、未下载、未删除、未复制任何文件。----"
} else {
    Write-Output "---- 插件包安装完成。宿主启动后扫描 .rds/plugins 即加载已安装的驱动。----"
}
