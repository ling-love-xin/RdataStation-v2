//! 处置「磁盘上有、从没登记过」的扩展
//!
//! ## 为什么只做这一种偏差
//!
//! `inventory` 报四种偏差，其余三种都得先知道「用户本来想干什么」才谈得上动作：
//!
//! - 登记有、磁盘没 → 要重装，但得先确认是不是被杀毒软件删了；
//! - 内核版本不一致 → 要按当前内核重装，得先确认用户是否要留旧版本；
//! - DuckDB 没认出 → 多半是目录或版本放错，得先看它到底认不认。
//!
//! 只有 `UnregisteredOnDisk` 是**明确的来路不明**：文件已经在扩展目录里，而
//! `autoload_known_extensions = true` 意味着它一旦被用到就拿到了执行权。
//! 这种状态不该只停在「看得见」，得给两条出口。
//!
//! ## 两个动作各自闭合什么
//!
//! - **认领（adopt）**：承认它就是我要的那个文件。登记出处为 `local_file`（人工认领），
//!   从此「有名、有记录、有指纹」。登记不会让它变得可信——只是让它变得可查。
//! - **移出（discard）**：删掉这个文件，扩展目录回到「只有登记过的文件」。
//!
//! ## 不做的事
//!
//! - 不在 `LOAD` 之后卸载已加载的扩展：DuckDB 没有卸载扩展的接口，
//!   进程内已经建好的连接会继续用它，删文件影响的是**下一个进程**。文案里说清楚。
//! - 不改 `allow_community_extensions` / `autoload_known_extensions`：
//!   这两条是建库前的 `DBConfig`，不属于单个扩展的处置。
//! - 不递归删空目录：文件删掉后留下的空目录无害，顺手删反而可能误伤。

use std::path::{Path, PathBuf};

use crate::persistence::extension_store::{
    DuckdbExtensionRow, ExtensionOperation, record_operation, upsert_extension,
};

use super::inventory::{ExtensionDrift, ExtensionEntry, inventory};

/// 在 `global.sqlite` 的一条同步连接上执行一段写操作
///
/// 用宏而不是函数：连接是 `rusqlite::Connection`，而 engine 没有直接依赖 rusqlite
/// （类型只经 `rds-store` 流转），函数签名写不出这个参数类型；宏展开后靠方法调用推断即可。
macro_rules! with_global_conn {
    (|$conn:ident| $body:expr) => {{
        let Some(db) = crate::migration::get_global_db_manager() else {
            return Err("global.db 未初始化（宿主启动流程未完成）".to_string());
        };
        let pool = db.sqlite_pool();
        // 这里刻意不用 `?`：宏内一旦出现 `?`，尾表达式的错误类型就被钉成 `CoreError`，
        // 而外层函数返回的是 `String`。显式 match 让错误口径只有一种。
        let guard = match pool.acquire_sync() {
            Ok(guard) => guard,
            Err(error) => return Err(error.to_string()),
        };
        let $conn = match guard.inner() {
            Ok(conn) => conn,
            Err(error) => return Err(error.to_string()),
        };
        match crate::persistence::extension_store::table_exists($conn) {
            Ok(true) => {}
            Ok(false) => return Err("duckdb_extensions 表不存在（迁移 031 未跑）".to_string()),
            Err(error) => return Err(error.to_string()),
        }
        $body
    }};
}

/// 一次处置动作
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileAction {
    /// 登记（承认这个文件）
    Adopt,
    /// 删除文件
    Discard,
}

impl ReconcileAction {
    /// 留痕用的 `action` 值（与 `ExtensionOperation` 的取值表对齐）
    pub fn operation(self) -> &'static str {
        match self {
            Self::Adopt => "import",
            Self::Discard => "remove",
        }
    }
}

/// 处置完成的结果
#[derive(Debug, Clone)]
pub struct ReconcileOutcome {
    pub name: String,
    pub action: ReconcileAction,
    /// 给界面直接显示的一句人话
    pub message: String,
}

/// 处置一个「没登记过」的扩展
///
/// 每次都重新读一遍清单再判断，不信任界面上那份快照——从点击到执行之间，
/// 文件可能已经被别人删掉或替换了。
pub fn apply(action: ReconcileAction, name: &str) -> Result<ReconcileOutcome, String> {
    let snapshot = inventory()?;
    let entry = snapshot
        .entries
        .iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| {
            format!("清单里已经没有 {name} 了，请刷新后重试（可能刚被外部改动）")
        })?;

    // 只放行这一种偏差：别的状态各有各的处置方式，误操作代价不对称
    if entry.drift.as_ref() != Some(&ExtensionDrift::UnregisteredOnDisk) {
        return Err(format!(
            "{name} 当前不是「没登记」状态，不能用这个动作处理，请先刷新看它的实际偏差"
        ));
    }
    let path = entry
        .install_path
        .as_deref()
        .ok_or_else(|| format!("拿不到 {name} 的文件路径，无法处置"))?;

    match action {
        ReconcileAction::Adopt => adopt(entry, path),
        ReconcileAction::Discard => discard(entry, path),
    }
}

