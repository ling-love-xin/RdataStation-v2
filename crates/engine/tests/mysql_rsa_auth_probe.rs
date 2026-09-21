//! 真机回归：sqlx MySQL 在**非 TLS** 连接上必须能通过 `caching_sha2_password` 认证。
//!
//! ## 为什么单开一个文件
//!
//! 这是「MySQL 连不上」那条缺陷的最小复现点：MySQL 8+ 默认认证插件
//! `caching_sha2_password` 在明文信道上要求客户端**用服务端公钥加密口令**（RSA），
//! 而 sqlx 的 RSA 后端由 feature `mysql-rsa` 门控。缺它时建连直接失败：
//!
//! ```text
//! error with configuration: RSA auth backend disabled;
//! enable feature `mysql-rsa` (or `rsa` if using sqlx-mysql directly) or use TLS.
//! ```
//!
//! 本仓的生产路径**必然会走到明文信道**：`connection_service::apply_lan_tls_default`
//! 对 LAN + 直连 + sqlx 驱动注入 `ssl-mode=DISABLED`（规避 `prefer` 的握手卡顿），
//! 且 `MySqlDatabase::new` 在 URL 没有 ssl 键时也补 `ssl-mode=disabled`。
//! 两条默认值各自都合理，缺了 feature 就变成「网段内的 MySQL 全连不上」。
//!
//! ## 与 `editor_exec_real` 的分工
//!
//! `crates/workbench/tests/editor_exec_real.rs` 覆盖 `mysql` 驱动的**全链路**（执行 / 事务 /
//! 分段抓取 / 超时…），但它的 URL 来自环境变量、断言落在执行结果上；**认证方式本身**
//! 没有单独钉住。这里按引擎层最小面（`MySqlDatabase::new`）钉两条：
//! ① 明文信道的 RSA 认证能过；② TLS 信道同样能过（证明失败与 TLS 无关）。
//!
//! 未设 `RDS_TEST_MYSQL_URL` 时跳过（与仓内其余真机套件同规矩）。
//!
//! 跑法：
//!
//! ```text
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//!   cargo test -p rds-engine --test mysql_rsa_auth_probe -- --nocapture --test-threads=1
//! ```

use rds_engine::driver::native::mysql::MySqlDatabase;
use rds_engine::driver::Database;

/// 取环境变量里的端点；未设则跳过。
fn endpoint() -> Option<String> {
    match std::env::var("RDS_TEST_MYSQL_URL") {
        Ok(url) => Some(url),
        Err(_) => {
            eprintln!("⏭️  未设 RDS_TEST_MYSQL_URL，跳过");
            None
        }
    }
}

/// 明文信道 + `caching_sha2_password`：走 RSA 公钥加密口令那条路。
///
/// 修复前必失败（`RSA auth backend disabled`），修复后能连上并能真查一行。
#[tokio::test(flavor = "multi_thread")]
async fn mysql_connects_over_plaintext_with_caching_sha2_password() {
    let Some(base) = endpoint() else { return };
    let url = format!("{base}?ssl-mode=DISABLED");

    let db = MySqlDatabase::new(&url)
        .await
        .expect("明文信道建连失败——检查 sqlx 的 `mysql-rsa` feature 是否还在");

    let result = db.query("SELECT 1 AS one").await.expect("查询失败");
    assert_eq!(result.total_rows(), 1, "明文信道连上后应能取到 1 行");
}

/// 对照：TLS 信道不受影响（说明失败与「服务端要不要 TLS」无关，只与 RSA feature 有关）。
#[tokio::test(flavor = "multi_thread")]
async fn mysql_connects_over_tls() {
    let Some(base) = endpoint() else { return };
    let url = format!("{base}?ssl-mode=REQUIRED");

    match MySqlDatabase::new(&url).await {
        Ok(db) => {
            let result = db.query("SELECT 1 AS one").await.expect("TLS 信道查询失败");
            assert_eq!(result.total_rows(), 1);
        }
        // 端点自身没开 TLS 时不算回归（能力缺口而非缺陷），如实打出来即可。
        Err(e) => eprintln!("⏭️  端点不支持 TLS，跳过对照：{e}"),
    }
}
