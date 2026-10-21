# rds-dbapi-shell smoke 测试：真实进程 + 分帧协议全链路（sqlite3 内存库）
# 覆盖：initialize → session.open → query.execute（建表/插入/查询/参数化）→
#       meta.schemas/objects → query.cancel（真取消，递归 CTE 阻塞）→ shutdown
# 用法：powershell -ExecutionPolicy Bypass -File smoke.ps1

$ErrorActionPreference = "Stop"
$root = "D:\RdataStation\RDS\RdataStation-v2\tools\rds-dbapi-shell"
$script = Join-Path $root "rds_dbapi_shell.py"
if (-not (Test-Path $script)) { throw "未找到 $script" }

$psi = [System.Diagnostics.ProcessStartInfo]::new()
$psi.FileName = "python"
if ($psi.ArgumentList) {
    $psi.ArgumentList.Add($script)
} else {
    $psi.Arguments = "`"$script`""
}
$psi.RedirectStandardInput = $true
$psi.RedirectStandardOutput = $true
$psi.RedirectStandardError = $true
$psi.UseShellExecute = $false
$p = [System.Diagnostics.Process]::Start($psi)
$stdin = $p.StandardInput.BaseStream
$stdout = $p.StandardOutput.BaseStream

# ---- 分帧读写（与 Java smoke 相同）----
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

function Invoke-Rpc([int]$id, [string]$method, $paramsObj) {
    $frame = @{ jsonrpc = "2.0"; id = $id; method = $method; params = $paramsObj } | ConvertTo-Json -Compress -Depth 12
    Send-Frame $frame
    $resp = Read-Frame | ConvertFrom-Json
    if ($resp.error) { throw "RPC $method 失败 [$($resp.error.code)]: $($resp.error.message)" }
    return $resp.result
}

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
    Assert-True ($r.driver_ids -contains "dbapi") "initialize 返回 driver_ids"

    # 2. 会话（sqlite3 内存库）
    Invoke-Rpc 2 "session.open" @{ session_id = "s1"; driver_id = "sqlite"; params = @{ driver = "sqlite"; file_path = ":memory:" } } | Out-Null
    Assert-True $true "session.open 成功"

    # 3. driver.describe
    $d = Invoke-Rpc 3 "driver.describe" @{ session_id = "s1"; driver_id = "sqlite" }
    Assert-True ($d.capabilities.cancel -eq $true) "describe 能力声明 cancel=true"

    # 4. 建表 + 插入（sqlite3 一次只允许一条语句，分开发）→ 查询
    Invoke-Rpc 4 "query.execute" @{ session_id = "s1"; request_id = "q1"; sql = "CREATE TABLE t(id INTEGER PRIMARY KEY, name TEXT)"; params = @(); max_rows = 100; timeout_ms = $null } | Out-Null
    $q = Invoke-Rpc 4 "query.execute" @{ session_id = "s1"; request_id = "q1b"; sql = "INSERT INTO t VALUES (1,'a'),(2,'b')"; params = @(); max_rows = 100; timeout_ms = $null }
    Assert-True ($null -ne $q.affected_rows) "DML 返回 affected_rows"

    $q2 = Invoke-Rpc 5 "query.execute" @{ session_id = "s1"; request_id = "q2"; sql = "SELECT * FROM t ORDER BY id"; params = @(); max_rows = 100; timeout_ms = $null }
    Assert-True ($q2.columns.Count -eq 2) "查询返回 2 列"
    Assert-True ($q2.rows.Count -eq 2) "查询返回 2 行"
    Assert-True ($q2.rows[0].id -eq 1 -and $q2.rows[1].name -eq "b") "行内容正确（id=1, name=b）"

    # 5. meta
    $s = Invoke-Rpc 6 "meta.schemas" @{ session_id = "s1"; catalog = $null }
    Assert-True (@($s.schemas | Where-Object { $_.name -eq "main" }).Count -ge 1) "meta.schemas 含 main"
    $o = Invoke-Rpc 7 "meta.objects" @{ session_id = "s1"; catalog = $null; schema = "main" }
    Assert-True (@($o.objects | Where-Object { $_.name -eq "t" }).Count -ge 1) "meta.objects 含表 t"

    # 6. 参数化查询
    $q3 = Invoke-Rpc 8 "query.execute" @{ session_id = "s1"; request_id = "q3"; sql = "SELECT name FROM t WHERE id = ?"; params = @(1); max_rows = 10; timeout_ms = $null }
    Assert-True ($q3.rows.Count -eq 1 -and $q3.rows[0].name -eq "a") "参数化查询返回正确行"

    # 7. 真取消：递归 CTE 大聚合阻塞 worker，cancel 打断 → execute 回 -32004
    $bigSql = "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM c WHERE x < 200000000) SELECT COUNT(*) FROM c"
    $reqExec = @{ jsonrpc = "2.0"; id = 30; method = "query.execute"; params = @{ session_id = "s1"; request_id = "qc"; sql = $bigSql; params = @(); max_rows = 1; timeout_ms = $null } } | ConvertTo-Json -Compress -Depth 12
    $reqCancel = @{ jsonrpc = "2.0"; id = 31; method = "query.cancel"; params = @{ session_id = "s1"; request_id = "qc" } } | ConvertTo-Json -Compress -Depth 12
    Send-Frame $reqExec
    Start-Sleep -Milliseconds 300  # 让 worker 进入聚合
    Send-Frame $reqCancel
    # 两个响应并发写回（cancel 打断 execute），按 id 归位而非按顺序
    $respExec = $null; $respCancel = $null
    for ($i = 0; $i -lt 2; $i++) {
        $r = Read-Frame | ConvertFrom-Json
        if ($r.id -eq 30) { $respExec = $r } else { $respCancel = $r }
    }
    Assert-True ($respExec.error.code -eq -32004) "cancel 打断聚合，execute 回 -32004"
    Assert-True ($null -eq $respCancel.error) "query.cancel 返回成功"

    # 8. 收摊
    Invoke-Rpc 9 "session.close" @{ session_id = "s1" } | Out-Null
    Invoke-Rpc 10 "shutdown" @{} | Out-Null
    $p.WaitForExit(5000) | Out-Null

    Write-Output "---- smoke 全部通过（$checks 项断言）----"
    exit 0
} finally {
    if (-not $p.HasExited) { $p.Kill() }
}
