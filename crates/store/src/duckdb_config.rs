//! DuckDB 连接默认参数配置
//!
//! 自 engine `duckdb::manager::DuckDBManager::configure_connection` 下沉
//! （2026-10-29，engine 瘦身第三刀前置）：全局 / 项目 / 读取池 / 加速会话 /
//! duckdb 驱动所有长期存活的 DuckDB 连接都要过这里，而 rds-driver（native
//! duckdb 实现）不能依赖 engine——放 store（engine 与 driver 两侧共用）。
//!
//! engine `DuckDBManager` 保留同名 `pub(crate)` 委托，engine 内部调用零改动。

use duckdb::Connection;

use shared::error::{CommonError, CoreError};

/// 内存库的内存上限默认值（可用 [`ENV_MEMORY_LIMIT`] 覆盖）。
///
/// 为什么要有这道闸：内存库是**进程级单例**，查询结果表与洞察中间表都记在它头上，
/// 而 DuckDB 自己的默认值是「物理内存的 80%」——桌面应用把机器吃光不是可接受的失败方式。
/// 上限一到，DuckDB 会**溢写到 `temp_directory`**（而不是直接报错），所以换来的是「慢一点」。
const DEFAULT_MEMORY_LIMIT: &str = "2GB";

/// 覆盖内存上限的环境变量（例：`RDS_DUCKDB_MEMORY_LIMIT=4GB`）。非法值被忽略并告警。
const ENV_MEMORY_LIMIT: &str = "RDS_DUCKDB_MEMORY_LIMIT";

/// 溢写目录的总量上限默认值（可用 [`ENV_TEMP_SIZE_LIMIT`] 覆盖）。
///
/// DuckDB 默认 100GB；数据根的 `tmp/` 是我们自己会清理的目录，不该让它长到那种量级。
const DEFAULT_TEMP_SIZE_LIMIT: &str = "10GB";

/// 覆盖溢写目录上限的环境变量。
const ENV_TEMP_SIZE_LIMIT: &str = "RDS_DUCKDB_MAX_TEMP_SIZE";

/// 配置 DuckDB 连接的默认参数。
///
/// **所有长期存活的 DuckDB 连接都要过这里**（全局 / 项目 / 读取池 / 加速会话 / duckdb 驱动）。
/// 绕过它就等于绕过资源闸与扩展策略——审计时抓到过两次：`accel` 自己拼扩展目录
/// （漏了内存闸与溢写口），duckdb 驱动干脆什么都不设（用户 SQL 里出现 `read_parquet`
/// 这类函数时，扩展会被**静默下到 `~/.duckdb`**，写用户 C 盘）。
///
/// 五件事：
/// 1. **扩展目录**：一律 `<RDS_HOME>/extensions`（应用目录内，可离线预置）；
/// 2. **不静默联网**：`autoinstall_known_extensions = false`——SQL 里出现未安装的扩展
///    函数时**报错**而不是偷偷下载（企业内网里那是“莫名卡住”）；`INSTALL` 一律由我们
///    显式做（见 engine `duckdb/extensions.rs` 与联邦的扩展门控）；
/// 3. **已装的自动加载**：`autoload_known_extensions = true`——它只加载**本地已装**的
///    扩展，无网络副作用，保住日常体验（关掉它会让“之前能用”的查询突然报未加载）；
///    同时显式允许社区扩展（联邦要用 `mssql` / `oracle_scanner`，它们都是社区扩展）；
/// 4. **内存闸**：`memory_limit`（DuckDB 默认是物理内存的 80%——桌面应用不该把机器吃光）；
/// 5. **溢写口**：`temp_directory` 钉到数据根的 `tmp/`（到顶时溢写到这里，而不是系统
///    临时目录；那是用户清理不到的角落），并给溢写总量上也一把限。
///
/// 两个大小值都可用环境变量覆盖（见 [`ENV_MEMORY_LIMIT`] / [`ENV_TEMP_SIZE_LIMIT`]）；
/// **拼进 SQL 前一律过 [`parse_size_setting`] 白名单**，非法值回退默认并告警——
/// 配错一个环境变量不该让程序起不来。
///
/// # 参数
/// - `conn`: 需要配置的 DuckDB 连接
///
/// # 返回
/// - `Ok(())`: 配置成功
/// - `Err(CoreError)`: 配置失败
pub fn configure_connection(conn: &Connection) -> Result<(), CoreError> {
    // 扩展目录：尽量先建出来（`INSTALL` 与离线预置都需要它）
    let extensions_dir = paths::extensions_dir();
    if let Err(e) = std::fs::create_dir_all(&extensions_dir) {
        tracing::warn!(
            "[duckdb] 扩展目录 {} 建不出来（{e}）：安装扩展时可能失败",
            extensions_dir.display()
        );
    }

    // 逐条收集再拼成一条 batch（`execute_batch` 仍按分号拆语句，换行不算分隔符）
    //
    // 注意 **不设 `allow_community_extensions`**：实测它在库跑起来之后不能改
    // （`Cannot change allow_community_extensions setting while database is running`），
    // 而它的默认值就是 `true`（联邦要用的 `mssql` / `oracle_scanner` 都是社区扩展）——
    // 要改它必须在建库前用 `DBConfig`，我们没这个需求，就不把它写进这里。
    let mut settings = vec![
        format!("SET extension_directory = '{}'", extensions_dir.display()),
        // 不静默联网（未装的扩展直接报错；装上是我们的事）
        "SET autoinstall_known_extensions = false".to_string(),
        // 已装的自动加载（无网络副作用）
        "SET autoload_known_extensions = true".to_string(),
    ];

    // Secret 目录：凭据由 `workbench::services::secret_integration` 注册到
    // `{system}/secrets`，会话必须指向同一个目录才看得到它们。
    // **不设的后果**：挂网络源时连接串里的口令是脱敏的（`user:******@`），
    // DuckDB 拿 `******` 去认证 → 真机上表现为“加速 / 联邦挂不上源库”，
    // 而错误信息只说认证失败，看不出是目录没对上。
    match crate::persistence::get_secrets_dir() {
        Ok(dir) => settings.push(format!("SET secret_directory = '{}'", dir.display())),
        Err(e) => tracing::warn!(
            "[duckdb] Secret 目录不可用（{e}）：本地加速 / 联邦的网络源可能认证失败"
        ),
    }

    // 临时目录：建不出来就跳过溢写设置（退回 DuckDB 默认），不挡启动
    let temp_dir = paths::temp_dir();
    match std::fs::create_dir_all(&temp_dir) {
        Ok(()) => {
            settings.push(format!("SET temp_directory = '{}'", temp_dir.display()));
            settings.push(format!(
                "SET max_temp_directory_size = '{}'",
                size_setting(
                    std::env::var_os(ENV_TEMP_SIZE_LIMIT),
                    DEFAULT_TEMP_SIZE_LIMIT,
                    ENV_TEMP_SIZE_LIMIT,
                )
            ));
        }
        Err(e) => tracing::warn!(
            "[duckdb] 溢写目录 {} 不可用（{e}）：将使用 DuckDB 默认临时目录",
            temp_dir.display()
        ),
    }

    settings.push(format!(
        "SET memory_limit = '{}'",
        size_setting(
            std::env::var_os(ENV_MEMORY_LIMIT),
            DEFAULT_MEMORY_LIMIT,
            ENV_MEMORY_LIMIT,
        )
    ));

    let sql = format!("{};", settings.join(";\n"));
    conn.execute_batch(&sql).map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "配置 DuckDB 连接失败: {}（尝试的设置：{}）",
            e,
            settings.join("; ")
        )))
    })?;

    // 只在 debug 级：本函数对每个连接都会跑（非 Windows 还有读取连接池）
    tracing::debug!("[duckdb] 连接配置已应用：{}", settings.join("; "));
    Ok(())
}

