//! 项目规则信任（Q1 ③ / D53）：把「项目自带的规则要不要在本机执行」变成**用户的一次决定**。
//!
//! # 为什么需要
//!
//! 项目层规则落在 `{项目}/.RSmeta/insight-rules/`，**跟着仓库走**。克隆一个不信任的仓库
//! 再打开项目，就等于把它的 SQL 拿到本机执行（K1）。解析期静态门（D52）只挡「不是只读
//! 查询」的写法，挡不住一条**合法但恶意**的查询——真正的边界只能是人的决定。
//!
//! # 为什么存在全局库
//!
//! 记录落在 `<RDS_HOME>/data/system/global.db`（本表 `insight_rule_trust`），不是项目库：
//! 项目目录在被信任之前就是**不可信输入**，信任标记如果写在项目里，攻击者可以连标记
//! 一起提交（自我授权）。全局库在用户的数据根下，项目文件碰不到。
//!
//! # 语义（fail closed）
//!
//! | 记录 | 装配项目层？ | 界面 |
//! | --- | --- | --- |
//! | 无（[`RuleTrust::Undecided`]） | ❌ | 首次打开规则管理时问一次 |
//! | `trusted` | ✅ | —— |
//! | `declined` | ❌ | 提示「你选择了不加载」+ 可改主意 |
//!
//! `declined` 也落库是有意的：把「不加载」记成一次决定，才不会每次打开项目都追问。
//!
//! # 已知取舍（2026-11-02 更新：内容指纹已落地）
//!
//! 信任原本只绑定**项目路径**——`git pull` 带进新规则不会重新确认。现在决定同时
//! 绑定**决定时的规则集内容指纹**（迁移 027）：指纹变了（无论 git pull 还是用户
//! 自己改）→ 回到未确认重新走一次人的决定。代价是规则开发者改规则也要重新确认，
//! 顺手的替代路径是把规则放**全局层**（随本机走，无信任门）。旧记录（无指纹列值）
//! 按纯路径语义放行，升级零打扰，下一次决定起绑定内容。

use std::path::Path;

use engine::persistence::global_db::GlobalSqlitePool;
use shared::error::{CommonError, CoreError};

/// 项目规则的信任状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleTrust {
    /// 已信任：项目层规则参与装配。
    Trusted,
    /// 用户明确选择了不加载（记录在案，不再追问）。
    Declined,
    /// 还没有决定（默认；也包含查库失败——fail closed）。
    Undecided,
}

impl RuleTrust {
    /// 落库取值；`Undecided` 没有取值（它=「没有记录」）。
    pub fn as_db_value(self) -> Option<&'static str> {
        match self {
            RuleTrust::Trusted => Some("trusted"),
            RuleTrust::Declined => Some("declined"),
            RuleTrust::Undecided => None,
        }
    }

    /// 从库里取值还原；不认识的值按 `Undecided`（宁可再问一次，也不要默默加载）。
    pub fn from_db_value(raw: &str) -> Self {
        match raw {
            "trusted" => RuleTrust::Trusted,
            "declined" => RuleTrust::Declined,
            _ => RuleTrust::Undecided,
        }
    }

    /// 项目层是否参与装配。
    pub fn is_trusted(self) -> bool {
        matches!(self, RuleTrust::Trusted)
    }
}

/// 信任记录的键：**规范化后的项目根**（与注册表缓存同一套归一，避免同一目录多种写法各存一条）。
pub fn trust_key(project_root: &Path) -> String {
    crate::normalized_project_key(project_root)
        .to_string_lossy()
        .to_string()
}

/// 项目规则集的**内容指纹**：对「相对路径 + 文件内容 SHA256」按字典序排序后聚合，再哈希一次。
///
/// 口径与索引扫描同一来源（[`crate::service::indexer::scan_scope_dir`]）——**解析失败的
/// 文件也算内容**（坏规则也是「这份规则集变了」）。文件增 / 删 / 改 / 重命名都会变；
/// 与扫描顺序无关（排序后聚合）。目录不存在 / 没有规则文件 → `None`。
pub fn ruleset_fingerprint(dir: &Path) -> Option<String> {
    let entries = crate::service::indexer::scan_scope_dir(dir, crate::rule::RuleScope::Project);
    if entries.is_empty() {
        return None;
    }
    let mut parts: Vec<(String, String)> = entries
        .into_iter()
        .map(|entry| (entry.source_path, entry.checksum))
        .collect();
    parts.sort();
    let mut buffer = String::new();
    for (path, checksum) in parts {
        buffer.push_str(&path);
        buffer.push('\0');
        buffer.push_str(&checksum);
        buffer.push('\n');
    }
    Some(crate::service::indexer::sha256_hex(buffer.as_bytes()))
}

