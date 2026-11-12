//! PG 的**二进制 wire 格式** → 展示文本（`postgres-types` 没给解码器的那几类）。
//!
//! ## 为什么自己解
//!
//! `postgres-types` 0.2.x **没有任何 decimal feature**（`NUMERIC` 没有 `FromSql`），
//! `INTERVAL` 也一样没有；`inet` / `point` 在 `postgres-protocol` 里有解码器但没接到
//! `FromSql` 上。结果是 Official 那个 PG 驱动把这些列一律交成 **NULL**（真机实测：
//! 金额、时间间隔、网络地址、几何点全变空白格），而 sqlx 那个驱动有 `BigDecimal` 与
//! `PgInterval` —— 同一个库的两个驱动行为不一致。
//!
//! 于是按协议文档自己解（字节一律**大端**）。格式对齐 **PG 自己的文本输出**：
//! `SELECT 1.50::numeric` 在 psql 里打出什么，这里就出什么，这样两个驱动在界面上
//! 看到同一口径。
//!
//! | 类型 | wire 布局 | 出处 |
//! | --- | --- | --- |
//! | `NUMERIC` | `ndigits i16, weight i16, sign u16, dscale i16, digits[i16; ndigits]` | PG 文档 §8.1「numeric 的内部格式」 |
//! | `INTERVAL` | `microseconds i64, days i32, months i32` | `postgres-protocol` 的 `interval`（0.6.12 只有 `interval_to_sql`） |
//! | `INET` / `CIDR` | `family u8, netmask u8, is_cidr u8, len u8, addr[len]` | `postgres-protocol-0.6.12/src/types/mod.rs::inet_from_sql`（family：2=IPv4、3=IPv6） |
//! | `POINT` | `x f64, y f64` | 同文件 `point_from_sql` |
//!
//! ## 与 sqlx 插件共用同一份解析
//!
//! sqlx 对 `inet` / `point` **没有**解码器（`ipnetwork` feature 才管 `inet`，还要多一个
//! 依赖；`point` 完全没有），而字节格式是同一套 —— 解析写在 `crates/driver-sql`
//! 一份，内核官方驱动与 sqlx 插件共用，免得「同一个库两个驱动、两套格式化」。
//! （D3 之后 sqlx 实现在插件里，本文件只把这份解析挂到 tokio-postgres 的 `FromSql` 上。）
//!
//! ## 格式口径（与 PG 文本输出对齐，逐条都有真机断言）
//!
//! * `NUMERIC`：按 `dscale` 出小数位（`1234.56` 就是 `1234.56`，不补成 `1234.5600`）；
//!   `NaN` / `Infinity` / `-Infinity` 按 PG 的写法。**注**：sqlx 插件那条路走
//!   `BigDecimal::to_string()`，标度会多带（它按 base-10000 分组拼），两边值相等、
//!   写法可能差尾零 —— 真机用例各按各的口径断言。
//! * `INTERVAL`：`N year(s)` / `N mon(s)` / `N day(s)` + `HH:MM:SS[.ffffff]`，各段自带符号
//!   （PG 会在时间前补 `+`，我们不打；对混合符号的区间两者文字略不同、值相同）。
//! * `INET`：`addr/netmask`，netmask 是满位（IPv4 的 32 / IPv6 的 128）且不是 `cidr` 时
//!   只打地址（与 `inet_out` 一致）。
//! * `POINT`：`(x,y)`。

// 纯解码实现在 `crates/driver-sql/src/wire.rs`（内核官方 PG 驱动 + sqlx 插件共用一份）；
// 这里只 re-export，并把它挂到 `tokio_postgres::types::FromSql` 上。
pub use rds_driver_sql::wire::{
    array_literal, inet_to_text, interval_parts_to_text, interval_to_text, numeric_to_text,
    point_to_text,
};

/// `NUMERIC` 的 OID（`pg_type.oid`；两个客户端库用同一套）。
const OID_NUMERIC: u32 = 1700;
/// `INTERVAL` 的 OID。
const OID_INTERVAL: u32 = 1186;
/// `INET` 的 OID。
const OID_INET: u32 = 869;
/// `POINT` 的 OID。
const OID_POINT: u32 = 600;

/// 给四类各生成一个「解出来就是文本」的包装类型，并挂到 tokio-postgres 的 `FromSql`
/// 上（官方 `postgres_native` 驱动靠它取 `PgNumeric` / `PgInterval` / `PgInet` / `PgPoint`）。
///
/// 只认二进制格式：tokio-postgres 对这几类按我们声明的类型取参数时发的是二进制
/// （文本格式走不到这里，也就不用管）。
macro_rules! pg_wire_type {
    ($name:ident, $oid:expr, $parse:path, $doc:literal) => {
        #[doc = $doc]
        ///
        /// 解出来就是展示文本（出口层这几类一律是文本）。
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name(pub String);

        impl<'a> tokio_postgres::types::FromSql<'a> for $name {
            fn from_sql(
                _ty: &tokio_postgres::types::Type,
                raw: &'a [u8],
            ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
                Ok(Self($parse(raw)?))
            }

            fn accepts(ty: &tokio_postgres::types::Type) -> bool {
                ty.oid() == $oid
            }
        }

    };
}

pg_wire_type!(
    PgNumeric,
    OID_NUMERIC,
    numeric_to_text,
    "`NUMERIC` 的 wire 解码（`postgres-types` 没有 decimal feature，只能自己解）。"
);
pg_wire_type!(
    PgInterval,
    OID_INTERVAL,
    interval_to_text,
    "`INTERVAL` 的 wire 解码（`postgres-types` 没有 `FromSql`）。"
);
pg_wire_type!(
    PgInet,
    OID_INET,
    inet_to_text,
    "`INET` / `CIDR` 的 wire 解码（sqlx 侧要开 `ipnetwork` feature，我们直接解）。"
);
pg_wire_type!(
    PgPoint,
    OID_POINT,
    point_to_text,
    "`POINT` 的 wire 解码（两个客户端库都没有）。"
);

