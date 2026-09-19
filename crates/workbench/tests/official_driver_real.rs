//! 真机验收：Official 驱动（`mysql_native` / `postgres_native`）的连接串归一与 TLS 落地。
//!
//! 为什么单开一个文件：既有真机套件（`editor_exec_real.rs` 等）把环境变量里的 URL
//! **原样**塞进 `DriverConnectionConfig.url_override`（scheme 已经是 `mysql://`），
//! 因此**测不到应用自己的连接串构造路径**——而「选 Official 驱动保存 → 导航点连接」
//! 正是走那条路径：`connection::url::build_connection_url` 拿**驱动 id** 当 scheme，
//! 拼出 `mysql_native://…`，而 `mysql_async` 只认 `mysql://`（`tokio-postgres` 同理）。
//!
//! 覆盖四条线（对应 2026-09-19 两轮修复）：
//! 1. record → `{驱动 id}://…` → 工厂归一 → 真机连接 + 真实查询；
//! 2. TLS：`require` 能连（服务端支持时）；一旦 `require` 成功，`verify-full` 无 CA
//!    **必须失败**——否则说明证书校验被跳过（这正是修复前 `postgres_native` 的行为）；
//! 3. `secret_type_of` 在**真实驱动目录**下把 `mysql_native` 归到 `MYSQL`；
//! 4. 驱动属性：声明里的键（`drivers.driver_properties`）下发后能连，而对话框**旧初值**
//!    （`ssl_mode` / `connect_timeout`）会被驱动拒下——两个 Official 驱动对未知参数是报错而非忽略
//!    （能力矩阵 §2.1；旧初值已由对话框架构决策 #90 删除）。
//!
//! 跑法（sh / bash 下路径与 URL 一律单引号，反斜杠会被吃）：
//!
//! ```text
//! RDS_TEST_MYSQL_URL='mysql://root:root@192.168.3.138:3306/mysql' \
//! RDS_TEST_PG_URL='postgres://postgres:postgresql@192.168.3.138:5432/postgres' \
//! cargo test -p rds-workbench -j 2 --test official_driver_real -- --nocapture --test-threads=1
//! ```
//!
//! 测试数据根由 `paths` 的 `test-support` 隔离（`HomeOrigin::TestRoot`），
//! 不会碰开发/生产数据；连接一律 `skip_persistence`，不写库。

use std::sync::Arc;

use connection::model::{ConnectionScope, DataSource};
use connection::url::build_connection_url;
use engine::connection_manager::{ConnectionManager, ConnectionType};
use engine::persistence::global_db::GlobalDatabaseManager;
use rds_workbench::services::connection_service::{ConnectRequest, ConnectionService};
use rds_workbench::services::secret_integration;

/// 环境变量名 ↔ 驱动 id。
const TARGETS: [(&str, &str); 2] = [
    ("mysql_native", "RDS_TEST_MYSQL_URL"),
    ("postgres_native", "RDS_TEST_PG_URL"),
];

