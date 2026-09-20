---
name: rds-plugin-author
description: RdataStation 驱动插件（sidecar 子进程）编写与自检规范：帧格式、五条硬约定、meta.* 形状、能力门控、错误码、Arrow 兼容子集、无孤儿进程约定。要写/改一个 RdataStation 驱动插件（Go/Python/任何语言），或改动 crates/plugin 的 sidecar 协议时使用。
---

# 驱动插件（sidecar）编写规范

## 先读（事实来源，按顺序）

| 文件 | 给什么 |
| --- | --- |
| `docs/architecture/plugin/plugin-user-guide.md` §2.8 | 上手：协议要点 + 自检命令 + 五条硬约定（**最先读**） |
| `docs/architecture/plugin/plugin-dev-plan.md` §4.2 / §4.2.2.1 / §4.4 | 帧与阈值 / `meta.*` 线格式 / 能力门控口径 |
| `crates/plugin/src/sidecar/proto.rs` | 帧格式、`PROTOCOL_VERSION`、错误码、内联阈值的**代码真源** |
| `crates/plugin/src/sidecar/meta.rs` | `meta.*` 的解析口径与「认不出的类别怎么办」 |
| `crates/plugin/tests/fixture/sidecar.rs` | **独立实现**一遍帧编解码的测试对端（照它写最快） |
| `crates/plugin/tests/sidecar_conformance.rs` | 六条自检（P1 五条 + P2 导航面）= 插件能不能上线的裁决 |

## 形态选择：什么时候必须做 sidecar

- **sidecar**（子进程 + stdio）：要用**别的语言**（Go / Python / JVM），或要连**真库**（JDBC、psycopg 那类生态在别处）。
- **wasm**（进程内沙箱）：纯数据变换（P3 还在收，别指望它能连库）。
- 清单落在插件目录的 `manifest.toml`：`[backend]`（`kind`/`executable`/`transport`/`protocol`/`platforms`/`max_instances`）+ `[[contributes.drivers]]`。

## 协议要点（照着做，别自由发挥）

```text
帧： [u32 大端 total_len][u8 kind][payload]        total_len 含头 5 字节
     kind 0x01 = JSON-RPC 2.0（UTF-8）             单帧上限 64 MiB
     kind 0x02 = Arrow IPC stream 分片
宿主 → 你： initialize / driver.describe / session.open / session.close /
            session.ping / ping / query.execute / query.fetch / query.cancel /
            meta.catalogs / meta.schemas / meta.objects / meta.object_detail /
            meta.routine_source
你 → 宿主： 响应（结果大时：JSON 头 + 紧跟 N 个 0x02 帧）；
            stderr 落 plugin-cache/<id>/sidecar.log
```

**五条硬约定**（每条都有自动检查，见 `sidecar_conformance.rs`）：

1. `initialize` 里报 `protocol = 1` —— 不一致宿主**当场拒绝加载**；
2. `driver.describe` 给得出 `display_name` 与 `capabilities`（**没说的一律按不支持**）；
3. 超过 200 行**或**超过 256 KiB 的结果走 Arrow 附件：JSON 头写
   `attachments:[{id:0,kind:"arrow-ipc-stream",frames:N}]`，随后紧跟 N 个 `0x02` 帧；
4. `query.cancel` 要**真的中断**在跑的查询，让它以 `-32004` 收场（不是把请求丢掉）；
5. **stdin 见 EOF 立即退出** —— 宿主死了就自己收场。宿主不依赖平台相关的杀进程组，就靠这一条；
   不守它，用户机器上会留后台进程。

**几条容易写错的细节**：

- `session_id` / `request_id` **宿主发号**，你只回声（别自己造一套 id 表）；
- 小结果**可以内联**（≤ 200 行**且** ≤ 256 KiB）：`rows: [{"列名": 值}, …]` —— **对象，不是位置数组**；
- 内联行的时间用 **RFC3339 字符串**；Arrow 侧才是 `Timestamp(us, UTC)`；
- 空结果集**也必须有 `columns`**（少了 schema，网格与类型映射都无从下手）；
- 连不上库用 **`-32009 connect_failed`**（不要用 `-32003`）：界面要把它与「SQL 写错了」分开；
- 错误码：`-32001` 驱动不支持 / `-32002` 会话不存在 / `-32003` SQL 错（可带 `sqlstate`+`position`）/
  `-32004` 已取消 / `-32005` 超时 / `-32006` 能力被拒 / `-32007` 协议版本 / `-32008` 资源上限 / `-32009` 连接失败。

