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
    let rt = runtime();
    let manager = rt
        .block_on(GlobalDatabaseManager::new(
            dir.join("global.db"),
            dir.join("analytics.duckdb"),
            2,
        ))
        .expect("init global db");
    let manager: &'static GlobalDatabaseManager = Box::leak(Box::new(manager));
    DataSourceService::new(manager).with_analysis_db(dir.join("secret-target.duckdb"))
}

fn input(name: &str, db_type: &str, url: &str) -> DataSourceSaveInput {
    DataSourceSaveInput::new(name, db_type, url)
}

/// 打开项目侧连接仓库（与 `DataSourceService` 内部 `open_project_store` 同构）。
async fn open_project_store(project_root: &Path) -> ProjectConnectionStore {
    let db = ProjectDatabaseManager::open(project_root, 4)
        .await
        .expect("open project db");
    ProjectConnectionStore::new(Arc::new(db))
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
        .block_on(service.get(&b_id))
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
    std::fs::create_dir_all(&project_root).expect("mkdir project");
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
    assert!(rt.block_on(service.get(&pid)).unwrap().is_none());

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
    std::fs::create_dir_all(&project_root).expect("mkdir project");
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
    assert!(rt.block_on(service.get(&id)).unwrap().is_none());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn nav_runtime_resolves_project_connection_with_project_path() {
    let dir = temp_dir("nav-project");
    let project_root = dir.join("proj");
    std::fs::create_dir_all(&project_root).expect("mkdir project");
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
    std::fs::create_dir_all(&project_root).expect("mkdir");
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
        project_root.join(".RSMETA").join("project.db"),
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
fn test_connection_reports_unknown_driver_without_io() {
    let dir = temp_dir("probe-err");
    let service = make_service(&dir);
    let rt = runtime();

    let result = rt.block_on(service.test(&input("bad", "no_such_driver", "x://y")));
    assert!(!result.success);
    assert!(result.message.contains("驱动") || result.message.contains("no_such_driver"), "{}", result.message);
    assert!(result.version.is_none(), "失败不应返回版本");

    let _ = std::fs::remove_dir_all(&dir);
}
