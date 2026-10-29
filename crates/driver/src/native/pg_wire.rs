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
//! 依赖；`point` 完全没有），而字节格式是同一套 —— 解析写在这里一份，两个驱动共用，
//! 免得「同一个库两个驱动、两套格式化」。
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

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// `NUMERIC` 的 OID（`pg_type.oid`；两个客户端库用同一套）。
const OID_NUMERIC: u32 = 1700;
/// `INTERVAL` 的 OID。
const OID_INTERVAL: u32 = 1186;
/// `INET` 的 OID。
const OID_INET: u32 = 869;
/// `POINT` 的 OID。
const OID_POINT: u32 = 600;

/// `NUMERIC`：base-10000 的数字串 → 十进制文本（按 `dscale` 定小数位）。
///
/// 布局：`ndigits i16, weight i16, sign u16, dscale i16, digits[i16; ndigits]`。
/// `sign`：`0x0000` 正 / `0x4000` 负 / `0xC000` NaN / `0xD000` +Inf / `0xF000` -Inf。
/// `weight` 是**最高有效组**的十进制权（组 = 4 位）。
pub fn numeric_to_text(bytes: &[u8]) -> Result<String, String> {
    let need = |n: usize| -> Result<(), String> {
        (bytes.len() >= n)
            .then_some(())
            .ok_or_else(|| format!("NUMERIC 的字节数不对：{} < {n}", bytes.len()))
    };
    need(8)?;
    let ndigits = i16::from_be_bytes([bytes[0], bytes[1]]);
    let weight = i16::from_be_bytes([bytes[2], bytes[3]]);
    let sign = u16::from_be_bytes([bytes[4], bytes[5]]);
    let dscale = i16::from_be_bytes([bytes[6], bytes[7]]);
    if ndigits < 0 || dscale < 0 {
        return Err(format!(
            "NUMERIC 的头部不合理：ndigits={ndigits} dscale={dscale}"
        ));
    }
    need(8 + ndigits as usize * 2)?;

    match sign {
        0xC000 => return Ok("NaN".to_string()),
        0xD000 => return Ok("Infinity".to_string()),
        0xF000 => return Ok("-Infinity".to_string()),
        _ => {}
    }

    // digits[i] 的位权是 10000^(weight - i)（数组最高位在前）
    let digits: Vec<i32> = (0..ndigits as usize)
        .map(|i| {
            let at = 8 + i * 2;
            i16::from_be_bytes([bytes[at], bytes[at + 1]]) as i32
        })
        .collect();
    for d in &digits {
        if !(0..10_000).contains(d) {
            return Err(format!("NUMERIC 的组超出 0..10000：{d}"));
        }
    }

    // 整数部分：位权 >= 0 的组（最高组不补零，其余补足 4 位）；weight < 0 说明整数值是 0
    let mut text = String::new();
    if sign == 0x4000 {
        text.push('-');
    }
    if weight >= 0 {
        for i in 0..=weight as usize {
            let group = digits.get(i).copied().unwrap_or(0);
            if i == 0 {
                text.push_str(&group.to_string());
            } else {
                text.push_str(&format!("{group:04}"));
            }
        }
    } else {
        text.push('0');
    }

    // 小数部分：按 dscale 出位数。
    //
    // 第 (k+1) 个**小数**组的指数是 -(k+1)，在数组里的下标是 `weight + k + 1`；下标为负
    // 说明这一组前面还隔着整组零（0.000001 的最高组在 -2 上），补 `0000`；最后一组只取
    // `dscale` 需要的几位（1234.56 的 dscale=2，最后一组 5600 只取 "56"）。
    let dscale = dscale as usize;
    if dscale > 0 {
        text.push('.');
        let groups = dscale.div_ceil(4);
        for k in 0..groups {
            let idx = weight as i32 + k as i32 + 1;
            let group = if idx < 0 {
                0
            } else {
                digits.get(idx as usize).copied().unwrap_or(0)
            };
            let rendered = format!("{group:04}");
            let take = if k + 1 == groups { dscale - k * 4 } else { 4 };
            text.push_str(&rendered[..take]);
        }
    }
    Ok(text)
}

