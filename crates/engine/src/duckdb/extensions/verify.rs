//! 重算指纹：让「磁盘上这个文件还是不是上次登记的那个」可以被随时问一次
//!
//! ## 为什么需要它
//!
//! `sha256` 只在「首次登记 / 刚安装 / 文件长度变了」时算一次，之后就没人再碰过。
//! 结果是：登记表里那串指纹只能证明「装的时候是这个文件」，不能回答「现在还是不是」。
//! 用户要的是后者——他自己也可能想不起上次放进去的是哪个版本。
//!
//! ## 口径（与第 1 步的定案一致）
//!
//! 指纹只负责让「和上次登记的不一样」可见，**不做完整性判定**：
//!
//! - 一致 → 说「一致」，不说「文件安全」；
//! - 不一致 → 说「内容与上次登记不同」，不说「被篡改」——同一台机器上换了个
//!   重新编译的同名扩展，现象与篡改完全一样，这里分不出来，也不该假装分得出来。
//!
//! 所以这里既不阻断加载，也不改 `state`：`state` 说的是「装没装、能不能用」，
//! 指纹说的是「还是不是那个文件」，两件事不该互相污染。

use std::path::Path;

use crate::persistence::extension_store::{
    ExtensionOperation, find_extension, record_operation, upsert_extension,
};

use super::inventory::inventory;

/// 一次校验的结论
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyVerdict {
    /// 与登记时的指纹一致
    Matched,
    /// 与登记时的指纹不同
    Changed,
    /// 原先没有指纹（老数据 / 上次没算成），这次补上
    Seeded,
}

/// 一次校验的结果
#[derive(Debug, Clone)]
pub struct VerifyOutcome {
    pub name: String,
    pub verdict: VerifyVerdict,
    /// 登记表里原来的指纹（`Seeded` 时为 `None`）
    pub registered_sha256: Option<String>,
    /// 现在磁盘文件的指纹
    pub current_sha256: String,
    pub size_bytes: i64,
    /// 给界面直接显示的一句人话
    pub message: String,
}

/// 重算一个已登记扩展的指纹，并与登记表里的值比对
///
/// 只接受「已登记且文件在磁盘上」的扩展：没登记的东西该走认领或移出，
/// 文件不在的没什么可算的。
pub fn verify(name: &str) -> Result<VerifyOutcome, String> {
    let snapshot = inventory()?;
    let entry = snapshot
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| format!("清单里没有 {name}，请刷新后重试"))?;
    if !entry.registered {
        return Err(format!(
            "{name} 还没登记过，没有可比对的对象（先在「引擎扩展」里认领它）"
        ));
    }
    if !entry.on_disk {
        return Err(format!("{name} 的文件不在磁盘上，没有可校验的内容"));
    }
    let path = entry
        .install_path
        .as_deref()
        .ok_or_else(|| format!("拿不到 {name} 的文件路径，无法校验"))?;

    let (current_sha256, size_bytes) = super::sha256_of(Path::new(path))
        .map_err(|error| format!("读 {} 失败：{error}", path))?;

    // 先取旧行：`upsert_extension` 是全列覆盖，不先读出其余列会被抹成空
    let mut row = with_global_conn!(|conn| {
        find_extension(conn, name).map_err(|error| error.to_string())
    })?
    .ok_or_else(|| format!("登记表里没有 {name} 的行（清单与表不一致，请刷新）"))?;

    let registered_sha256 = row.sha256.clone();
    let verdict = match registered_sha256.as_deref() {
        None => VerifyVerdict::Seeded,
        Some(old) if old == current_sha256 => VerifyVerdict::Matched,
        Some(_) => VerifyVerdict::Changed,
    };

    // 按现状更新指纹：校验的意义就是「让登记表回到与磁盘一致」，否则下一次还是拿旧值比
    row.sha256 = Some(current_sha256.clone());
    row.size_bytes = Some(size_bytes as i64);

    let mut op = match verdict {
        VerifyVerdict::Changed => ExtensionOperation::fail(
            "ui",
            "verify",
            format!(
                "文件内容与上次登记不同（登记 {} → 现在 {}）",
                short(registered_sha256.as_deref().unwrap_or_default()),
                short(&current_sha256)
            ),
        ),
        VerifyVerdict::Seeded | VerifyVerdict::Matched => {
            ExtensionOperation::ok("ui", "verify")
        }
    };
    op = op.extension(name).build(&entry.kernel_version, &entry.platform);
    op.before_json = registered_sha256
        .as_ref()
        .map(|sha| format!("{{\"sha256\":\"{sha}\"}}"));
    op.after_json = Some(format!("{{\"sha256\":\"{current_sha256}\"}}"));

    with_global_conn!(|conn| {
        upsert_extension(conn, &row).map_err(|error| error.to_string())?;
        record_operation(conn, &op)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })?;

    let message = match verdict {
        VerifyVerdict::Matched => format!(
            "{name}：文件内容与登记一致（{}）",
            short(&current_sha256)
        ),
        VerifyVerdict::Changed => format!(
            "{name}：文件内容与上次登记不同——登记的是 {}，现在是 {}。已按现状更新指纹，\
             这次不同已记进留痕（不代表文件有问题，同名重新编译也会这样）",
            short(registered_sha256.as_deref().unwrap_or_default()),
            short(&current_sha256)
        ),
        VerifyVerdict::Seeded => format!(
            "{name}：原先没有指纹，已补上（{}）",
            short(&current_sha256)
        ),
    };

    Ok(VerifyOutcome {
        name: name.to_string(),
        verdict,
        registered_sha256,
        current_sha256,
        size_bytes: size_bytes as i64,
        message,
    })
}

/// 指纹前 8 位（界面上看见「变了没」够用，完整值留给留痕表）
fn short(sha256: &str) -> String {
    sha256.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    // 安全模式：不通配导入
    use super::{VerifyVerdict, short};

    #[test]
    fn short_keeps_the_first_eight() {
        assert_eq!(short("abcdef0123456789"), "abcdef01");
        assert_eq!(short(""), "", "空值不能 panic，界面上老数据就是空的");
    }

    /// 三种结论必须各自独立：混淆任何一种都会让用户看错「变了没」
    #[test]
    fn verdicts_are_distinct() {
        assert_ne!(VerifyVerdict::Matched, VerifyVerdict::Changed);
        assert_ne!(VerifyVerdict::Matched, VerifyVerdict::Seeded);
        assert_ne!(VerifyVerdict::Changed, VerifyVerdict::Seeded);
    }
}
