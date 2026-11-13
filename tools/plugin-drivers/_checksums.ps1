# install.ps1 / pack.ps1 共用的 checksums 生成工具（dot-source 引入）。
#
# 为什么单独成文件：安装要生成一次、打包要生成一次，各写一份迟早会漂——
# 生成口径一漂，同一个包在两条路线上就得到不同的校验清单。

function Get-PluginDriversRepoRoot {
    Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
}

# 生成工具：优先用已编译的二进制（快），没有就退回 `cargo run`；两者都没有返回 $null。
function Get-ChecksumTool {
    $repoRoot = Get-PluginDriversRepoRoot
    $candidates = @(
        (Join-Path $repoRoot ".rds\target-gf\debug\rds-plugin-checksums.exe"),
        (Join-Path $repoRoot ".rds\target-gf\release\rds-plugin-checksums.exe"),
        (Join-Path $repoRoot "target\debug\rds-plugin-checksums.exe"),
        (Join-Path $repoRoot "target\release\rds-plugin-checksums.exe")
    ) | Where-Object { Test-Path -LiteralPath $_ }
    if ($candidates) {
        return @{ exe = ($candidates | Select-Object -First 1); root = $repoRoot }
    }
    if (Get-Command cargo -ErrorAction SilentlyContinue) {
        return @{ cargo = $true; root = $repoRoot }
    }
    return $null
}

# 在 $dir 里生成 checksums.json。返回是否真的生成（$false = 没有工具，由调用方决定是跳过还是失败）。
function New-PackageChecksums([string]$dir, $tool) {
    if (-not $tool) {
        return $false
    }
    if ($tool.exe) {
        # | Out-Host：工具自己会打印「已生成 …」，不能让它混进返回值
        & $tool.exe $dir | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "生成 checksums.json 失败：$dir" }
        return $true
    }
    Push-Location $tool.root
    try {
        & cargo run -q -p rds-plugin --bin rds-plugin-checksums -- $dir | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "生成 checksums.json 失败：$dir" }
    } finally {
        Pop-Location
    }
    return $true
}

# 安装/打包共用的排除清单（与 layout::is_dev_residue 的目录、文件两张表同口径）。
function Get-DevResidueRobocopyArgs {
    return @(
        '/XD', '.git', '.svn', '.hg', '.rds', '.idea', '.vscode', 'target', 'node_modules',
        '__pycache__', '.pytest_cache', '.mypy_cache',
        '/XF', '.DS_Store', 'Thumbs.db',
        '/NFL', '/NDL', '/NJH', '/NJS', '/NP'
    )
}