/// `INTERVAL`：`microseconds i64, days i32, months i32` → PG 风格文本。
pub fn interval_to_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < 16 {
        return Err(format!("INTERVAL 的字节数不对：{} < 16", bytes.len()));
    }
    let micros = i64::from_be_bytes(bytes[0..8].try_into().expect("8 字节"));
    let days = i32::from_be_bytes(bytes[8..12].try_into().expect("4 字节"));
    let months = i32::from_be_bytes(bytes[12..16].try_into().expect("4 字节"));
    Ok(interval_parts_to_text(months, days, micros))
}

/// 区间文本：`months` / `days` / `microseconds` 三个分量 → PG 风格。
///
/// 抽出来是因为 **sqlx 侧有现成的 `PgInterval`**（也是这三个字段），两条路共用一份格式化，
/// 免得同一个库的两个驱动打出两种写法。
pub fn interval_parts_to_text(months: i32, days: i32, micros: i64) -> String {
    let mut parts: Vec<String> = Vec::new();
    let years = months / 12;
    let mons = months % 12;
    if years != 0 {
        parts.push(format!(
            "{years} year{}",
            if years.abs() == 1 { "" } else { "s" }
        ));
    }
    if mons != 0 {
        parts.push(format!("{mons} mon{}", if mons.abs() == 1 { "" } else { "s" }));
    }
    if days != 0 {
        parts.push(format!("{days} day{}", if days.abs() == 1 { "" } else { "s" }));
    }

    let negative = micros < 0;
    let abs = micros.unsigned_abs();
    let (secs, micro) = (abs / 1_000_000, abs % 1_000_000);
    let (hours, minutes, seconds) = (secs / 3600, (secs / 60) % 60, secs % 60);
    let mut time = format!(
        "{}{hours:02}:{minutes:02}:{seconds:02}",
        if negative { "-" } else { "" }
    );
    if micro > 0 {
        let frac = format!("{micro:06}");
        time.push('.');
        time.push_str(frac.trim_end_matches('0'));
    }
    if micros != 0 || parts.is_empty() {
        parts.push(time);
    }
    parts.join(" ")
}

/// PG 的数组字面量：`{a,b}`；元素含分隔符 / 引号 / 反斜杠时按 PG 的规矩加双引号并转义。
///
/// 为什么不是简单 `format!("{:?}")`：`text[]` 的元素可以含逗号，不加引号就再也分不回来。
pub fn array_literal(items: impl IntoIterator<Item = String>) -> String {
    let rendered: Vec<String> = items
        .into_iter()
        .map(|item| {
            let needs_quotes = item.is_empty()
                || item
                    .chars()
                    .any(|c| matches!(c, ',' | '{' | '}' | '"' | '\\') || c.is_whitespace())
                || item.eq_ignore_ascii_case("null");
            if needs_quotes {
                format!("\"{}\"", item.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                item
            }
        })
        .collect();
    format!("{{{}}}", rendered.join(","))
}

/// `INET` / `CIDR`：`family u8, netmask u8, is_cidr u8, len u8, addr[len]` → `addr/netmask`。
pub fn inet_to_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < 4 {
        return Err(format!("INET 的字节数不对：{} < 4", bytes.len()));
    }
    let (family, netmask, is_cidr, len) = (bytes[0], bytes[1], bytes[2] != 0, bytes[3] as usize);
    if bytes.len() < 4 + len {
        return Err(format!(
            "INET 的地址长度与字节数不符：{len} + 4 > {}",
            bytes.len()
        ));
    }
    let addr: IpAddr = match family {
        2 => {
            if len != 4 {
                return Err(format!("IPv4 的地址长度应为 4，实际 {len}"));
            }
            IpAddr::V4(Ipv4Addr::from(
                <[u8; 4]>::try_from(&bytes[4..8]).expect("4 字节"),
            ))
        }
        3 => {
            if len != 16 {
                return Err(format!("IPv6 的地址长度应为 16，实际 {len}"));
            }
            IpAddr::V6(Ipv6Addr::from(
                <[u8; 16]>::try_from(&bytes[4..20]).expect("16 字节"),
            ))
        }
        other => return Err(format!("未知的地址族：{other}")),
    };
    let full = match addr {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    };
    Ok(if is_cidr || netmask != full {
        format!("{addr}/{netmask}")
    } else {
        addr.to_string()
    })
}

