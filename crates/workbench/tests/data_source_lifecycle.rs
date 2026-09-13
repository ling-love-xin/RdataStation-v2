//! M3 数据源连接：服务层单元测试 + 全局联动测试。
//!
//! 覆盖 `DataSourceService` 的保存 / 回读 / 更新 / 删除 / 测试连接与作用域规则，
//! 并验证「服务写入 → 加载器读回」的工作台联动链路。
//!
//! 隔离原则：全局库、分析库与 Secret 目标库全部落在临时目录，不触碰用户真实数据；
//! loader 为同步入口（内部自建 runtime），故用普通 `#[test]` 显式落 runtime。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use connection::model::{ConnectionScope, DataSourceSaveInput};
use engine::persistence::global_db::GlobalDatabaseManager;
use engine::persistence::project_connection_store::ProjectConnectionStore;
use engine::persistence::project_db::ProjectDatabaseManager;
use rds_workbench::services::data_source_service::DataSourceService;
use rds_workbench::services::workspace_loader::load_persisted_connections_from;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_ds_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Runtime::new().expect("runtime")
}

/// 构造测试服务：全局库落临时目录；Secret 目标库隔离（不写用户分析库）；
/// `&'static` 由 `Box::leak` 提供（测试进程生命周期内常驻，可接受）。
fn make_service(dir: &Path) -> DataSourceService {
    make_service_with_db(dir).1
}

/// 同 `make_service`，额外返回全局库句柄（需要直接写元数据表的用例用）。
fn make_service_with_db(dir: &Path) -> (&'static GlobalDatabaseManager, DataSourceService) {
    let rt = runtime();
    let manager = rt
        .block_on(GlobalDatabaseManager::new(
            dir.join("global.db"),
            dir.join("analytics.duckdb"),
            2,
        ))
        .expect("init global db");
    let manager: &'static GlobalDatabaseManager = Box::leak(Box::new(manager));
    let service = DataSourceService::new(manager).with_analysis_db(dir.join("secret-target.duckdb"));
    (manager, service)
}

fn input(name: &str, db_type: &str, url: &str) -> DataSourceSaveInput {
    DataSourceSaveInput::new(name, db_type, url)
}

/// 字段是否为空（存储层用空串代替 NULL，两者视为“未填写”）。
fn blank(v: Option<&str>) -> bool {
    v.map(|s| s.trim().is_empty()).unwrap_or(true)
}

/// 打开项目侧连接仓库（与 `DataSourceService` 内部 `open_project_store` 同构）。
async fn open_project_store(project_root: &Path) -> ProjectConnectionStore {
    let db = ProjectDatabaseManager::open(project_root, 4)
        .await
        .expect("open project db");
    ProjectConnectionStore::new(Arc::new(db))
}

