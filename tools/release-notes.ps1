# 发布说明草稿：从 git 历史按中文提交前缀分组，输出 Markdown 骨架
#
# 用途：写 docs/release/notes/<标签名>.md 时先把素材摊开，再由人改写成「用户视角」的句子。
# 刻意**不直接发布**：脚本给的只是分组后的提交标题，句子要人写（口径见 docs/release/README.md）。
#
#   pwsh tools/release-notes.ps1 -From RDS-beta-0.2.7 -Head HEAD
#   pwsh tools/release-notes.ps1 -From RDS-beta-0.2.7 -Head RDS-beta-0.2.8 -Out .rds/tmp/notes-draft.md
#
# 依赖：git（工作目录须在仓库内）。输出默认走 stdout，落盘请自己重定向到 .rds/tmp/。

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$From,

    [string]$Head = 'HEAD',

    # 目标文件；不给就写 stdout
    [string]$Out
)

$ErrorActionPreference = 'Stop'

# 提交前缀 → 分区。docs/test/chore/build 归「内部」：默认不进正文（口径见 README）
$buckets = [ordered]@{
    'feat'     = @{ Title = '新增'; Keep = $true }
    'fix'      = @{ Title = '修复'; Keep = $true }
    'perf'     = @{ Title = '改进'; Keep = $true }
    'refactor' = @{ Title = '改进'; Keep = $true }
    'style'    = @{ Title = '改进'; Keep = $false }
    'other'    = @{ Title = '未分类（没有前缀或写法不规范）'; Keep = $false }
    'docs'     = @{ Title = '内部：文档'; Keep = $false }
    'test'     = @{ Title = '内部：测试'; Keep = $false }
    'chore'    = @{ Title = '内部：杂务'; Keep = $false }
    'build'    = @{ Title = '内部：构建与依赖'; Keep = $false }
    'ci'       = @{ Title = '内部：CI'; Keep = $false }
}

$root = (& git rev-parse --show-toplevel).Trim()
Push-Location $root
try {
    $range = "$From..$Head"
    (& git rev-parse --verify --quiet "$From") | Out-Null
    if (-not $?) { throw "找不到起点 ref：$From" }

    $commitCount = [int](& git rev-list --count --no-merges $range).Trim()
    $log = & git log --no-merges --format='%h%x09%s' $range

    $groups = @{}
    foreach ($key in $buckets.Keys) { $groups[$key] = @() }

    foreach ($line in $log) {
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        $parts = $line -split "`t", 2
        $sha = $parts[0]
        $subject = if ($parts.Count -gt 1) { $parts[1] } else { '' }

        if ($subject -match '^(?<type>[a-z]+)(\([^)]*\))?[!！]?:') {
            $type = $Matches.type
        } else {
            $type = 'other'
        }
        if (-not $buckets.Contains($type)) { $type = 'other' }
        $groups[$type] += "- $subject"
    }

    $sb = [System.Text.StringBuilder]::new()
    [void]$sb.AppendLine('# <产品名> <版本>')
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine("> <日期> · 相对 ``$From`` 的 $commitCount 个提交（未合并提交口径） · <beta / 定版说明>")
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('<!-- 下面是按提交前缀分组的素材：一条一句改写成人话，删掉不改变用户可见行为的条目 -->')
    [void]$sb.AppendLine('')

    [void]$sb.AppendLine('## 本次重点')
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('- <从下面分区里挑 3~5 条最值得说的>')
    [void]$sb.AppendLine('')

    foreach ($key in 'feat', 'fix', 'perf', 'refactor', 'style', 'other') {
        $items = $groups[$key]
        if ($items.Count -eq 0) { continue }
        $title = $buckets[$key].Title
        [void]$sb.AppendLine("## $title（$key，$($items.Count) 条）")
        [void]$sb.AppendLine('')
        foreach ($it in $items) { [void]$sb.AppendLine($it) }
        [void]$sb.AppendLine('')
    }

    $internalTotal = 0
    foreach ($key in 'docs', 'test', 'chore', 'build', 'ci') { $internalTotal += $groups[$key].Count }
    if ($internalTotal -gt 0) {
        [void]$sb.AppendLine('## 内部改动（默认不进正文）')
        [void]$sb.AppendLine('')
        [void]$sb.AppendLine('```')
        foreach ($key in 'docs', 'test', 'chore', 'build', 'ci') {
            foreach ($it in $groups[$key]) { [void]$sb.AppendLine($it) }
        }
        [void]$sb.AppendLine('```')
        [void]$sb.AppendLine('')
    }

    [void]$sb.AppendLine('## 下载与校验')
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('| 平台 | 文件 | 说明 |')
    [void]$sb.AppendLine('| --- | --- | --- |')
    [void]$sb.AppendLine('| <平台> | <归档名> | <解压即用 / 需要系统库> |')
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('## 已知问题')
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine('- <这一版明确没做的、需要用户绕的>')
    [void]$sb.AppendLine('')
    [void]$sb.AppendLine("**完整对比**：https://github.com/ling-love-xin/RdataStation-v2/compare/$From...$Head")

    $text = $sb.ToString().Replace("`r`n", "`n")
    if ($Out) {
        [System.IO.File]::WriteAllText((Join-Path $root $Out), $text, (New-Object System.Text.UTF8Encoding($false)))
        Write-Output "已写入 $Out（$commitCount 个提交）"
    } else {
        Write-Output $text
    }
}
finally {
    Pop-Location
}