/// 同步读取信任状态（`build_registry` 在同步上下文里，见 [`crate::project_rule_trust`]）。
///
/// **不用连接池的 `acquire_sync`**：它在「已处于 tokio 运行时」的调用点会直接返回错误
/// （`acquire_sync` 自己的契约），而 `build_registry` 确实可能从异步路径进来
/// （例如 `sample_source_to_analysis_table` 就是 async）。真让它失败，已信任的项目会**静默**
/// 少装一层规则——正是本功能要消除的那类问题。
///
/// 改为**自开一条只读连接**：一次主键查询，开销微秒级；而且结果会被进程级缓存记住，
/// 每个项目每个会话只会走到这里很少几次。
///
/// **不返回 `Result`**：读不到（无记录 / 表未建 / 出错）一律 `Undecided`，
/// 调用方不需要为「查库失败」准备另一条分支——fail closed 就是唯一正确的失败方向。
pub fn read_at(global_db: &Path, project_root: &Path) -> RuleTrust {
    read_record_at(global_db, project_root).0
}

/// 读取完整信任记录：决定 + 决定时的内容指纹（指纹列由迁移 027 补上）。
///
/// 旧记录的指纹为 `NULL`（027 之前决定、尚未再次决定）：按**纯路径语义**放行
/// （升级零打扰），用户下一次做出决定时起绑定内容。失败口径与 [`read_at`]
/// 相同——一律 fail closed（`Undecided`，指纹按未绑定）。
pub fn read_record_at(global_db: &Path, project_root: &Path) -> (RuleTrust, Option<String>) {
    let key = trust_key(project_root);
    let Ok(db) = rusqlite::Connection::open_with_flags(
        global_db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        tracing::warn!(
            "[rule-trust] 打不开全局库 {}（项目规则按未信任处理）",
            global_db.display()
        );
        return (RuleTrust::Undecided, None);
    };

    match db.query_row(
        "SELECT state, fingerprint FROM insight_rule_trust WHERE project_path = ?1",
        rusqlite::params![key],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
    ) {
        Ok((state, fingerprint)) => (RuleTrust::from_db_value(&state), fingerprint),
        Err(rusqlite::Error::QueryReturnedNoRows) => (RuleTrust::Undecided, None),
        // 表还没建（迁移未跑 / 旧库）不算错，按未决定即可；其余错误要留痕
        Err(e) if e.to_string().contains("no such table") => {
            tracing::debug!("[rule-trust] 信任表尚未建立（迁移 025 未跑），按未信任处理");
            (RuleTrust::Undecided, None)
        }
        // 指纹列还没建（迁移 027 未跑的旧库）：按未决定处理（fail closed；
        // 启动迁移随后补列，用户重新决定即绑定内容）
        Err(e) if e.to_string().contains("no such column") => {
            tracing::debug!("[rule-trust] 信任表缺指纹列（迁移 027 未跑），按未信任处理");
            (RuleTrust::Undecided, None)
        }
        Err(e) => {
            tracing::warn!("[rule-trust] 读取信任记录失败（按未信任处理）: {e}");
            (RuleTrust::Undecided, None)
        }
    }
}