#[test]
fn catalog_drivers_resolve_and_sqlite_probe_succeeds() {
    // 驱动目录（global.db `drivers` 种子）里的 id 必须能在 DriverRegistry 解析——
    // 真机曾报 `CONN_DRIVER_NOT_FOUND: Driver 'sqlite' not found in registry`（应用启动漏注册）。
    engine::driver::AutoDriverRegistrar::register_builtin_drivers();
    let dir = temp_dir("probe");
    let service = make_service(&dir);
    let rt = runtime();

    let drivers = rt.block_on(service.list_drivers()).expect("list drivers");
    assert!(!drivers.is_empty(), "种子驱动目录不应为空");
    for d in &drivers {
        assert!(
            engine::driver::DriverRegistry::get(&d.id).is_some(),
            "驱动目录里的 id 必须能解析到注册表：{}",
            d.id
        );
    }

    // SQLite 测试连接（真实文件）：文件型工厂从 url_override 取地址（不再要求 config.database 非空），
    // 成功时引擎会创建空库文件并探测版本。
    let db_path = dir.join("probe.db");
    let url = db_path.to_string_lossy().to_string();
    let result = rt.block_on(service.test(&input("probe_sqlite", "sqlite", &url), None));
    assert!(result.success, "SQLite 测试连接应成功：{}", result.message);
    assert!(db_path.exists(), "测试连接应创建数据库文件：{url}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_then_list_roundtrip_global_scope() {
    let dir = temp_dir("roundtrip");
    let service = make_service(&dir);
    let rt = runtime();

    let mut i = input("demo_pg", "postgres", "postgres://u:p@10.0.0.9:5433/warehouse");
    i.username = Some("alice".into());
    i.password = Some("s3cret".into());
    i.use_duckdb_fed = Some(true);

    let id = rt.block_on(service.save(&i, None)).expect("save");
    assert!(id.starts_with("G_conn_"), "仅全局应生成 G_ 前缀：{id}");

    let list = rt.block_on(service.list()).expect("list");
    assert_eq!(list.len(), 1);
    let ds = &list[0];
    assert_eq!(ds.id, id);
    assert_eq!(ds.name, "demo_pg");
    assert_eq!(ds.db_type, "postgres");
    assert_eq!(ds.host.as_deref(), Some("10.0.0.9"));
    assert_eq!(ds.port, Some(5433));
    assert_eq!(ds.database.as_deref(), Some("warehouse"));
    assert_eq!(ds.username.as_deref(), Some("alice"));
    assert!(ds.password_encrypted.is_some(), "凭据应加密落库");
    assert!(ds.use_duckdb_fed);
    assert_eq!(ds.scope, ConnectionScope::Global);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_rejects_duplicate_name_case_insensitive() {
    let dir = temp_dir("dup");
    let service = make_service(&dir);
    let rt = runtime();

    rt.block_on(service.save(
        &input("DemoDB", "mysql", "mysql://u:p@127.0.0.1:3306/d1"),
        None,
    ))
    .expect("first save");

    let err = rt
        .block_on(service.save(
            &input("demodb", "mysql", "mysql://u:p@127.0.0.1:3306/d2"),
            None,
        ))
        .expect_err("同名应被拦截");
    assert!(err.to_string().contains("已存在"), "{err}");

    let list = rt.block_on(service.list()).expect("list");
    assert_eq!(list.len(), 1, "重名不应覆盖或新增");
    assert_eq!(list[0].name, "DemoDB");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn update_changes_fields_and_guards_duplicate_name() {
    let dir = temp_dir("update");
    let service = make_service(&dir);
    let rt = runtime();

    let a_id = rt
        .block_on(service.save(
            &input("alpha", "postgres", "postgres://u:p@127.0.0.1:5432/adb"),
            None,
        ))
        .expect("save a");
    let b_id = rt
        .block_on(service.save(
            &input("beta", "postgres", "postgres://u:p@127.0.0.1:5432/bdb"),
            None,
        ))
        .expect("save b");

    // 正常更新：改名 + 换端口。
    let mut upd = input("beta_v2", "postgres", "postgres://u:p@10.1.1.1:6543/bdb2");
    upd.username = Some("bob".into());
    upd.password = Some("newpw".into());
    rt.block_on(service.update(&b_id, &upd, None)).expect("update");

    let b = rt
        .block_on(service.get_global(&b_id))
        .expect("get")
        .expect("exists");
    assert_eq!(b.name, "beta_v2");
    assert_eq!(b.host.as_deref(), Some("10.1.1.1"));
    assert_eq!(b.port, Some(6543));
    assert_eq!(b.database.as_deref(), Some("bdb2"));

    // 更新为他人名称 → 拦截。
    let mut clash = input("alpha", "postgres", "postgres://u:p@127.0.0.1:5432/bdb");
    clash.username = Some("bob".into());
    let err = rt
        .block_on(service.update(&b_id, &clash, None))
        .expect_err("同名应被拦截");
    assert!(err.to_string().contains("已存在"), "{err}");

    // 自身原名更新不触发重名误报。
    let mut self_name = input("beta_v2", "postgres", "postgres://u:p@10.1.1.1:6543/bdb2");
    self_name.username = Some("bob".into());
    rt.block_on(service.update(&b_id, &self_name, None))
        .expect("同名自身更新应通过");

    assert_eq!(rt.block_on(service.list()).unwrap().len(), 2);
    assert_eq!(a_id, "G_conn_alpha");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_removes_connection_and_cleans_secret() {
    let dir = temp_dir("delete");
    let service = make_service(&dir);
    let rt = runtime();

    let mut i = input("fed_pg", "postgres", "postgres://bob:pw@127.0.0.1:5432/analytics");
    i.use_duckdb_fed = Some(true);
    let id = rt.block_on(service.save(&i, None)).expect("save");

    // 保存应联动注册 DuckDB Secret（目标库与 Secret 目录均隔离在临时目录）。
    let secret_db = dir.join("secret-target.duckdb");
    let secret_dir = dir.join("secrets");
    let mgr = connection::secret::SecretManager::open_with_dir(&secret_db, Some(&secret_dir))
        .expect("open secret db");
    let secrets = mgr.list().expect("list secrets");
    assert_eq!(secrets.len(), 1, "保存后应注册联邦 Secret：{secrets:?}");
    assert_eq!(secrets[0].name, id.to_lowercase(), "Secret 名为连接 ID 小写形式");
    drop(mgr);

    let result = rt.block_on(service.delete(&id, None)).expect("delete");
    assert!(result.removed_secret, "删除应清理 Secret：{result:?}");
    assert!(result.message.contains("Secret 已清理"));

    let mgr = connection::secret::SecretManager::open_with_dir(&secret_db, Some(&secret_dir))
        .expect("reopen secret db");
    assert!(mgr.list().expect("list").is_empty(), "Secret 应被移除");
    drop(mgr);

    assert!(rt.block_on(service.list()).unwrap().is_empty(), "列表应为空");

    // 再次删除 → 记录不存在（engine 删除不报错），Secret 清理为 false 不视为失败。
    let again = rt.block_on(service.delete(&id, None)).expect("delete again");
    assert!(!again.removed_secret);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_without_federation_does_not_register_secret() {
    let dir = temp_dir("no-register");
    let service = make_service(&dir);
    let rt = runtime();

    let mut i = input("plain_pg", "postgres", "postgres://u:p@127.0.0.1:5432/plain");
    i.use_duckdb_fed = Some(false);
    rt.block_on(service.save(&i, None)).expect("save");

    let secret_dir = dir.join("secrets");
    let mgr = connection::secret::SecretManager::open_with_dir(
        dir.join("secret-target.duckdb"),
        Some(&secret_dir),
    )
    .expect("open secret db");
    assert!(
        mgr.list().expect("list").is_empty(),
        "未开启联邦加速不应注册 Secret"
    );
    drop(mgr);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn delete_without_federation_skips_secret() {
    let dir = temp_dir("nofed");
    let service = make_service(&dir);
    let rt = runtime();

    let mut i = input("plain_sqlite", "sqlite", "sqlite:///tmp/plain.db");
    i.use_duckdb_fed = Some(false);
    let id = rt.block_on(service.save(&i, None)).expect("save");

    let result = rt.block_on(service.delete(&id, None)).expect("delete");
    assert!(!result.removed_secret, "未开启联邦不应产生 Secret 清理");
    assert_eq!(result.message, "连接已删除");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_scope_requires_project_path_and_writes_nothing() {
    let dir = temp_dir("proj-precheck");
    let service = make_service(&dir);
    let rt = runtime();

    let mut i = input("proj_only", "sqlite", "sqlite:///tmp/x.db");
    i.scope = ConnectionScope::Project;

    let err = rt
        .block_on(service.save(&i, None))
        .expect_err("项目作用域缺项目路径应失败");
    assert!(err.to_string().contains("未打开项目"), "{err}");
    assert!(
        rt.block_on(service.list()).unwrap().is_empty(),
        "预检失败不应产生全局半成品"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn global_and_project_scope_writes_both_sides() {
    let dir = temp_dir("gp-scope");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let service = make_service(&dir);
    let rt = runtime();

    let mut i = input("gp_pg", "postgres", "postgres://u:p@127.0.0.1:5432/gpdb");
    i.scope = ConnectionScope::GlobalAndProject;
    i.username = Some("carol".into());
    i.password = Some("pw".into());
    i.use_duckdb_fed = Some(true);
    let path = project_root.to_string_lossy().to_string();

    let pid = rt
        .block_on(service.save(&i, Some(&path)))
        .expect("save gp");
    assert!(pid.starts_with("GP_conn_"), "全局+项目应返回 GP_ 快照 ID：{pid}");

    // 全局侧：G_ 定义可回读。
    let list = rt.block_on(service.list()).unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].id.starts_with("G_conn_"), "全局侧应为 G_ 定义");

    // 项目侧：GP_ 快照可回读（含加密凭据），字段与输入一致。
    let proj = rt
        .block_on(async {
            let store = open_project_store(&project_root).await;
            store.get_connection(&pid).await
        })
        .expect("get project conn")
        .expect("项目连接存在");
    assert_eq!(proj.name, "gp_pg");
    assert_eq!(proj.driver, "postgres");
    assert_eq!(proj.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(proj.port, Some(5432));
    assert_eq!(proj.database.as_deref(), Some("gpdb"));
    assert!(proj.password_encrypted.is_some(), "项目侧凭据应加密");

    // 对话框编辑回读路径（`get_with_project`）：项目侧连接必须带项目根才取得到。
    let ds = rt
        .block_on(service.get_with_project(&pid, Some(&path)))
        .expect("get_with_project")
        .expect("项目侧连接应可读回");
    assert_eq!(ds.name, "gp_pg");
    assert_eq!(ds.db_type, "postgres");
    assert_eq!(ds.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(ds.port, Some(5432));
    assert_eq!(ds.database.as_deref(), Some("gpdb"));
    assert_eq!(ds.username.as_deref(), Some("carol"));
    assert_eq!(
        ds.scope,
        ConnectionScope::GlobalAndProject,
        "GP_ 应回读为全局+项目"
    );
    // 不带项目根：项目侧取不到（不报错，由 UI 降级）；全局 `get` 也查不到 GP_ 行。
    assert!(rt
        .block_on(service.get_with_project(&pid, None))
        .unwrap()
        .is_none());
    assert!(rt.block_on(service.get_global(&pid)).unwrap().is_none());

    // 项目侧删除：路由到项目库，不误删全局定义。
    rt.block_on(service.delete(&pid, Some(&path)))
        .expect("delete project side");
    assert_eq!(rt.block_on(service.list()).unwrap().len(), 1, "全局定义仍在");
    let gone = rt.block_on(async {
        let store = open_project_store(&project_root).await;
        store.get_connection(&pid).await
    });
    assert!(gone.expect("query").is_none(), "项目连接应已删除");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_scope_readback_requires_project_path() {
    let dir = temp_dir("proj-readback");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let service = make_service(&dir);
    let rt = runtime();
    let path = project_root.to_string_lossy().to_string();

    let mut i = input("proj_read", "sqlite", "sqlite:///tmp/p.db");
    i.scope = ConnectionScope::Project;
    i.username = Some("reader".into());
    i.tags = Some(r#"["dev"]"#.to_string());
    let id = rt
        .block_on(service.save(&i, Some(&path)))
        .expect("save project only");
    assert!(id.starts_with("P_"), "仅项目应返回 P_ 前缀：{id}");

    // 编辑器回读：带项目根可读回，作用域按 ID 前缀回推为“仅项目”。
    let ds = rt
        .block_on(service.get_with_project(&id, Some(&path)))
        .expect("get_with_project")
        .expect("项目侧连接应可读回");
    assert_eq!(ds.name, "proj_read");
    assert_eq!(ds.db_type, "sqlite");
    assert_eq!(ds.scope, ConnectionScope::Project);
    assert_eq!(ds.tags.as_deref(), Some(r#"["dev"]"#));

    // 不带项目根 / 走全局 `get`：取不到（不报错，UI 降级为空表单）。
    assert!(rt
        .block_on(service.get_with_project(&id, None))
        .unwrap()
        .is_none());
    assert!(rt.block_on(service.get_global(&id)).unwrap().is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_db_path_survives_save_and_readback() {
    let dir = temp_dir("file-path");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let service = make_service(&dir);
    let rt = runtime();
    let path = project_root.to_string_lossy().to_string();

    // 文件型连接：地址（路径）必须落 `database`（引擎 `build_connection_url` 与编辑回读
    // 都从这一列取路径），且要去掉 scheme / 多余前导斜杠。
    let mut i = input("local_sqlite", "sqlite", "sqlite:///C:/data/app.db");
    i.scope = ConnectionScope::GlobalAndProject;
    let pid = rt.block_on(service.save(&i, Some(&path))).expect("save gp");
    assert!(pid.starts_with("GP_conn_"), "{pid}");

    let list = rt.block_on(service.list()).expect("list");
    let g = list
        .iter()
        .find(|d| d.id.starts_with("G_conn_"))
        .expect("全局定义应存在");
    assert_eq!(
        g.database.as_deref(),
        Some("C:/data/app.db"),
        "全局侧路径应落 database"
    );
    assert!(blank(g.host.as_deref()), "文件型不应写主机：{:?}", g.host);

    let ds = rt
        .block_on(service.get_with_project(&pid, Some(&path)))
        .expect("get_with_project")
        .expect("项目侧连接应可读回");
    assert_eq!(ds.database.as_deref(), Some("C:/data/app.db"), "项目侧同样保留路径");
    assert!(blank(ds.host.as_deref()), "文件型不应写主机：{:?}", ds.host);
    // 连接 URL 只从 database 列还原：项目侧文件连接此前因缺路径会直接报“缺少数据库路径”。
    assert_eq!(
        connection::url::build_connection_url(&ds).expect("build url"),
        "sqlite://C:/data/app.db"
    );

    // 更新路径：项目侧同样要按文件型解析（不能落成 host/空 database）。
    let mut upd = i.clone();
    upd.url = "sqlite:///D:/data/app2.db".to_string();
    rt.block_on(service.update(&pid, &upd, Some(&path)))
        .expect("update");
    let ds2 = rt
        .block_on(service.get_with_project(&pid, Some(&path)))
        .expect("readback")
        .expect("项目侧连接应可读回");
    assert_eq!(ds2.database.as_deref(), Some("D:/data/app2.db"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_scope_rejects_non_project_root() {
    let dir = temp_dir("proj-bogus");
    let bogus = dir.join("not-a-project");
    std::fs::create_dir_all(&bogus).expect("mkdir bogus");
    let service = make_service(&dir);
    let rt = runtime();
    let path = bogus.to_string_lossy().to_string();

    // 写路径：非项目根（缺 .RSmeta）→ 明确报错，且**不在磁盘上造骨架**。
    let mut i = input("bogus_proj", "sqlite", "sqlite:///tmp/b.db");
    i.scope = ConnectionScope::Project;
    let err = rt
        .block_on(service.save(&i, Some(&path)))
        .expect_err("非项目根应被拒");
    assert!(err.to_string().contains(".RSmeta"), "{err}");
    assert!(!bogus.join(".RSmeta").exists(), "写路径不得创建项目骨架");

    // 读路径：降级为空，同样不建目录。
    assert!(rt
        .block_on(service.get_with_project("P_conn_probe", Some(&path)))
        .expect("读降级不报错")
        .is_none());
    assert!(!bogus.join(".RSmeta").exists(), "读路径不得创建项目骨架");

    // 同步 / 删除（项目侧）同样被拦。
    assert!(rt
        .block_on(service.sync_snapshot_from_global("GP_conn_probe_20260912", &path))
        .is_err());
    assert!(rt.block_on(service.delete("P_conn_probe", Some(&path))).is_err());
    assert!(!bogus.join(".RSmeta").exists(), "删除路径不得创建项目骨架");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_update_keeps_password_when_blank() {
    let dir = temp_dir("proj-pw");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let service = make_service(&dir);
    let rt = runtime();
    let path = project_root.to_string_lossy().to_string();

    let mut i = input("pw_keep", "postgres", "postgres://u:secret@127.0.0.1:5432/db");
    i.scope = ConnectionScope::Project;
    i.password = Some("secret".into());
    let id = rt
        .block_on(service.save(&i, Some(&path)))
        .expect("save project");

    let read = |rt: &tokio::runtime::Runtime| {
        rt.block_on(async {
            let store = open_project_store(&project_root).await;
            store.get_connection(&id).await
        })
        .expect("get")
        .expect("记录存在")
    };
    let before = read(&rt);
    assert!(before.password_encrypted.is_some(), "首次保存应有密文");

    // 编辑回读后密码框留空 → update 不带密码：项目库必须保留原密文
    // （否则一次编辑就把凭据清空，后续连接必失败）。
    let mut upd = input("pw_keep", "postgres", "postgres://u@127.0.0.1:5432/db2");
    upd.scope = ConnectionScope::Project;
    upd.password = None;
    rt.block_on(service.update(&id, &upd, Some(&path)))
        .expect("update without password");

    let after = read(&rt);
    assert_eq!(
        after.password_encrypted, before.password_encrypted,
        "空密码更新应保留原密文（与全局库 update 语义一致）"
    );
    assert_eq!(after.database.as_deref(), Some("db2"), "其他字段仍应更新");
    // 时间戳不得为空（旧实现会把项目侧 created_at / updated_at 写成空串 → 脏数据）。
    assert!(!after.created_at.is_empty(), "created_at 不应为空");
    assert!(!after.updated_at.is_empty(), "updated_at 不应为空");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn snapshot_sync_pulls_latest_global_definition() {
    use engine::persistence::id_prefix;

    let dir = temp_dir("snap-sync");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let service = make_service(&dir);
    let rt = runtime();
    let path = project_root.to_string_lossy().to_string();

    // 1) 建「全局+项目」连接 → 全局定义 G_ + 项目快照 GP_。
    let mut i = input("sync_gp", "postgres", "postgres://carol:pw1@127.0.0.1:5432/db1");
    i.scope = ConnectionScope::GlobalAndProject;
    i.password = Some("pw1".into());
    i.tags = Some(r#"["old"]"#.to_string());
    let pid = rt
        .block_on(service.save(&i, Some(&path)))
        .expect("save gp");
    let gid = id_prefix::source_global_id(&pid).expect("GP_ 可反查全局源 ID");

    // 标签权威表（connection_tags）在项目库侧：保存时已同步为快照的标签。
    let project_org = engine::persistence::ConnectionOrgStore::open_at(
        project_root.join(".RSmeta").join("project.db"),
        true,
    )
    .expect("open project org");
    assert_eq!(project_org.list_tags(&pid), vec!["old".to_string()]);

    // 2) 改全局定义（名称 / 端口 / 密码 / 标签）——快照是独立副本，此时不同步。
    let mut upd = input("sync_gp", "postgres", "postgres://carol:pw2@127.0.0.1:5433/db2");
    upd.password = Some("pw2".into());
    upd.tags = Some(r#"["new"]"#.to_string());
    rt.block_on(service.update(&gid, &upd, None)).expect("update global");

    let read = |rt: &tokio::runtime::Runtime| {
        rt.block_on(async {
            let store = open_project_store(&project_root).await;
            store.get_connection(&pid).await
        })
        .expect("get")
        .expect("快照存在")
    };
    let stale = read(&rt);
    assert_eq!(stale.port, Some(5432), "同步前快照仍是旧配置");
    assert_eq!(stale.database.as_deref(), Some("db1"));
    let stale_pw = stale.password_encrypted.clone();

    // 3) 显式同步 → 配置与凭据密文均来自全局定义；ID / 创建时间保留；
    //    标签权威表（`connection_tags`）也必须跟着快照一起更新（否则 tag 检索读到旧值）。
    assert_eq!(
        project_org.list_tags(&pid),
        vec!["old".to_string()],
        "快照独立：同步前项目侧标签不变"
    );
    rt.block_on(service.sync_snapshot_from_global(&pid, &path))
        .expect("sync snapshot");
    assert_eq!(
        project_org.list_tags(&pid),
        vec!["new".to_string()],
        "同步后项目侧标签权威表应更新"
    );
    let synced = read(&rt);
    assert_eq!(synced.port, Some(5433), "同步后应使用全局定义的新端口");
    assert_eq!(synced.database.as_deref(), Some("db2"));
    assert_eq!(synced.id, pid, "快照 ID 不应变化");
    assert!(
        synced.password_encrypted.is_some() && synced.password_encrypted != stale_pw,
        "凭据密文应随全局定义更新"
    );
    assert!(!synced.updated_at.is_empty(), "时间戳应写入");

    // 4) 错误路径：非快照 ID / 缺项目路径 / 全局定义已删除。
    let err = rt
        .block_on(service.sync_snapshot_from_global(&gid, &path))
        .expect_err("G_ 不是快照");
    assert!(err.to_string().contains("快照连接"), "{err}");
    let err = rt
        .block_on(service.sync_snapshot_from_global(&pid, "  "))
        .expect_err("缺项目路径应报错");
    assert!(err.to_string().contains("项目路径"), "{err}");
    rt.block_on(service.delete(&gid, None)).expect("删除全局定义");
    let err = rt
        .block_on(service.sync_snapshot_from_global(&pid, &path))
        .expect_err("全局定义不存在应报错");
    assert!(err.to_string().contains("不存在"), "{err}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn environment_policies_come_from_db_by_env_name() {
    let dir = temp_dir("env-policies");
    let service = make_service(&dir);
    let rt = runtime();

    // 种子环境（G_env_prod）在迁移里带 5 类策略（security/schema/performance/audit/ui）。
    let policies = rt
        .block_on(service.list_environment_policies_by_name("生产环境"))
        .expect("list policies");
    assert_eq!(policies.len(), 5, "应读到库里的 5 类策略");
    let types: Vec<&str> = policies.iter().map(|p| p.policy_type.as_str()).collect();
    assert!(types.contains(&"security"), "{types:?}");
    assert!(types.contains(&"performance"), "{types:?}");
    assert!(
        policies.iter().any(|p| p.policy_config.is_some()),
        "策略配置应来自库（非空）"
    );

    // 不存在的环境 → 空列表（不报错、不造默认值）。
    let none = rt
        .block_on(service.list_environment_policies_by_name("不存在的环境"))
        .expect("missing env");
    assert!(none.is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nav_runtime_resolves_project_connection_with_project_path() {
    let dir = temp_dir("nav-project");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let service = make_service(&dir);
    let rt = runtime();
    let path = project_root.to_string_lossy().to_string();

    let mut i = input("nav_proj", "sqlite", "sqlite:///tmp/nav.db");
    i.scope = ConnectionScope::Project;
    let id = rt
        .block_on(service.save(&i, Some(&path)))
        .expect("save project only");

    // 导航树「连接」入口：项目侧连接必须带项目根才解析得到（旧实现只查全局库 → 报“数据源不存在”）。
    let ds = rds_workbench::services::nav_runtime::load_entry_with(&service, &id, Some(&path))
        .expect("项目侧连接应可解析");
    assert_eq!(ds.name, "nav_proj");
    assert_eq!(ds.scope, ConnectionScope::Project);
    let err = rds_workbench::services::nav_runtime::load_entry_with(&service, &id, None)
        .expect_err("无项目根应明确报错");
    assert!(err.contains("数据源不存在"), "{err}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn service_save_is_visible_to_workspace_loader() {
    let dir = temp_dir("linkage");
    let service = make_service(&dir);
    let rt = runtime();

    let id = rt
        .block_on(service.save(
            &input("联动库", "mysql", "mysql://u:p@127.0.0.1:3306/link"),
            None,
        ))
        .expect("save");

    // 服务写 → 加载器读（同一目录、同一契约）。
    let (items, notice) = load_persisted_connections_from(&dir);
    assert!(notice.is_none(), "加载不应报错：{notice:?}");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, id);
    assert_eq!(items[0].name, "联动库");
    assert_eq!(items[0].driver, "mysql");
    assert_eq!(items[0].host.as_deref(), Some("127.0.0.1"));
    assert_eq!(items[0].port, Some(3306));
    assert_eq!(items[0].database.as_deref(), Some("link"));

    // 服务删除 → 加载器读回为空。
    rt.block_on(service.delete(&id, None)).expect("delete");
    let (items2, notice2) = load_persisted_connections_from(&dir);
    assert!(notice2.is_none(), "{notice2:?}");
    assert!(items2.is_empty(), "删除后加载器应看不到连接");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn tags_sync_and_delete_cleanup() {
    let dir = temp_dir("org-tags");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let service = make_service(&dir);
    let rt = runtime();
    let root_str = project_root.to_string_lossy().to_string();

    // 全局连接：保存时标签同步到连接组织存储（权威检索源）。
    let mut g = input("tagged_global", "sqlite", "sqlite:///tmp/tg.db");
    g.tags = Some(r#"["prod","core"]"#.to_string());
    let gid = rt.block_on(service.save(&g, None)).expect("save global");

    let global_org =
        engine::persistence::ConnectionOrgStore::open_at(dir.join("global.db"), false)
            .expect("open global org");
    assert_eq!(
        global_org.list_tags(&gid),
        vec!["core".to_string(), "prod".to_string()]
    );
    assert_eq!(global_org.list_connections_by_tag("prod"), vec![gid.clone()]);

    // 项目连接：标签落项目库；分组关系可加入。
    let mut p = input("tagged_project", "sqlite", "sqlite:///tmp/tp.db");
    p.scope = ConnectionScope::Project;
    p.tags = Some(r#"["dev"]"#.to_string());
    let pid = rt
        .block_on(service.save(&p, Some(&root_str)))
        .expect("save project");

    let project_org = engine::persistence::ConnectionOrgStore::open_at(
        project_root.join(".RSmeta").join("project.db"),
        true,
    )
    .expect("open project org");
    assert_eq!(project_org.list_tags(&pid), vec!["dev".to_string()]);
    project_org.create_group("g1", "alpha", None).expect("group");
    project_org.add_member("g1", &pid).expect("member");
    assert_eq!(project_org.list_group_members("g1"), vec![pid.clone()]);

    // 更新：标签改为新集合（覆盖式）。
    let mut upd = input("tagged_global", "sqlite", "sqlite:///tmp/tg.db");
    upd.tags = Some(r#"["archive"]"#.to_string());
    rt.block_on(service.update(&gid, &upd, None)).expect("update");
    assert_eq!(global_org.list_tags(&gid), vec!["archive".to_string()]);

    // 删除全局连接 → 标签清理。
    rt.block_on(service.delete(&gid, None)).expect("delete global");
    assert!(global_org.list_tags(&gid).is_empty());
    assert!(global_org.list_connections_by_tag("prod").is_empty());

    // 删除项目连接 → 标签 + 分组成员清理，分组保留。
    rt.block_on(service.delete(&pid, Some(&root_str)))
        .expect("delete project");
    assert!(project_org.list_tags(&pid).is_empty());
    assert!(project_org.list_group_members("g1").is_empty());
    assert_eq!(project_org.list_groups().len(), 1, "分组定义应保留");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn global_delete_cleans_project_group_membership() {
    // 回归点：全局连接加入「项目分组」后删除，项目库的成员关系不能残留（否则分组视图出现幻影成员）。
    let dir = temp_dir("global-del-org");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let service = make_service(&dir);
    let rt = runtime();
    let root_str = project_root.to_string_lossy().to_string();

    // 仅全局作用域的连接，加入项目分组：成员关系存项目库（连接本身在全局库）。
    let g = input("global_grouped", "sqlite", "sqlite:///tmp/gg.db");
    let gid = rt.block_on(service.save(&g, None)).expect("save global");
    let project_org = engine::persistence::ConnectionOrgStore::open_at(
        project_root.join(".RSmeta").join("project.db"),
        true,
    )
    .expect("open project org");
    project_org.create_group("g1", "alpha", None).expect("group");
    service
        .set_connection_groups(&gid, &["g1".to_string()], Some(&root_str))
        .expect("分组同步");
    assert_eq!(project_org.list_group_members("g1"), vec![gid.clone()]);

    // 删除全局连接（带项目根）→ 项目侧成员关系一并清理；分组定义保留。
    rt.block_on(service.delete(&gid, Some(&root_str)))
        .expect("delete global");
    assert!(
        project_org.list_group_members("g1").is_empty(),
        "项目侧不应残留幻影成员"
    );
    assert_eq!(project_org.list_groups().len(), 1, "分组定义应保留");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn group_sync_failure_is_reported_to_caller() {
    // 回归点（#28）：分组写出失败以前只打日志（UI 看到的是“保存成功”，勾选静默丢失）；
    // 现返回 Err，对话框据此把结果行降为 warning 级并带出原因。
    let dir = temp_dir("group-sync-err");
    let project_root = dir.join("proj-broken");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    // 项目库文件写垃圾字节：`open_at` 的 ensure_tables 会报「file is not a database」。
    std::fs::write(
        project_root.join(".RSmeta").join("project.db"),
        b"not a sqlite database",
    )
    .expect("write garbage db");
    let service = make_service(&dir);
    let root_str = project_root.to_string_lossy().to_string();

    let err = service
        .set_connection_groups("P_x", &["g1".to_string()], Some(&root_str))
        .expect_err("项目库不可用时应返回 Err（不再静默吞掉）");
    assert!(!err.to_string().is_empty(), "错误应带原因，供结果行展示");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn referenced_network_profile_reaches_connect_request() {
    // 回归点（审计 #20）：网络配置档案存在、连接也引用了，但连接时被静默忽略——
    // 根因有三层：① 入口恒传 `network_method: None`；② 解析器只认小写类型键，
    // 而 UI 写入的是 `Proxy` / `SSH`；③ 隧道守卫随临时服务实例释放。
    // 本用例覆盖 ①②（③ 由 `connection_service` 内嵌单测覆盖）。
    use connection::config::ConnectionMethod;

    let dir = temp_dir("net-profile");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let service = make_service(&dir);
    let rt = runtime();
    let root_str = project_root.to_string_lossy().to_string();

    // 1) 先让项目库走正常迁移建库，再写入网络配置档案（类型用 UI 实际会写的大写标签）。
    //    直接手建表会与迁移流水线冲突（`add_id_prefix_snapshot` 加 `origin` 列报重）。
    let net_id = {
        let pm = rt
            .block_on(ProjectDatabaseManager::open(&project_root, 4))
            .expect("open project db");
        rt.block_on(pm.create_project_network_config(
            Some("公司代理"),
            "Proxy",
            r#"{"host":"127.0.0.1","port":1080}"#,
        ))
        .expect("create network config")
        .id
    };

    // 2) 项目连接引用该档案（id 由引擎生成）。
    let mut i = input("proxied", "mysql", "mysql://u:p@10.0.0.9:3306/app");
    i.scope = ConnectionScope::Project;
    i.network_config_id = Some(net_id.clone());
    let pid = rt
        .block_on(service.save(&i, Some(&root_str)))
        .expect("save project");
    let ds = rt
        .block_on(service.get_with_project(&pid, Some(&root_str)))
        .expect("get")
        .expect("连接存在");

    // 3) 档案 → ConnectionMethod（含大写类型键归一 + 项目库路由）。
    let method = rt
        .block_on(rds_workbench::services::connection_service::resolve_network_method_with_project(
            ds.network_config_id.as_deref(),
            Some(&root_str),
        ))
        .expect("resolve network method");
    assert!(
        matches!(method, Some(ConnectionMethod::HttpProxy(_))),
        "大写类型键的档案应可解析：{method:?}"
    );

    // 4) 请求组装带出网络方式 + 作用域路由（此前 network_method 恒为 None）。
    let req = rds_workbench::services::nav_runtime::build_connect_request(
        &ds,
        Some(&root_str),
        method,
    )
    .expect("build connect request");
    assert_eq!(
        req.connection_type,
        engine::connection_manager::ConnectionType::Project,
        "P_ 连接应在项目库侧"
    );
    assert!(
        matches!(req.network_method, Some(ConnectionMethod::HttpProxy(_))),
        "网络方式必须随请求带出"
    );
    assert!(!req.url.is_empty(), "URL 应可还原");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_connection_reports_unknown_driver_without_io() {
    let dir = temp_dir("probe-err");
    let service = make_service(&dir);
    let rt = runtime();

    let result = rt.block_on(service.test(&input("bad", "no_such_driver", "x://y"), None));
    assert!(!result.success);
    assert!(result.message.contains("驱动") || result.message.contains("no_such_driver"), "{}", result.message);
    assert!(result.version.is_none(), "失败不应返回版本");

    let _ = std::fs::remove_dir_all(&dir);
}

/// 探测配置输入（集成测试接缝；无档案引用时为纯字段透传）。
fn probe_input<'a>(
    db_type: &'a str,
    url: &'a str,
    auth_config_id: Option<&'a str>,
) -> rds_workbench::services::connection_service::ProbeConfigInput<'a> {
    rds_workbench::services::connection_service::ProbeConfigInput {
        db_type,
        url,
        name: "probe_test",
        username: None,
        password: None,
        auth_config_id,
        auth_method: Some("password"),
        network_config_id: None,
        project_path: None,
        driver_properties: None,
        advanced_options: None,
    }
}

/// A1：测试连接的配置构建必须与真实连接同源（审计 A1）。
///
/// 此前 `DataSourceService::test` 只看 UI 输入（url/username/password/driver_properties），
/// 引用认证 / 网络档案时被静默忽略：测试结果与真实连接相反。
#[test]
fn probe_config_applies_referenced_auth_profile() {
    use engine::persistence::auth_store::AuthConfig;
    use rds_workbench::services::connection_service::ConnectionService;

    let dir = temp_dir("probe-config");
    let (global_db, _service) = make_service_with_db(&dir);
    let rt = runtime();

    let auth_id = "G_auth_probe_demo";
    rt.block_on(global_db.create_auth_config(&AuthConfig {
        id: auth_id.into(),
        name: Some("探测用认证".into()),
        auth_type: "password".into(),
        auth_data: r#"{"username":"alice","password":"s3cret"}"#.into(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: "2026-09-12T00:00:00Z".into(),
        updated_at: "2026-09-12T00:00:00Z".into(),
    }))
    .expect("create auth config");

    // 1) 引用存在的档案：凭据（解密后）注入 URL，并回填字段凭据（驱动校验 / 旧版 URL 构建需要）。
    let (config, guards, notes) = rt
        .block_on(ConnectionService::build_probe_config(
            Some(global_db),
            probe_input("postgres", "postgres://db.internal:5432/app", Some(auth_id)),
        ))
        .expect("build probe config");
    assert!(guards.is_empty(), "无网络档案不应建隧道");
    let url = config.url_override.as_deref().unwrap_or_default();
    assert!(url.contains("alice:s3cret@"), "档案凭据必须注入 URL：{url}");
    assert_eq!(config.username.as_deref(), Some("alice"));
    assert_eq!(config.password.as_deref(), Some("s3cret"));
    assert!(
        notes.iter().any(|n| n.contains("已应用认证档案凭据")),
        "说明应标明档案已生效：{notes:?}"
    );

    // 2) 引用不存在的档案：A2 严格模式——直接报错，不再静默回退到无凭据直连。
    let err = rt
        .block_on(ConnectionService::build_probe_config(
            Some(global_db),
            probe_input("postgres", "postgres://db.internal:5432/app", Some("G_auth_missing")),
        ))
        .err()
        .expect("档案缺失必须让配置构建失败");
    assert!(
        err.to_string().contains("引用的认证配置不存在"),
        "错误应指明档案缺失：{err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A2：引用计数与删除守卫（原型 §3.6「被引用的配置不可删除」）。
#[test]
fn manager_reference_count_and_delete_guard() {
    use engine::persistence::auth_store::AuthConfig;
    use rds_workbench::services::data_source_service::ReferenceField;

    let dir = temp_dir("refcount");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let (global_db, service) = make_service_with_db(&dir);
    let rt = runtime();
    let root_str = project_root.to_string_lossy().to_string();

    let auth_id = "G_auth_ref_demo";
    let net_id = "G_net_ref_demo";
    rt.block_on(global_db.create_auth_config(&AuthConfig {
        id: auth_id.into(),
        name: Some("演示认证".into()),
        auth_type: "password".into(),
        auth_data: r#"{"username":"u","password":"p"}"#.into(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: "2026-09-12T00:00:00Z".into(),
        updated_at: "2026-09-12T00:00:00Z".into(),
    }))
    .expect("create auth config");
    rt.block_on(global_db.create_network_config(&engine::persistence::network_store::NetworkConfig {
        id: net_id.into(),
        name: Some("演示网络".into()),
        network_type: "ssh".into(),
        config: r#"{"host":"h","username":"u","auth_type":"password","password":"p","remote_host":"db","remote_port":5432}"#.into(),
        auth_config_id: None,
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: "2026-09-12T00:00:00Z".into(),
        updated_at: "2026-09-12T00:00:00Z".into(),
    }))
    .expect("create network config");

    // 1) 无引用：计数为 0，守卫放行（空 id 不误报）。
    let count = rt.block_on(service.count_references(ReferenceField::AuthConfig, auth_id, None));
    assert_eq!(count.total(), 0);
    rt.block_on(service.ensure_no_references(
        ReferenceField::AuthConfig,
        auth_id,
        "演示认证",
        None,
    ))
    .expect("无引用应可删除");
    let empty = rt.block_on(service.count_references(ReferenceField::AuthConfig, "", Some(&root_str)));
    assert_eq!(empty.total(), 0, "空 id 不参与统计");

    // 2) 全局连接引用二者 → 计数与拦截消息都要说清范围。
    let mut g = input("ref_global", "postgres", "postgres://u:p@h:5432/db");
    g.auth_config_id = Some(auth_id.into());
    g.network_config_id = Some(net_id.into());
    rt.block_on(service.save(&g, None)).expect("save global");

    let auth = rt.block_on(service.count_references(ReferenceField::AuthConfig, auth_id, Some(&root_str)));
    assert_eq!((auth.global, auth.project), (1, 0));
    let err = rt
        .block_on(service.ensure_no_references(
            ReferenceField::AuthConfig,
            auth_id,
            "演示认证",
            Some(&root_str),
        ))
        .expect_err("被引用的认证配置不可删除");
    let msg = err.to_string();
    assert!(msg.contains("认证配置") && msg.contains("全局 1 条"), "{msg}");

    let net = rt.block_on(service.count_references(ReferenceField::NetworkConfig, net_id, None));
    assert_eq!(net.total(), 1, "网络配置走同一套计数");
    assert!(
        rt.block_on(service.ensure_no_references(
            ReferenceField::NetworkConfig,
            net_id,
            "演示网络",
            None
        ))
        .is_err(),
        "被引用的网络配置同样不可删除"
    );

    // 3) 项目连接引用（P_）→ 项目侧计数可见；未打开项目时项目侧不可见（已知局限）。
    let mut p = input("ref_project", "postgres", "postgres://u:p@h:5432/db");
    p.scope = ConnectionScope::Project;
    p.auth_config_id = Some(auth_id.into());
    rt.block_on(service.save(&p, Some(&root_str))).expect("save project");
    let both = rt.block_on(service.count_references(ReferenceField::AuthConfig, auth_id, Some(&root_str)));
    assert_eq!((both.global, both.project), (1, 1));
    let msg = rt
        .block_on(service.ensure_no_references(
            ReferenceField::AuthConfig,
            auth_id,
            "演示认证",
            Some(&root_str),
        ))
        .expect_err("项目引用同样拦截")
        .to_string();
    assert!(msg.contains("全局 1 条") && msg.contains("当前项目 1 条"), "{msg}");
    let no_project = rt.block_on(service.count_references(ReferenceField::AuthConfig, auth_id, None));
    assert_eq!(
        (no_project.global, no_project.project),
        (1, 0),
        "未打开项目时“当前项目”为 0（引用记录仍在库中）"
    );
    assert!(no_project.other.is_empty(), "此时名册里只有当前项目");

    // 4) #33：**跨项目引用**（已登记但未打开的项目）也必须计入，消息里列出项目名。
    let other_root = dir.join("proj_other");
    std::fs::create_dir_all(other_root.join(".RSmeta")).expect("mkdir other .RSmeta");
    let other_str = other_root.to_string_lossy().to_string();
    let mut other_conn = input("ref_other", "postgres", "postgres://u:p@h:5432/db");
    other_conn.scope = ConnectionScope::Project;
    other_conn.auth_config_id = Some(auth_id.into());
    rt.block_on(service.save(&other_conn, Some(&other_str)))
        .expect("save other project");
    // 登记进项目名册（`get_all_projects` 的来源；生产上由项目模块写入）
    rt.block_on(global_db.save_project_info(
        "proj_other",
        "另一项目",
        None,
        &other_str,
        "active",
        Some("2026-09-12T00:00:00Z"),
    ))
    .expect("register project");

    // 在当前项目上下文里看：其它项目 1 条（记项目名，每个项目只计一次）
    let cross = rt.block_on(service.count_references(
        ReferenceField::AuthConfig,
        auth_id,
        Some(&root_str),
    ));
    assert_eq!(cross.other_projects(), 1, "{cross:?}");
    assert_eq!(cross.other, vec!["另一项目".to_string()]);

    // 未打开任何项目时同样能看到（旧实现会漏掉）
    let cross_none = rt.block_on(service.count_references(ReferenceField::AuthConfig, auth_id, None));
    assert_eq!(cross_none.other_projects(), 1, "{cross_none:?}");

    // 拦截消息列出范围与项目名
    let msg = rt
        .block_on(service.ensure_no_references(
            ReferenceField::AuthConfig,
            auth_id,
            "演示认证",
            None,
        ))
        .expect_err("跨项目引用同样拦截")
        .to_string();
    assert!(msg.contains("全局 1 条"), "{msg}");
    assert!(
        msg.contains("其它项目 1 条") && msg.contains("另一项目"),
        "{msg}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A5：认证档案的编辑回填必须拿到**解密后**的 auth_data（列表接口是脱敏的）。
#[test]
fn auth_config_detail_by_name_decrypts_for_edit() {
    use engine::persistence::auth_store::AuthConfig;

    let dir = temp_dir("auth-detail");
    let (global_db, service) = make_service_with_db(&dir);
    let rt = runtime();

    let auth_id = "G_auth_detail_demo";
    rt.block_on(global_db.create_auth_config(&AuthConfig {
        id: auth_id.into(),
        name: Some("回填用认证".into()),
        auth_type: "password".into(),
        auth_data: r#"{"username":"alice","password":"s3cret"}"#.into(),
        origin: None,
        source_id: None,
        snapshot_at: None,
        created_at: "2026-09-12T00:00:00Z".into(),
        updated_at: "2026-09-12T00:00:00Z".into(),
    }))
    .expect("create auth config");

    // 列表接口在服务层已脱敏（auth_data 置空），否则管理器列表就是密码泄漏面。
    let listed = rt.block_on(service.list_auth_configs()).expect("list");
    let row = listed.iter().find(|a| a.id == auth_id).expect("listed");
    assert!(
        row.auth_data.is_empty(),
        "列表接口应脱敏（auth_data 置空）：{}",
        row.auth_data
    );

    // 编辑回填：解密后的 JSON（否则字段表单会把密文当成密码写回去）
    let (ty, data) = rt
        .block_on(service.auth_config_detail_by_name("回填用认证"))
        .expect("detail")
        .expect("档案存在");
    assert_eq!(ty, "password");
    let parsed: serde_json::Value = serde_json::from_str(&data).expect("auth_data 是 JSON");
    assert_eq!(parsed["username"], "alice");
    assert_eq!(parsed["password"], "s3cret", "回填必须是明文");

    // 不存在 → Ok(None)（不是错误）
    assert!(rt
        .block_on(service.auth_config_detail_by_name("不存在的档案"))
        .expect("detail")
        .is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

/// #34：网络档案内的 SSH / 代理密码必须加密入库，且列表接口脱敏、编辑回填解密。
#[test]
fn network_profile_secrets_are_encrypted_and_masked_in_list() {
    use connection::config::{ConnectionMethod, SshAuth};

    let dir = temp_dir("net-secret");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let (global_db, service) = make_service_with_db(&dir);
    let rt = runtime();
    let root_str = project_root.to_string_lossy().to_string();

    let ssh_json = r#"{"host":"jump.example.com","port":22,"username":"deploy","auth_type":"password","password":"jump-secret","remote_host":"db","remote_port":5432}"#;
    let net_id = {
        let nc = engine::persistence::network_store::NetworkConfig {
            id: "G_net_secret".into(),
            name: Some("加密网络".into()),
            network_type: "ssh".into(),
            config: ssh_json.into(),
            auth_config_id: None,
            origin: None,
            source_id: None,
            snapshot_at: None,
            created_at: "2026-09-12T00:00:00Z".into(),
            updated_at: "2026-09-12T00:00:00Z".into(),
        };
        rt.block_on(global_db.create_network_config(&nc))
            .expect("create network config");
        nc.id
    };

    // 1) 库里是密文（直接读原始列，绕过解密读路径）
    let raw: String = rt.block_on(async {
        let conn = global_db.sqlite_pool().acquire().await.expect("pool");
        let inner = conn.inner().expect("conn");
        inner
            .query_row(
                "SELECT config FROM network_configs WHERE id = ?1",
                rusqlite::params![net_id],
                |r| r.get(0),
            )
            .expect("row")
    });
    assert!(!raw.contains("jump-secret"), "落库不得为明文：{raw}");
    assert!(raw.contains("AES:"), "应为加密形式：{raw}");

    // 2) 服务层列表脱敏：config 置空（下拉 / 列表行不需要明文凭据）
    let list = rt.block_on(service.list_network_configs()).expect("list");
    let row = list.iter().find(|n| n.id == net_id).expect("listed");
    assert!(row.config.is_empty(), "列表应脱敏：{}", row.config);

    // 3) 编辑回填接口返回解密明文
    let (ty, cfg) = rt
        .block_on(service.network_config_detail_by_name("加密网络"))
        .expect("detail")
        .expect("档案存在");
    assert_eq!(ty, "ssh");
    let parsed: serde_json::Value = serde_json::from_str(&cfg).expect("json");
    assert_eq!(parsed["password"], "jump-secret", "回填必须是明文");

    // 4) 连接解析链路拿到明文（项目档案走项目库直读，不依赖全局单例）
    let p_net_id = {
        let pm = rt
            .block_on(ProjectDatabaseManager::open(&project_root, 4))
            .expect("open project db");
        rt.block_on(pm.create_project_network_config(Some("加密SSH"), "SSH", ssh_json))
            .expect("create project network config")
            .id
    };
    let method = rt
        .block_on(
            rds_workbench::services::connection_service::resolve_network_method_with_project(
                Some(&p_net_id),
                Some(&root_str),
            ),
        )
        .expect("resolve");
    match method {
        Some(ConnectionMethod::Ssh(ssh)) => match ssh.auth {
            SshAuth::Password { password } => assert_eq!(password, "jump-secret", "解析需拿到明文"),
            other => panic!("应为密码认证：{other:?}"),
        },
        other => panic!("应解析为 SSH：{other:?}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

/// A1：网络档案在测试连接时**真实建隧道**；建不起来就是测试失败（而非静默直连）。
#[test]
fn probe_config_applies_referenced_network_profile() {
    use rds_workbench::services::connection_service::{ConnectionService, ProbeConfigInput};

    let dir = temp_dir("probe-net");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(project_root.join(".RSmeta")).expect("mkdir .RSmeta");
    let (global_db, _service) = make_service_with_db(&dir);
    let rt = runtime();
    let root_str = project_root.to_string_lossy().to_string();

    // 不可达的 SSH 跳板：端口 1（连接会被立即拒绝，不依赖外部服务）。
    let net_id = {
        let pm = rt
            .block_on(ProjectDatabaseManager::open(&project_root, 4))
            .expect("open project db");
        rt.block_on(pm.create_project_network_config(
            Some("不可达 SSH"),
            "SSH",
            r#"{"host":"127.0.0.1","port":1,"username":"u","auth_type":"password","password":"p","remote_host":"db.internal","remote_port":5432}"#,
        ))
        .expect("create network config")
        .id
    };

    let outcome = rt.block_on(ConnectionService::build_probe_config(
        Some(global_db),
        ProbeConfigInput {
            db_type: "postgres",
            url: "postgres://db.internal:5432/app",
            name: "probe_net",
            username: None,
            password: None,
            auth_config_id: None,
            auth_method: None,
            network_config_id: Some(&net_id),
            project_path: Some(&root_str),
            driver_properties: None,
            advanced_options: None,
        },
    ));
    let err = match outcome {
        Ok((_, _, notes)) => panic!(
            "不可达的 SSH 档案必须让测试失败（证明真的建了隧道）；notes={notes:?}"
        ),
        Err(e) => e,
    };
    assert!(
        err.to_string().contains("网络档案应用失败"),
        "错误应说明网络档案：{err}"
    );

    // 类型未知的档案（`parse_network_config_json` 返回 None）同样必须失败：
    // 不能因为解析不了就退回直连（A2）。
    let unknown_id = {
        let pm = rt
            .block_on(ProjectDatabaseManager::open(&project_root, 4))
            .expect("open project db");
        rt.block_on(pm.create_project_network_config(Some("未知类型"), "carrier_pigeon", "{}"))
            .expect("create network config")
            .id
    };
    let outcome = rt.block_on(ConnectionService::build_probe_config(
        Some(global_db),
        ProbeConfigInput {
            db_type: "postgres",
            url: "postgres://db.internal:5432/app",
            name: "probe_net_unknown",
            username: None,
            password: None,
            auth_config_id: None,
            auth_method: None,
            network_config_id: Some(&unknown_id),
            project_path: Some(&root_str),
            driver_properties: None,
            advanced_options: None,
        },
    ));
    let err = match outcome {
        Ok((_, _, notes)) => {
            panic!("无法解析的网络档案必须失败（不能退回直连）；notes={notes:?}")
        }
        Err(e) => e,
    };
    assert!(
        err.to_string().contains("无法解析"),
        "错误应说明解析失败：{err}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
