# rds-jdbc-shell smoke 测试：真实进程 + 分帧协议全链路（H2 内存库）
# 覆盖：initialize 握手 → session.open → query.execute（建表/插入/查询/参数化）→
#       meta.catalogs/schemas/objects → driver.describe → session.close → shutdown
# 用法：powershell -ExecutionPolicy Bypass -File smoke.ps1

$ErrorActionPreference = "Stop"
$root = "D:\RdataStation\RDS\RdataStation-v2\tools\rds-jdbc-shell"
$jar = Join-Path $root "lib\rds-jdbc-shell.jar"
if (-not (Test-Path $jar)) { throw "未找到 $jar（先跑 build.ps1）" }

$psi = [System.Diagnostics.ProcessStartInfo]::new()
$psi.FileName = "java"
if ($psi.ArgumentList) {
    # .NET Core 3+（pwsh 7）
    $psi.ArgumentList.Add("-jar")
    $psi.ArgumentList.Add($jar)
} else {
    # .NET Framework（Windows PowerShell 5.1）
    $psi.Arguments = "-jar `"$jar`""
}
$psi.RedirectStandardInput = $true
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.UseShellExecute = $false
$p = [System.Diagnostics.Process]::Start($psi)
$stdin = $p.StandardInput.BaseStream
$stdout = $p.StandardOutput.BaseStream

# ---- 分帧读写 ----
function Send-Frame([string]$json) {
    $payload = [System.Text.Encoding]::UTF8.GetBytes($json)
    $total = 5 + $payload.Length
    $hdr = [byte[]]::new(5)
    $hdr[0] = ($total -shr 24) -band 0xFF
    $hdr[1] = ($total -shr 16) -band 0xFF
    $hdr[2] = ($total -shr 8) -band 0xFF
    $hdr[3] = $total -band 0xFF
    $hdr[4] = 0x01
    $stdin.Write($hdr, 0, 5)
    $stdin.Write($payload, 0, $payload.Length)
    $stdin.Flush()
}

function Read-Frame {
    $hdr = [byte[]]::new(5)
    $read = 0
    while ($read -lt 5) {
        $n = $stdout.Read($hdr, $read, 5 - $read)
        if ($n -le 0) { throw "进程输出提前结束" }
        $read += $n
    }
    $total = (($hdr[0] -band 0xFF) -shl 24) -bor (($hdr[1] -band 0xFF) -shl 16) -bor (($hdr[2] -band 0xFF) -shl 8) -bor ($hdr[3] -band 0xFF)
    if ($hdr[4] -ne 0x01) { throw "非 JSON 帧种类 $($hdr[4])" }
    $payload = [byte[]]::new($total - 5)
    $read = 0
    while ($read -lt $payload.Length) {
        $n = $stdout.Read($payload, $read, $payload.Length - $read)
        if ($n -le 0) { throw "进程输出提前结束（载荷）" }
        $read += $n
    }
    return [System.Text.Encoding]::UTF8.GetString($payload)
}

# paramsObj 可以是 hashtable / 数组 / $null——内部用 ConvertTo-Json 序列化（避免手写转义）
function Invoke-Rpc([int]$id, [string]$method, $paramsObj) {
    $frame = @{ jsonrpc = "2.0"; id = $id; method = $method; params = $paramsObj } | ConvertTo-Json -Compress -Depth 12
    Send-Frame $frame
    $resp = Read-Frame | ConvertFrom-Json
    if ($resp.error) { throw "RPC $method 失败 [$($resp.error.code)]: $($resp.error.message)" }
    return $resp.result
}

# ---- 断言辅助 ----
$checks = 0
function Assert-True([bool]$cond, [string]$msg) {
    $script:checks++
    if (-not $cond) { throw "断言失败：$msg" }
    Write-Output ("[PASS] " + $msg)
}

try {
    # 1. initialize 握手
    $r = Invoke-Rpc 1 "initialize" @{ protocol = 1; host = @{ name = "smoke"; version = "0.0.1" } }
    Assert-True ($r.protocol -eq 1) "initialize 返回 protocol=1"
    Assert-True ($r.driver_ids -contains "jdbc") "initialize 返回 driver_ids"

    # 2. 会话（H2 内存库）
    Invoke-Rpc 2 "session.open" @{ session_id = "s1"; driver_id = "h2"; params = @{ driver = "h2"; url_override = "jdbc:h2:mem:rds" } } | Out-Null
    Assert-True $true "session.open 成功"

    # 3. driver.describe（会话方法，需带 session_id）
    $d = Invoke-Rpc 3 "driver.describe" @{ session_id = "s1"; driver_id = "h2" }
    Assert-True ($d.capabilities.schemas -eq $true) "describe 能力声明 schemas=true"
    Assert-True ($d.capabilities.cancel -eq $true) "describe 能力声明 cancel=true"

    # 4. 建表 + 插入（DML）→ 查询
    $ddl = "CREATE TABLE t(id INT PRIMARY KEY, name VARCHAR(50)); INSERT INTO t VALUES (1,'a'),(2,'b');"
    $q = Invoke-Rpc 4 "query.execute" @{ session_id = "s1"; request_id = "q1"; sql = $ddl; params = @(); max_rows = 100; timeout_ms = $null }
    Assert-True ($null -ne $q.affected_rows) "DML 返回 affected_rows"

    $q2 = Invoke-Rpc 5 "query.execute" @{ session_id = "s1"; request_id = "q2"; sql = "SELECT * FROM t ORDER BY id"; params = @(); max_rows = 100; timeout_ms = $null }
    Assert-True ($q2.columns.Count -eq 2) "查询返回 2 列"
    Assert-True ($q2.rows.Count -eq 2) "查询返回 2 行"
    Assert-True ($q2.rows[0].id -eq 1 -and $q2.rows[1].name -eq "b") "行内容正确（id=1, name=b）"
    Assert-True ($q2.truncated -eq $false) "小结果未截断"

    # 5. meta 三件套
    Invoke-Rpc 6 "meta.catalogs" @{ session_id = "s1" } | Out-Null
    $s = Invoke-Rpc 7 "meta.schemas" @{ session_id = "s1"; catalog = $null }
    Assert-True ($s.schemas.Count -ge 1) "meta.schemas 返回 schema"
    # 表建在 PUBLIC（H2 默认），逐个 schema 找
    $foundT = $false
    foreach ($sc in $s.schemas) {
        $o = Invoke-Rpc 8 "meta.objects" @{ session_id = "s1"; catalog = $null; schema = $sc.name }
        # @(...) 强制数组：过滤结果单元素时 .Count 在 PS5.1 上是 null
        if (@($o.objects | Where-Object { $_.name -eq "T" -or $_.name -eq "t" }).Count -ge 1) { $foundT = $true; break }
    }
    Assert-True $foundT "meta.objects 含表 T"

    # 6. 参数化查询（prepared）
    $q3 = Invoke-Rpc 9 "query.execute" @{ session_id = "s1"; request_id = "q3"; sql = "SELECT name FROM t WHERE id = ?"; params = @(1); max_rows = 10; timeout_ms = $null }
    Assert-True ($q3.rows.Count -eq 1 -and $q3.rows[0].name -eq "a") "参数化查询返回正确行"

    # 7. query.cancel 协议契约：路由通 + 幂等 + 与 execute 并发不串帧
    #    （H2 对 SLEEP/SYSTEM_RANGE 的 cancel 无法可靠复现"执行中"阻塞；
    #     Statement.cancel() 的深度验证留给 #39 真实 Oracle/PG 插件）
    $reqExec = @{ jsonrpc = "2.0"; id = 20; method = "query.execute"; params = @{ session_id = "s1"; request_id = "qc"; sql = "SELECT * FROM t ORDER BY id"; params = @(); max_rows = 100; timeout_ms = $null } } | ConvertTo-Json -Compress -Depth 12
    $reqCancel = @{ jsonrpc = "2.0"; id = 21; method = "query.cancel"; params = @{ session_id = "s1"; request_id = "qc" } } | ConvertTo-Json -Compress -Depth 12
    Send-Frame $reqExec
    Send-Frame $reqCancel
    $r1 = Read-Frame | ConvertFrom-Json
    $r2 = Read-Frame | ConvertFrom-Json
    # execute 可能已跑完（成功）也可能被取消（-32004）——两者都算契约内
    Assert-True (($null -eq $r1.error) -or ($r1.error.code -eq -32004)) "cancel 与 execute 并发不串帧"
    Assert-True ($null -eq $r2.error) "query.cancel 返回成功"
    # 幂等：取消不存在的请求也返回成功
    $reqCancel2 = @{ jsonrpc = "2.0"; id = 22; method = "query.cancel"; params = @{ session_id = "s1"; request_id = "nonexistent" } } | ConvertTo-Json -Compress -Depth 12
    Send-Frame $reqCancel2
    $r3 = Read-Frame | ConvertFrom-Json
    Assert-True ($null -eq $r3.error) "cancel 幂等（不存在的请求不报错）"

    # 8. 收摊
    Invoke-Rpc 10 "session.close" @{ session_id = "s1" } | Out-Null
    Invoke-Rpc 11 "shutdown" @{} | Out-Null
    $p.WaitForExit(5000) | Out-Null

    Write-Output "---- smoke 全部通过（$checks 项断言）----"
    exit 0
} finally {
    if (-not $p.HasExited) { $p.Kill() }
}
