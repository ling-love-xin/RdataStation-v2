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
//! ## 为什么也接 sqlx 那一侧
//!
//! sqlx 对 `inet` / `point` **没有**解码器（`ipnetwork` feature 才管 `inet`，还要多一个
//! 依赖；`point` 完全没有），而字节格式是同一套 —— 解析写在 `crates/driver-sql`
//! 一份，两个驱动 + sqlx 插件共用，免得「同一个库两个驱动、两套格式化」。
//!
//! ## 格式口径（与 PG 文本输出对齐，逐条都有真机断言）
//!
//! * `NUMERIC`：按 `dscale` 出小数位（`1234.56` 就是 `1234.56`，不补成 `1234.5600`）；
//!   `NaN` / `Infinity` / `-Infinity` 按 PG 的写法。**注**：sqlx 那条路走
//!   `BigDecimal::to_string()`，标度会多带（它按 base-10000 分组拼），两条路的值相等、
//!   写法可能差尾零 —— 真机用例各按各的口径断言。
//! * `INTERVAL`：`N year(s)` / `N mon(s)` / `N day(s)` + `HH:MM:SS[.ffffff]`，各段自带符号
//!   （PG 会在时间前补 `+`，我们不打；对混合符号的区间两者文字略不同、值相同）。
//! * `INET`：`addr/netmask`，netmask 是满位（IPv4 的 32 / IPv6 的 128）且不是 `cidr` 时
//!   只打地址（与 `inet_out` 一致）。
//! * `POINT`：`(x,y)`。

// 纯解码实现在 `crates/driver-sql/src/wire.rs`（内核两个 PG 驱动 + sqlx 插件共用一份）；
// 这里只 re-export，并把它们挂到两条后端的 trait 上。
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

/// 给四类各生成一个「解出来就是文本」的包装类型，并把两侧后端的 trait 都接上：
/// tokio-postgres（Official 驱动）走 `FromSql`，sqlx 走 `Type` + `Decode`。
///
/// `Format` 两种都要认：sqlx 对**它认识的**类型一律要二进制（我们这几类它认识 OID，
/// 只是没有解码器），但它也可能按文本回（例如服务端强制文本格式时）——文本本来就
/// 是人能看的，直接用。
macro_rules! pg_wire_type {
    ($name:ident, $oid:expr, $parse:path, $doc:literal) => {
        #[doc = $doc]
        ///
        /// 解出来就是展示文本（出口层这几类一律是文本，与 `sqlx_cell_as_text!` 同口径）。
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

        impl sqlx::Type<sqlx::Postgres> for $name {
            fn type_info() -> sqlx::postgres::PgTypeInfo {
                sqlx::postgres::PgTypeInfo::with_oid(sqlx::postgres::types::Oid($oid))
            }

            fn compatible(ty: &sqlx::postgres::PgTypeInfo) -> bool {
                ty.oid() == Some(sqlx::postgres::types::Oid($oid))
            }
        }

        impl<'r> sqlx::Decode<'r, sqlx::Postgres> for $name {
            fn decode(
                value: sqlx::postgres::PgValueRef<'r>,
            ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
                match value.format() {
                    // 文本格式本来就是展示文本（服务端已按它的口径排好）
                    sqlx::postgres::PgValueFormat::Text => {
                        let text = std::str::from_utf8(value.as_bytes()?)?;
                        Ok(Self(text.to_string()))
                    }
                    sqlx::postgres::PgValueFormat::Binary => Ok(Self($parse(value.as_bytes()?)?)),
                }
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
    "`INET` / `CIDR` 的 wire 解码（sqlx 侧要 `ipnetwork` feature，我们直接解）。"
);
pg_wire_type!(
    PgPoint,
    OID_POINT,
    point_to_text,
    "`POINT` 的 wire 解码（两个客户端库都没有）。"
);