/// 按 record 组装的 URL 建连（生产路径：`build_connection_url` → 工厂）。
///
/// 返回 `(连接 ID, 数据库实例)`；`advanced_options` 用于带「连接安全」的 SSL 覆盖，
/// `driver_properties` 用于带属性页的键（两者都按生产路径下发到连接串）。
/// **必须传同一个 runtime**：`mysql_async` 的连接池绑定在创建它的 runtime 上，
/// 换一个 runtime 查询会得到 `Pool was disconnected`（第一次写这个测试就踩了）。
fn connect_via_record(
    runtime: &tokio::runtime::Runtime,
    driver: &str,
    url: &str,
    advanced_options: Option<&str>,
    driver_properties: Option<&str>,
) -> Result<(String, engine::driver::DynDatabase), String> {
    engine::driver::AutoDriverRegistrar::auto_register();

    let parts = secret_integration::parse_connection_url(url)
        .ok_or_else(|| format!("环境变量 URL 无法解析：{url}"))?;
    let record = DataSource {
        id: "G_conn_official_probe".to_string(),
        name: format!("Official 驱动真机探针（{driver}）"),
        // 关键前提：记录里的 db_type 是**驱动 id**（这正是 bug 的入口）
        db_type: driver.to_string(),
        host: Some(parts.host.clone()),
        port: Some(parts.port),
        database: Some(parts.database.clone()),
        schema_name: None,
        username: Some(parts.user.clone()),
        password_encrypted: if parts.password.is_empty() {
            None
        } else {
            Some(shared::crypto::encrypt_password(&parts.password).map_err(|e| e.to_string())?)
        },
        description: None,
        driver_id: Some(driver.to_string()),
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: driver_properties.map(str::to_string),
        advanced_options: advanced_options.map(str::to_string),
        options: None,
        tags: None,
        use_duckdb_fed: false,
        metadata_path: None,
        server_version: None,
        scope: ConnectionScope::Global,
        is_active: true,
        created_at: String::new(),
        updated_at: String::new(),
    };

    let built = build_connection_url(&record)?;
    assert!(
        built.starts_with(&format!("{driver}://")),
        "前提校验：record 组装出的 scheme 就是驱动 id（实际 {built}）——这正是修复前会让驱动报错的那条串"
    );

    let manager = Arc::new(ConnectionManager::new());
    let service = ConnectionService::new(manager);
    let req = ConnectRequest {
        conn_id: Some(record.id.clone()),
        db_type: driver.to_string(),
        url: built,
        name: Some(record.name.clone()),
        connection_type: ConnectionType::Global,
        project_path: None,
        description: None,
        driver_id: Some(driver.to_string()),
        environment_id: None,
        auth_config_id: None,
        auth_method: None,
        network_config_id: None,
        driver_properties: driver_properties.map(str::to_string),
        advanced_options: advanced_options.map(str::to_string),
        options: None,
        tags: None,
        metadata_path: None,
        schema_name: None,
        use_duckdb_fed: Some(false),
        password: None,
        skip_persistence: Some(true),
        network_method: None,
    };

    runtime
        .block_on(service.connect_with_type(req))
        .map_err(|e| e.to_string())
}

/// 查询一句并等它回来（证明连接真的可用，不只是握手成功）。
fn query_one(
    runtime: &tokio::runtime::Runtime,
    db: &engine::driver::DynDatabase,
) -> Result<(), String> {
    runtime
        .block_on(db.query("SELECT 1"))
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 线路 ④：驱动属性（`driver_properties`）真机验收——**声明的键真能连，UI 编的键会被拒**。
///
/// 为什么单列一条：属性键的“真伪”只取决于客户端库的解析器——sqlx 静默忽略未知键，
/// 而 `mysql_async` / `tokio-postgres` **直接报错**（`UnknownParameter` / `UnknownOption`，
/// 见 `driver-capability-matrix.md` §2.1）。对话框属性页的默认值现在取**驱动声明**
/// （`drivers.driver_properties`，由 `driver/declaration.rs` 从 `descriptors.rs` 灌入），
/// 所以「声明里的键下发后连得上」必须由真机钉住。
///
/// 反向用例用的是对话框**旧初值**（`ssl_mode=prefer` + `connect_timeout=10`，决策 #90 已删）：
/// 它们在 Official 驱动上必须报错——若以后有人把“猜测型默认值”加回 UI，本条提供反例。
#[test]
fn declared_driver_properties_connect_and_ui_fabricated_keys_are_rejected() {
    engine::driver::AutoDriverRegistrar::auto_register();
    let rt = runtime();
    let mut checked = 0;

    for (driver, env) in TARGETS {
        let Ok(url) = std::env::var(env) else {
            eprintln!("⏭️  {driver}：未设 {env}，跳过");
            continue;
        };
        checked += 1;

        // ① 声明里写的键（直接从声明读，不在这里手抄一份）
        let declared = declared_properties_json(driver)
            .unwrap_or_else(|| panic!("{driver} 的声明里没有 driver_properties——属性页就无默认值可填"));
        let (_, db) = connect_via_record(&rt, driver, &url, None, Some(&declared)).unwrap_or_else(|e| {
            panic!("{driver} 带声明属性 {declared} 真机连接失败：{e}")
        });
        query_one(&rt, &db).expect("带声明属性时应能查询");
        eprintln!("✅ {driver}：声明的驱动属性 {declared} 下发后连接与查询均正常");

        // ② 旧 UI 初值（两个键对两个 Official 驱动都不是它们的参数名）必须被拒
        let fabricated = r#"{"ssl_mode":"prefer","connect_timeout":"10"}"#;
        match connect_via_record(&rt, driver, &url, None, Some(fabricated)) {
            Err(e) => eprintln!(
                "✅ {driver}：UI 旧初值被驱动拒下（{e}）——这正是决策 #90 要防的形态"
            ),
            Ok(_) => panic!(
                "{driver}：UI 编的属性键居然被接受了——能力矩阵 §2.1 的「未知参数」判断需要重查"
            ),
        }
    }

    if checked == 0 {
        eprintln!("⚠️  未设 RDS_TEST_MYSQL_URL / RDS_TEST_PG_URL，本项未覆盖（不算失败）");
    }
}

/// 从**驱动声明**取该驱动声明的属性默认值（JSON 对象文本）。
fn declared_properties_json(driver: &str) -> Option<String> {
    let d = engine::driver::DriverRegistry::all_descriptors()
        .into_iter()
        .find(|d| d.id == driver)?;
    if d.driver_properties.is_empty() {
        return None;
    }
    serde_json::to_string(&d.driver_properties).ok()
}

/// 一个测试用 runtime（与池同生共死）。
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().expect("tokio runtime")
}