/// 从环境变量解析大小设置，非法时回退默认并告警。
///
/// - `raw`: 环境变量原值（未设置时为 `None`）
/// - `default`: 回退值
/// - `env_name`: 环境变量名（仅用于告警文案）
fn size_setting(raw: Option<std::ffi::OsString>, default: &str, env_name: &str) -> String {
    let Some(raw) = raw else {
        return default.to_string();
    };
    let raw = raw.to_string_lossy();
    match parse_size_setting(&raw) {
        Some(ok) => ok,
        None => {
            tracing::warn!(
                "[duckdb] 环境变量 {env_name}=\"{}\" 不是合法大小，按默认值 {default} 处理",
                raw.trim()
            );
            default.to_string()
        }
    }
}

/// 校验并规整大小字符串（`2GB` / `512 mb` / `1073741824`）。
///
/// 白名单故意窄：这些值会被**拼进 SQL**（DuckDB 的 `SET` 不接受绑定参数），
/// 且窄白名单同时挡住了单位写错（`2 GBB`）与手滑。不合法返回 `None`。
fn parse_size_setting(raw: &str) -> Option<String> {
    let s = raw.trim();
    // 数字部分：数字与至多一个小数点；其余算单位（允许中间有空格）
    let split = s
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let unit = unit.trim();

    let digits_ok = !num.is_empty()
        && num.chars().any(|c| c.is_ascii_digit())
        && num.chars().filter(|c| *c == '.').count() <= 1;
    let unit_ok = matches!(
        unit.to_ascii_uppercase().as_str(),
        "" | "B" | "KB" | "MB" | "GB" | "TB"
    );

    (digits_ok && unit_ok).then(|| format!("{num}{}", unit.to_ascii_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_size_setting_whitelist() {
        // 合法：数字 + 可选单位（大小写 / 空格不敏感）
        assert_eq!(parse_size_setting("2GB").as_deref(), Some("2GB"));
        assert_eq!(parse_size_setting(" 512 mb ").as_deref(), Some("512MB"));
        assert_eq!(parse_size_setting("1073741824").as_deref(), Some("1073741824"));
        assert_eq!(parse_size_setting("1.5gb").as_deref(), Some("1.5GB"));

        // 不合法：空 / 纯单位 / 负号 / 未知单位 / 带 SQL 尾巴——后者是白名单真正要挡的
        assert_eq!(parse_size_setting(""), None);
        assert_eq!(parse_size_setting("GB"), None);
        assert_eq!(parse_size_setting("-1"), None);
        assert_eq!(parse_size_setting("2PB"), None);
        assert_eq!(parse_size_setting("2GB'; DROP TABLE t; --"), None);
        assert_eq!(parse_size_setting("."), None);
    }

    #[test]
    fn test_size_setting_env_override_and_fallback() {
        use std::ffi::OsString;

        let default = "2GB";
        // 未设置 → 默认值
        assert_eq!(size_setting(None, default, "RDS_TEST"), default);
        // 合法覆盖 → 规整后的值
        assert_eq!(
            size_setting(Some(OsString::from("4 gb")), default, "RDS_TEST"),
            "4GB"
        );
        // 非法覆盖 → 回退默认（而不是拼进 SQL）
        assert_eq!(
            size_setting(Some(OsString::from("huge")), default, "RDS_TEST"),
            default
        );
        assert_eq!(
            size_setting(Some(OsString::from("   ")), default, "RDS_TEST"),
            default
        );
    }
}