**Arrow 兼容子集（硬性）**：little-endian；`LargeUtf8` / `LargeBinary`（64 位 offset）；
时间归一化到 `Timestamp(us, UTC)` 或 `Date32`；**每页一条自洽 stream**（自带 schema）；
类型映射写在 field metadata 的 `rds.*` 键上（`rds.type_raw` / `rds.canonical` / `rds.nullable` / `rds.format`）；
嵌套类型先转 JSON 字符串；不用扩展类型。

## 导航面：`meta.*` 五件

| 方法 | 返回 |
| --- | --- |
| `meta.catalogs` | `{catalogs: [string]}` —— **第一层不能空**：拿不出 catalog 概念的库（MySQL 类）把 schema 名当 catalog 回 |
| `meta.schemas` | `{schemas: [{name, comment?}]}` —— 没声明 `schemas` 能力就返回空（宿主会把 catalog 当 schema 传） |
| `meta.objects` | `{objects: [{name, kind, comment?, parent?}]}` —— **一次给全、含 kind**（五个文件夹共用这一次内省） |
| `meta.object_detail` | `{object: {name, kind}, columns: [ColumnMeta], indexes?, row_count?}` |
| `meta.routine_source` | 字符串 或 `null`（**只有这两种形状**，别包一层对象） |

`kind` 取值：`table` / `view` / `materialized_view` / `procedure` / `function` / `sequence` / `trigger`。
`ColumnMeta`：`{name, type_raw, canonical?, nullable?, is_pk?, is_fk?, default?, format?, comment?}`。

三条最容易踩的：① **列一定带 `type_raw`，能给就给 `canonical`**（少了它 mock 与质量分只能猜首词，最后全落文本）；
② **能力没声明的东西别报**（`sequences = false` 就不在 `meta.objects` 里给序列），被点名的调用如实回 `-32006`；
③ **认不出的类别别硬塞**（报了我们也会跳过并记日志，摆进树会变成假信息）。

## 能力门控（`driver.describe` 的 `capabilities`）

| 键 | 门控什么 |
| --- | --- |
| `schemas` | 导航给不给 schema 层（**没声明 = 没有**，catalog 直接挂五个文件夹） |
| `cancel` | 用户点「中断」能不能真停（没声明时界面会如实说「没能停下来」） |
| `transactions` | 开事务（没声明 = 明确拒绝，不留半截写入） |
| `views` / `routines` / `sequences` / `triggers` | 你**报不报**那类对象（驱动侧的事） |
| `cursor` / `streaming` / `explain` / `readonly` | 协议面还没接，先不用声明 |

## 自检（唯一裁决）

```sh
cargo test -p rds-plugin --test sidecar_conformance -- --nocapture

# 换成你自己的二进制（Go / Python / 任何语言）
RDS_SIDECAR_BIN=./your-sidecar RDS_SIDECAR_DRIVER=postgres \
RDS_SIDECAR_PARAMS='{"host":"127.0.0.1","port":5432,"database":"demo","username":"me","password":"…"}' \
RDS_SIDECAR_SQL_BIG='select * from generate_series(1, 3000)' \
RDS_SIDECAR_SQL_SLOW='select pg_sleep(5)' \
cargo test -p rds-plugin --test sidecar_conformance -- --nocapture
```

`--nocapture` 会把实际答出来的 catalog / schema / 对象数 / 首列类型打出来，照着看就知道卡在哪一环。
库是空的也算过（那时只验「方法答得出来」）。

## 交付前检查清单

- [ ] `initialize` 报 `protocol = 1`，不一致时把两个版本号都带上
- [ ] `driver.describe` 的 `display_name` / `capabilities` / `identifier_quote` / `default_schema` 齐全
- [ ] 大结果走 Arrow 附件、小结果内联；两种承载**消费方看不出区别**
- [ ] 空结果集仍带 `columns`
- [ ] `query.cancel` 真中断（回 `-32004`），不是丢请求
- [ ] stdin EOF 立即退出（**别做「赖着不走」的开关**）
- [ ] 连不上库回 `-32009`；SQL 错回 `-32003` 且尽量带 `sqlstate` / `position`
- [ ] `meta.*` 五件齐全；`meta.objects` 一次给全含 `kind`
- [ ] 没声明的能力**不报数据**；被点名的调用回 `-32006`
- [ ] `sidecar_conformance` 六条全过（把 `RDS_SIDECAR_BIN` 换成你的二进制再跑一遍）