/// 线路 ①：`{驱动 id}://…` 的连接串能被 Official 驱动接住（修复前必失败）。
#[test]
fn official_drivers_connect_from_a_record_built_url() {
    let rt = runtime();
    let mut checked = 0;
    for (driver, env) in TARGETS {
        let Ok(url) = std::env::var(env) else {
            eprintln!("⏭️  {driver}：未设 {env}，跳过");
            continue;
        };
        checked += 1;

        let (conn_id, db) = connect_via_record(&rt, driver, &url, None, None)
            .unwrap_or_else(|e| panic!("{driver} 真机连接失败：{e}"));
        query_one(&rt, &db).unwrap_or_else(|e| panic!("{driver} 查询失败：{e}"));
        eprintln!("✅ {driver}：驱动 id 当 scheme 也能连上（conn_id={conn_id}）且查询可用");
    }
    if checked == 0 {
        eprintln!("⚠️  未设 RDS_TEST_MYSQL_URL / RDS_TEST_PG_URL，真机链路未验证（不算失败）");
    }
}

/// 线路 ②：TLS——`require` 的连接行为 + `verify-*` 的**校验必须真的发生**。
#[test]
fn official_driver_tls_require_connects_and_verify_is_real() {
    let rt = runtime();
    for (driver, env) in TARGETS {
        let Ok(url) = std::env::var(env) else {
            eprintln!("⏭️  {driver}：未设 {env}，跳过");
            continue;
        };

        let require = connect_via_record(
            &rt,
            driver,
            &url,
            Some(r#"{"ssl":{"mode":"require"}}"#),
            None,
        );
        match require {
            Ok((_, db)) => {
                query_one(&rt, &db).expect("require 模式下查询应可用");
                eprintln!("✅ {driver}：mode=require 已加密连接且查询可用");

                // 关键断言：require 成功说明服务端支持 TLS；此时 verify-full 在**没有 CA**
                // 的情况下必须失败，否则就是「填了校验却被跳过」（修复前 postgres_native 就是如此）。
                let verify = connect_via_record(
                    &rt,
                    driver,
                    &url,
                    Some(r#"{"ssl":{"mode":"verify-full"}}"#),
                    None,
                );
                match verify {
                    Err(e) => eprintln!("✅ {driver}：verify-full 如预期失败（{e}）"),
                    Ok(_) => panic!(
                        "{driver}：verify-full 居然连上了——说明证书链/主机名校验没有真的执行"
                    ),
                }
            }
            Err(e) => {
                // 服务端没开 TLS（或拒绝加密握手）时，本项在真机上无法覆盖——如实记录，不假装通过。
                eprintln!("⚠️  {driver}：mode=require 失败（{e}）——服务端可能未启用 TLS，本项未覆盖");
            }
        }
    }
}

/// 诊断：真机 PG 到底有没有开 TLS（决定上面 `require` 的失败是服务端能力还是我们的缺陷）。
///
/// 判据用 **sqlx 驱动 + `?sslmode=require`**——这条路子的 TLS 参数支持无争议
/// （`PgSslMode` 五档就在 URL 上），若它也失败，就是服务端没启 TLS。
/// 不拿 `SHOW ssl` / `SELECT current_setting('ssl')` 当判据：那两个查询在 sqlx 驱动里
/// 只填 `batches`（`rows` / `total_rows` 为空，是既有实现的取舍），读值先要解 Arrow，
/// 而且 `SHOW` 实测连行都不回。
#[test]
fn pg_server_tls_capability_diagnostic() {
    let Ok(url) = std::env::var("RDS_TEST_PG_URL") else {
        eprintln!("⏭️  未设 RDS_TEST_PG_URL，跳过");
        return;
    };
    engine::driver::AutoDriverRegistrar::auto_register();
    let rt = runtime();

    let plain = route(&rt, "postgres", &url);
    assert!(plain.is_ok(), "明文连接应可用（既有真机套件同路径）");

    let with_tls = route(&rt, "postgres", &format!("{url}?sslmode=require"));
    match with_tls {
        Ok(_) => eprintln!("ℹ️  真机 PG **支持** TLS（sqlx sslmode=require 连上了）"),
        Err(e) => eprintln!("ℹ️  真机 PG **未启用** TLS（sqlx sslmode=require 也失败：{e}）——本项未覆盖"),
    }
}

/// 用给定驱动 + URL 直接路由建连（真机探针共用；`DataSourceRouter::route` 是 async）。
fn route(
    runtime: &tokio::runtime::Runtime,
    driver: &str,
    url: &str,
) -> Result<engine::driver::DynDatabase, String> {
    let config = engine::DriverConnectionConfig::new(driver).with_url_override(url.to_string());
    runtime
        .block_on(engine::driver::DataSourceRouter::route(config))
        .map_err(|e| e.to_string())
}

/// 线路 ③：Secret 族解析要能在**真实驱动目录**下成立（目录来自迁移种子）。
#[test]
fn secret_family_resolves_against_the_real_catalog() {
    let rt = runtime();
    let installed = rt.block_on(async {
        let sqlite_path = engine::migration::get_global_db_path().expect("全局库路径");
        let duckdb_path = engine::migration::get_global_duckdb_path().expect("分析库路径");
        let manager = GlobalDatabaseManager::new(sqlite_path, duckdb_path, 2)
            .await
            .expect("建全局库（含迁移种子）");
        // 已在别处装过就忽略（同一测试二进制内只可能装一次）
        let _ = engine::migration::install_global_db_manager(manager);
        true
    });
    assert!(installed);

    // 驱动 id → 族 id → Secret 类型：修复前 `mysql_native` 直接返回 None（静默不注册）
    assert_eq!(
        secret_integration::secret_type_of("mysql_native").as_deref(),
        Some("MYSQL"),
        "真实驱动目录应把 mysql_native 归到 mysql"
    );
    assert_eq!(
        secret_integration::secret_type_of("postgres_native").as_deref(),
        Some("POSTGRES"),
        "真实驱动目录应把 postgres_native 归到 postgresql"
    );
    // 族 id 与未知值：前者照旧，后者仍为 None（不编造）
    assert_eq!(
        secret_integration::secret_type_of("postgresql").as_deref(),
        Some("POSTGRES")
    );
    assert_eq!(secret_integration::secret_type_of("oracle"), None);
}