/// `POINT`：`x f64, y f64` → `(x,y)`。
pub fn point_to_text(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < 16 {
        return Err(format!("POINT 的字节数不对：{} < 16", bytes.len()));
    }
    let x = f64::from_be_bytes(bytes[0..8].try_into().expect("8 字节"));
    let y = f64::from_be_bytes(bytes[8..16].try_into().expect("8 字节"));
    Ok(format!("({x},{y})"))
}

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

#[cfg(test)]
mod tests {
    use super::*;

    // 下面这些字节按协议文档**手工拼**（大端），用来钉住解析本身；
    // 「真库真值」那一半在 `crates/engine/tests/type_decode_fidelity.rs`（需要端点）。

    /// `ndigits, weight, sign, dscale, digits…`
    fn numeric_bytes(weight: i16, sign: u16, dscale: i16, digits: &[i16]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(digits.len() as i16).to_be_bytes());
        out.extend_from_slice(&weight.to_be_bytes());
        out.extend_from_slice(&sign.to_be_bytes());
        out.extend_from_slice(&dscale.to_be_bytes());
        for d in digits {
            out.extend_from_slice(&d.to_be_bytes());
        }
        out
    }

    #[test]
    fn numeric_decodes_base_10000_groups() {
        // 1234.56 → 组 [1234, 5600]，weight 0，dscale 2
        assert_eq!(
            numeric_to_text(&numeric_bytes(0, 0x0000, 2, &[1234, 5600])).unwrap(),
            "1234.56"
        );
        // -0.001 → 组 [10]，weight -1，dscale 3
        assert_eq!(
            numeric_to_text(&numeric_bytes(-1, 0x4000, 3, &[10])).unwrap(),
            "-0.001"
        );
        // 0 → 无组，dscale 2
        assert_eq!(
            numeric_to_text(&numeric_bytes(0, 0x0000, 2, &[])).unwrap(),
            "0.00"
        );
        // 整数：123456789012345678 → 组 [12, 3456, 7890, 1234, 5678]（最高组不补零）
        assert_eq!(
            numeric_to_text(&numeric_bytes(4, 0x0000, 0, &[12, 3456, 7890, 1234, 5678])).unwrap(),
            "123456789012345678"
        );
        // 大数带小数：100000.0001 → 组 [10, 0, 1]，weight 1，dscale 4
        assert_eq!(
            numeric_to_text(&numeric_bytes(1, 0x0000, 4, &[10, 0, 1])).unwrap(),
            "100000.0001"
        );
        // 中段的 0 要补成 4 位
        assert_eq!(
            numeric_to_text(&numeric_bytes(2, 0x0000, 0, &[1, 0, 0])).unwrap(),
            "100000000"
        );
        // 小数前导零要整组补齐：1e-6（最高组在 -2 上，前面空了一整组）
        assert_eq!(
            numeric_to_text(&numeric_bytes(-2, 0x0000, 6, &[100])).unwrap(),
            "0.000001"
        );
        // 0.0001（正好落在 -1 组的边界上）
        assert_eq!(
            numeric_to_text(&numeric_bytes(-1, 0x0000, 4, &[1])).unwrap(),
            "0.0001"
        );
        // 特殊值
        assert_eq!(
            numeric_to_text(&numeric_bytes(0, 0xC000, 0, &[])).unwrap(),
            "NaN"
        );
        assert_eq!(
            numeric_to_text(&numeric_bytes(0, 0xD000, 0, &[])).unwrap(),
            "Infinity"
        );
        assert_eq!(
            numeric_to_text(&numeric_bytes(0, 0xF000, 0, &[])).unwrap(),
            "-Infinity"
        );
        // 头不完整 / 组越界 → 报错（不 panic、不瞎猜）
        assert!(numeric_to_text(&[0, 1, 0, 0]).is_err());
        assert!(numeric_to_text(&numeric_bytes(0, 0, 0, &[10_000])).is_err());
    }

    #[test]
    fn interval_decodes_its_three_fields() {
        let bytes = |micros: i64, days: i32, months: i32| -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&micros.to_be_bytes());
            out.extend_from_slice(&days.to_be_bytes());
            out.extend_from_slice(&months.to_be_bytes());
            out
        };
        // 1 day 02:03:04
        assert_eq!(
            interval_to_text(&bytes(
                2 * 3600 * 1_000_000 + 3 * 60 * 1_000_000 + 4 * 1_000_000,
                1,
                0
            ))
            .unwrap(),
            "1 day 02:03:04"
        );
        // 1 mon 2 days（没有时间部分）
        assert_eq!(interval_to_text(&bytes(0, 2, 1)).unwrap(), "1 mon 2 days");
        // 1 year 3 mons → 15 个月
        assert_eq!(interval_to_text(&bytes(0, 0, 15)).unwrap(), "1 year 3 mons");
        // 负的时间部分（-1 秒）
        assert_eq!(
            interval_to_text(&bytes(-1_000_000, 0, 0)).unwrap(),
            "-00:00:01"
        );
        // 小数秒：1.5 秒
        assert_eq!(
            interval_to_text(&bytes(1_500_000, 0, 0)).unwrap(),
            "00:00:01.5"
        );
        // 零区间
        assert_eq!(interval_to_text(&bytes(0, 0, 0)).unwrap(), "00:00:00");
        // 超过 24 小时的部分按小时打（PG 不把它归一成天）
        assert_eq!(
            interval_to_text(&bytes(25 * 3600 * 1_000_000, 0, 0)).unwrap(),
            "25:00:00"
        );
        assert!(interval_to_text(&[0; 8]).is_err());
    }

    #[test]
    fn interval_parts_and_array_literal_are_shared() {
        // sqlx 侧（PgInterval）与 wire 侧共用同一份格式化
        assert_eq!(interval_parts_to_text(1, 2, 7_380_000_000), "1 mon 2 days 02:03:00");
        assert_eq!(interval_parts_to_text(0, 0, 0), "00:00:00");

        assert_eq!(array_literal(["1".to_string(), "2".to_string()]), "{1,2}");
        // 含逗号的文本元素要加引号（不然分不回来）
        assert_eq!(
            array_literal(["a,b".to_string(), "c".to_string()]),
            "{\"a,b\",c}"
        );
        // 空串按 PG 的规矩也要引号
        assert_eq!(array_literal([String::new()]), "{\"\"}");
    }

    #[test]
    fn inet_decodes_ipv4_and_ipv6() {
        // 192.168.3.138/32 → 只打地址（netmask 满位且不是 cidr）
        let mut v4 = vec![2, 32, 0, 4];
        v4.extend_from_slice(&[192, 168, 3, 138]);
        assert_eq!(inet_to_text(&v4).unwrap(), "192.168.3.138");

        // 192.168.3.0/24 → 带网段
        let mut v4_24 = vec![2, 24, 0, 4];
        v4_24.extend_from_slice(&[192, 168, 3, 0]);
        assert_eq!(inet_to_text(&v4_24).unwrap(), "192.168.3.0/24");

        // cidr 即便满位也带网段
        let mut v4_cidr = vec![2, 32, 1, 4];
        v4_cidr.extend_from_slice(&[10, 0, 0, 1]);
        assert_eq!(inet_to_text(&v4_cidr).unwrap(), "10.0.0.1/32");

        // IPv6 回环
        let mut v6 = vec![3, 128, 0, 16];
        v6.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        assert_eq!(inet_to_text(&v6).unwrap(), "::1");

        // 长度与字节数不符 → 报错
        assert!(inet_to_text(&[2, 32, 0, 4, 1, 2]).is_err());
        // 未知地址族 → 报错
        assert!(inet_to_text(&[9, 32, 0, 4, 1, 2, 3, 4]).is_err());
    }

    #[test]
    fn point_decodes_two_doubles() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1.5f64.to_be_bytes());
        bytes.extend_from_slice(&(-2.25f64).to_be_bytes());
        assert_eq!(point_to_text(&bytes).unwrap(), "(1.5,-2.25)");

        let mut ints = Vec::new();
        ints.extend_from_slice(&1.0f64.to_be_bytes());
        ints.extend_from_slice(&2.0f64.to_be_bytes());
        assert_eq!(point_to_text(&ints).unwrap(), "(1,2)");

        assert!(point_to_text(&[0; 8]).is_err());
    }
}
