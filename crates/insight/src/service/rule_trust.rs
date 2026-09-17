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
//! # 已知取舍
//!
//! 信任绑定**项目路径**，不是规则内容。项目里的规则后来被改动（例如 `git pull` 带进来
//! 一条新规则）**不会**重新确认——否则用户自己改规则也会被反复打扰。要更严的口径，
//! 得在这张表上加一列内容指纹（见架构 §12 Q1 的「更严一档」）。

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

/// 同步读取信任状态（`build_registry` 在同步上下文里，见 [`crate::project_rule_trust`]）。
///
/// **不用连接池的 `acquire_sync`**：它在「已处于 tokio 运行时」的调用点会直接返回错误
/// （`acquire_sync` 自己的契约），而 `build_registry` 确实可能从异步路径进来
/// （例如 `profile_column_from_table` 就是 async）。真让它失败，已信任的项目会**静默**
/// 少装一层规则——正是本功能要消除的那类问题。
///
/// 改为**自开一条只读连接**：一次主键查询，开销微秒级；而且结果会被进程级缓存记住，
/// 每个项目每个会话只会走到这里很少几次。
///
/// **不返回 `Result`**：读不到（无记录 / 表未建 / 出错）一律 `Undecided`，
/// 调用方不需要为「查库失败」准备另一条分支——fail closed 就是唯一正确的失败方向。
pub fn read_at(global_db: &Path, project_root: &Path) -> RuleTrust {
    let key = trust_key(project_root);
    let Ok(db) = rusqlite::Connection::open_with_flags(
        global_db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        tracing::warn!(
            "[rule-trust] 打不开全局库 {}（项目规则按未信任处理）",
            global_db.display()
        );
        return RuleTrust::Undecided;
    };

    match db.query_row(
        "SELECT state FROM insight_rule_trust WHERE project_path = ?1",
        rusqlite::params![key],
        |row| row.get::<_, String>(0),
    ) {
        Ok(state) => RuleTrust::from_db_value(&state),
        Err(rusqlite::Error::QueryReturnedNoRows) => RuleTrust::Undecided,
        // 表还没建（迁移未跑 / 旧库）不算错，按未决定即可；其余错误要留痕
        Err(e) if e.to_string().contains("no such table") => {
            tracing::debug!("[rule-trust] 信任表尚未建立（迁移 025 未跑），按未信任处理");
            RuleTrust::Undecided
        }
        Err(e) => {
            tracing::warn!("[rule-trust] 读取信任记录失败（按未信任处理）: {e}");
            RuleTrust::Undecided
        }
    }
}

/// 写入信任状态（`Undecided` = 删掉记录，回到「还没决定」）。
pub async fn write(
    pool: &GlobalSqlitePool,
    project_root: &Path,
    state: RuleTrust,
) -> Result<(), CoreError> {
    let key = trust_key(project_root);
    let conn = pool.acquire().await?;
    let db = conn.inner()?;

    let affected = match state.as_db_value() {
        Some(value) => db.execute(
            "INSERT INTO insight_rule_trust (project_path, state, decided_at)
             VALUES (?1, ?2, CURRENT_TIMESTAMP)
             ON CONFLICT(project_path) DO UPDATE SET state = ?2, decided_at = CURRENT_TIMESTAMP",
            rusqlite::params![key, value],
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
        "[rule-trust] 项目规则信任状态更新：{} → {:?}（影响 {affected} 行）",
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
}