/// 写入信任状态（`Undecided` = 删掉记录，回到「还没决定」）。
///
/// 决定即绑定**此刻的规则集内容指纹**（迁移 027）：这次决定针对的是当前磁盘上的
/// 这份规则集，之后内容变了（git pull / 手改）要重新确认。规则集为空时存空串
/// （`''`）而不是 `NULL`——`NULL` 严格保留给「迁移 027 之前的旧记录」，这样
/// 「信任了空规则集、随后出现规则文件」也能被识别为内容变化。
pub async fn write(
    pool: &GlobalSqlitePool,
    project_root: &Path,
    state: RuleTrust,
) -> Result<(), CoreError> {
    let key = trust_key(project_root);
    let fingerprint =
        ruleset_fingerprint(&crate::rule_registry::get_project_rules_dir(project_root))
            .unwrap_or_default();
    let conn = pool.acquire().await?;
    let db = conn.inner()?;

    let affected = match state.as_db_value() {
        Some(value) => db.execute(
            "INSERT INTO insight_rule_trust (project_path, state, fingerprint, decided_at)
             VALUES (?1, ?2, ?3, CURRENT_TIMESTAMP)
             ON CONFLICT(project_path) DO UPDATE SET
                 state = ?2, fingerprint = ?3, decided_at = CURRENT_TIMESTAMP",
            rusqlite::params![key, value, fingerprint],
        ),
        None => db.execute(
            "DELETE FROM insight_rule_trust WHERE project_path = ?1",
            rusqlite::params![key],
        ),
    }
    .map_err(|e| {
        CoreError::common(CommonError::General(format!(
            "保存项目规则信任状态失败: {e}"
        )))
    })?;

    tracing::info!(
        "[rule-trust] 项目规则信任状态更新：{} → {:?}（指纹已绑定，影响 {affected} 行）",
        key,
        state
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 测试用的全局库：临时目录里一个独立文件，用完删掉。
    async fn temp_db(tag: &str) -> (GlobalSqlitePool, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "rds_rule_trust_{}_{}_{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("临时目录");
        let pool = GlobalSqlitePool::new(dir.join("global.db"), 1)
            .await
            .expect("建全局库");
        // 建表（真机上由迁移 025 建；单测里只关心这一张表）
        {
            let conn = pool.acquire().await.expect("取连接");
            conn.inner()
                .expect("连接")
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS insight_rule_trust (
                        project_path TEXT PRIMARY KEY,
                        state        TEXT NOT NULL CHECK (state IN ('trusted', 'declined')),
                        fingerprint  TEXT,
                        decided_at   TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                    );",
                )
                .expect("建表");
        }
        (pool, dir)
    }

    #[tokio::test]
    async fn test_trust_roundtrip() {
        let (pool, dir) = temp_db("roundtrip").await;
        let db = pool.path().clone();
        let project = dir.join("proj-a");
        std::fs::create_dir_all(&project).expect("项目目录");

        // 默认：没有记录 → 未决定（不是「未信任」，也不是「已信任」）
        assert_eq!(read_at(&db, &project), RuleTrust::Undecided);

        write(&pool, &project, RuleTrust::Trusted)
            .await
            .expect("写信任");
        assert_eq!(read_at(&db, &project), RuleTrust::Trusted);

        // 改主意：拒绝也是一次决定，要覆盖上一条
        write(&pool, &project, RuleTrust::Declined)
            .await
            .expect("写拒绝");
        assert_eq!(read_at(&db, &project), RuleTrust::Declined);

        // 回到未决定 = 删记录
        write(&pool, &project, RuleTrust::Undecided)
            .await
            .expect("删记录");
        assert_eq!(read_at(&db, &project), RuleTrust::Undecided);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 信任绑定项目**路径**：另一个项目不受影响（同库多项目共存）。
    #[tokio::test]
    async fn test_trust_is_per_project() {
        let (pool, dir) = temp_db("per_project").await;
        let db = pool.path().clone();
        let a = dir.join("proj-a");
        let b = dir.join("proj-b");
        std::fs::create_dir_all(&a).expect("a");
        std::fs::create_dir_all(&b).expect("b");

        write(&pool, &a, RuleTrust::Trusted).await.expect("写 a");
        assert_eq!(read_at(&db, &a), RuleTrust::Trusted);
        assert_eq!(read_at(&db, &b), RuleTrust::Undecided);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 键做规范化：同一目录的「带 `..` 写法」与规范写法必须落在同一条记录上，
    /// 否则用户换个入口打开项目就会被重新追问一次。
    #[tokio::test]
    async fn test_trust_key_normalizes_path_forms() {
        let (pool, dir) = temp_db("normalize").await;
        let db = pool.path().clone();
        let project = dir.join("proj-a");
        std::fs::create_dir_all(&project).expect("项目目录");

        write(&pool, &project, RuleTrust::Trusted)
            .await
            .expect("写信任");
        let via_dotdot = project.join("..").join("proj-a");
        assert_eq!(read_at(&db, &via_dotdot), RuleTrust::Trusted);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 表还没建（旧库 / 迁移未跑）时按未决定处理，而不是报错或默默信任。
    #[test]
    fn test_missing_table_is_undecided() {
        let dir = std::env::temp_dir().join(format!(
            "rds_rule_trust_nomig_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("临时目录");
        let db = dir.join("empty.db");
        // 建一个空库（不建表）
        rusqlite::Connection::open(&db).expect("空库");

        assert_eq!(read_at(&db, &dir), RuleTrust::Undecided);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 不认识的值按未决定处理：库里出现脏值时，宁可再问一次也不要默默加载。
    #[test]
    fn test_unknown_db_value_is_undecided() {
        assert_eq!(RuleTrust::from_db_value("trusted"), RuleTrust::Trusted);
        assert_eq!(RuleTrust::from_db_value("declined"), RuleTrust::Declined);
        assert_eq!(RuleTrust::from_db_value("yes-please"), RuleTrust::Undecided);
        assert!(!RuleTrust::Undecided.is_trusted());
        assert!(!RuleTrust::Declined.is_trusted());
        assert!(RuleTrust::Trusted.is_trusted());
    }

    /// 指纹基础性质：稳定、随内容变化（增 / 删 / 改 / 重命名）、与建文件顺序无关、
    /// 空目录 / 缺目录为 `None`。
    #[test]
    fn test_ruleset_fingerprint_tracks_content() {
        let dir = std::env::temp_dir().join(format!(
            "rds_rule_fp_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        // 缺目录 → None
        assert_eq!(ruleset_fingerprint(&dir), None);

        let rules = dir.join(".RSmeta").join("insight-rules");
        std::fs::create_dir_all(&rules).expect("建规则目录");
        // 空目录 → None（没有内容就没有可绑定的东西）
        assert_eq!(ruleset_fingerprint(&rules), None);

        // 两个文件（倒序创建，验证与顺序无关）
        std::fs::write(rules.join("b.rule.toml"), "id = \"b\"").expect("b");
        std::fs::write(rules.join("a.rule.toml"), "id = \"a\"").expect("a");
        let fp1 = ruleset_fingerprint(&rules).expect("有文件必有指纹");

        // 倒序重建同名同内容 → 指纹不变（排序聚合）
        std::fs::remove_file(rules.join("a.rule.toml")).expect("删 a");
        std::fs::remove_file(rules.join("b.rule.toml")).expect("删 b");
        std::fs::write(rules.join("a.rule.toml"), "id = \"a\"").expect("重建 a");
        std::fs::write(rules.join("b.rule.toml"), "id = \"b\"").expect("重建 b");
        assert_eq!(ruleset_fingerprint(&rules).as_deref(), Some(fp1.as_str()));

        // 改内容 → 变
        std::fs::write(rules.join("a.rule.toml"), "id = \"a2\"").expect("改 a");
        let fp2 = ruleset_fingerprint(&rules).expect("仍有指纹");
        assert_ne!(fp1, fp2);

        // 重命名（内容不变）→ 变：绑定的是「路径 + 内容」
        std::fs::rename(rules.join("a.rule.toml"), rules.join("c.rule.toml")).expect("重命名");
        assert_ne!(ruleset_fingerprint(&rules).as_deref(), Some(fp2.as_str()));

        // 删到只剩一个 → 变；删空 → None
        std::fs::remove_file(rules.join("b.rule.toml")).expect("删 b");
        std::fs::remove_file(rules.join("c.rule.toml")).expect("删 c");
        assert_eq!(ruleset_fingerprint(&rules), None);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 决定即绑定内容指纹：write 时规则集的现状被记进记录；
    /// 规则集为空时存空串（''）而不是 NULL——「信任了空集、随后出现文件」
    /// 才能被识别为内容变化。
    #[tokio::test]
    async fn test_write_binds_ruleset_fingerprint() {
        let (pool, dir) = temp_db("bind_fp").await;
        let db = pool.path().clone();

        // 规则集为空的项目：指纹绑定空串
        let empty = dir.join("proj-empty");
        std::fs::create_dir_all(&empty).expect("项目目录");
        write(&pool, &empty, RuleTrust::Trusted)
            .await
            .expect("写信任");
        assert_eq!(
            read_record_at(&db, &empty),
            (RuleTrust::Trusted, Some(String::new())),
            "空规则集绑定空串（不是 NULL——NULL 严格留给旧记录）"
        );

        // 带规则文件的项目：绑定 = 此刻指纹
        let with_rules = dir.join("proj-rules");
        let rules_dir = with_rules.join(".RSmeta").join("insight-rules");
        std::fs::create_dir_all(&rules_dir).expect("建规则目录");
        std::fs::write(rules_dir.join("x.rule.toml"), "id = \"x\"").expect("规则");
        write(&pool, &with_rules, RuleTrust::Trusted)
            .await
            .expect("写信任");
        let expected = ruleset_fingerprint(&rules_dir).expect("指纹");
        assert_eq!(
            read_record_at(&db, &with_rules),
            (RuleTrust::Trusted, Some(expected))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 迁移 027 之前的旧记录（指纹 NULL）：读取为 `(决定, None)`——
    /// 调用方（`effective_trust`）按纯路径语义放行，升级零打扰。
    #[tokio::test]
    async fn test_legacy_record_without_fingerprint_reads_none() {
        let (pool, dir) = temp_db("legacy").await;
        let db = pool.path().clone();
        let project = dir.join("proj-legacy");
        std::fs::create_dir_all(&project).expect("项目目录");

        // 手工插一条无指纹的旧记录（等价于 027 前的存量行）
        {
            let conn = pool.acquire().await.expect("取连接");
            conn.inner()
                .expect("连接")
                .execute(
                    "INSERT INTO insight_rule_trust (project_path, state) VALUES (?1, 'trusted')",
                    rusqlite::params![trust_key(&project)],
                )
                .expect("插旧记录");
        }
        assert_eq!(
            read_record_at(&db, &project),
            (RuleTrust::Trusted, None),
            "旧记录指纹为 NULL：按未绑定读取（纯路径语义）"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