fn adopt(entry: &ExtensionEntry, path: &str) -> Result<ReconcileOutcome, String> {
    let file = PathBuf::from(path);
    // 认领要算指纹：这是「这批文件」离开「来路不明」的唯一凭据
    let (sha256, size_bytes) = super::sha256_of(&file)
        .map_err(|error| format!("读 {} 失败：{error}", file.display()))?;

    let mut row = DuckdbExtensionRow::installed(
        &entry.name,
        &entry.kernel_version,
        &entry.platform,
        "local_file",
    );
    row = row
        .with_path(path.to_string())
        .with_sha256(sha256, size_bytes as i64);

    let mut op = ExtensionOperation::ok("ui", ReconcileAction::Adopt.operation())
        .extension(&entry.name)
        .build(&entry.kernel_version, &entry.platform);
    op.reason = Some("人工认领扩展目录里的文件（原先没有登记）".to_string());

    with_global_conn!(|conn| {
        upsert_extension(conn, &row).map_err(|error| error.to_string())?;
        record_operation(conn, &op)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })?;

    Ok(ReconcileOutcome {
        name: entry.name.clone(),
        action: ReconcileAction::Adopt,
        message: format!(
            "已登记 {}（来源记为「本地文件」，指纹已记录）",
            entry.name
        ),
    })
}

fn discard(entry: &ExtensionEntry, path: &str) -> Result<ReconcileOutcome, String> {
    let file = ensure_within_extensions_dir(Path::new(path))?;
    std::fs::remove_file(&file).map_err(|error| {
        format!(
            "删除 {} 失败：{error}（文件可能正被宿主进程占用，关闭宿主后重试）",
            file.display()
        )
    })?;

    let mut op = ExtensionOperation::ok("ui", ReconcileAction::Discard.operation())
        .extension(&entry.name)
        .build(&entry.kernel_version, &entry.platform);
    op.reason = Some("移出来路不明的扩展文件".to_string());

    // 留痕失败不该把「文件已经删了」说成没删——先删文件、再记，记不上就报出来
    with_global_conn!(|conn| record_operation(conn, &op).map(|_| ()))
        .map_err(|error| format!("文件已删除，但留痕写入失败：{}", error.to_string()))?;

    Ok(ReconcileOutcome {
        name: entry.name.clone(),
        action: ReconcileAction::Discard,
        message: format!(
            "已移出 {}。进程内已经加载它的连接会继续用，下一个连接起才不再生效",
            entry.name
        ),
    })
}

/// 删除前的边界校验：必须是扩展目录下、后缀正确的真实文件
///
/// 两条都必要：路径来自数据库，可能被改过；不校验就等于「按数据库里的字符串删任意文件」。
fn ensure_within_extensions_dir(path: &Path) -> Result<PathBuf, String> {
    if path.extension().and_then(|ext| ext.to_str()) != Some("duckdb_extension") {
        return Err(format!("拒绝删除非扩展文件：{}", path.display()));
    }
    let root = paths::extensions_dir();
    let root = root
        .canonicalize()
        .map_err(|error| format!("扩展目录不存在或不可读：{error}"))?;
    let actual = path
        .canonicalize()
        .map_err(|error| format!("文件不存在或不可读：{error}"))?;
    if !actual.starts_with(&root) {
        return Err(format!(
            "拒绝删除扩展目录之外的文件：{}",
            actual.display()
        ));
    }
    Ok(actual)
}

#[cfg(test)]
mod tests {
    // 安全模式：不通配导入
    use super::ensure_within_extensions_dir;
    use std::path::Path;

    /// 目录外的东西一律拒绝——这条错了就是「按数据库字符串删任意文件」
    #[test]
    fn refuses_files_outside_the_extensions_dir() {
        let outside = tempdir("rds-ext-outside");
        let file = outside.join("evil.duckdb_extension");
        std::fs::write(&file, b"x").expect("写");
        let error = ensure_within_extensions_dir(&file).expect_err("必须拒绝");
        assert!(
            error.contains("扩展目录之外") || error.contains("扩展目录不存在"),
            "拒绝理由要能看懂，实际：{error}"
        );
        std::fs::remove_dir_all(&outside).ok();
    }

    /// 后缀不对的也不删：扩展目录里可能躺着别人放的别的东西
    #[test]
    fn refuses_non_extension_suffix() {
        let error = ensure_within_extensions_dir(Path::new("C:/some/where/notes.txt"))
            .expect_err("必须拒绝");
        assert!(error.contains("非扩展文件"), "实际：{error}");
    }

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建临时目录");
        dir
    }
}
