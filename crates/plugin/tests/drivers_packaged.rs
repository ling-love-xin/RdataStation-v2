//! 驱动插件包（`tools/plugin-drivers/install.ps1` 组装安装的产物）宿主加载端到端验证。
//!
//! 前提：先跑 `tools\plugin-drivers\install.ps1`（构建壳 + 下载驱动 jar + 安装到
//! `.rds/plugins/<manifest-id>`）。本测试直接读**真实安装产物**，链路为：
//!
//! ```text
//! plugin.toml → Deployment::from_manifest（含 java/python 运行时三级解析，替换解释器路径）
//!   → register_sidecar_drivers → DriverRegistry（连接面板下拉里出现）
//!   → create(配置) → open_session（起 java/python 进程 → 握手 → describe）
//!   → query
//! ```
//!
//! 无证书验证：oracle 包用 H2 内存库（`jdbc:h2:mem:`）、postgres-py 包只验部署与注册
//! （真连 PG 需要用户环境里的 psycopg，壳的连接失败文案由 `tools/rds-dbapi-shell` 的
//! smoke 覆盖）——都不依赖真库。
//!
//! 解释器不在三级查找范围内（java / python 缺失）时跳过，不误报为失败。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use engine::driver::registry::{DriverConnectionConfig, DriverRegistry};
use rds_plugin::manifest::{ManifestParser, PluginManifest};
use rds_plugin::sidecar::factory::register_sidecar_drivers;
use rds_plugin::sidecar::supervisor::{Deployment, SidecarSupervisor};

/// 仓库根（`crates/plugin` 的 `CARGO_MANIFEST_DIR` 上行两级）。
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("仓库根应当存在")
        .to_path_buf()
}

/// 读真实安装的插件包清单；包没装（没跑 install.ps1）返回 `None`（测试跳过）。
fn installed_manifest(package_id: &str) -> Option<(PathBuf, PluginManifest)> {
    let dir = repo_root().join(".rds/plugins").join(package_id);
    let manifest_path = dir.join("plugin.toml");
    if !manifest_path.exists() {
        return None;
    }
    let manifest = ManifestParser::parse(&manifest_path).ok()?;
    Some((dir, manifest))
}

/// 等「没有活会话」（会话交还由后台任务做，得给它机会拿到锁）。
async fn wait_until_no_sessions(supervisor: &Arc<Mutex<SidecarSupervisor>>) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        {
            let mut supervisor = supervisor.lock().await;
            supervisor.drain_events(Instant::now()).await;
            if supervisor.registry().session_count() == 0 {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

/// oracle-jdbc 包：部署（java 解释器替换）→ 注册 → 用 H2 连真链路（无证书验证）。
#[tokio::test]
async fn oracle_package_deploys_registers_and_queries_against_h2() {
    let Some((plugin_dir, manifest)) = installed_manifest("rds.driver.oracle") else {
        eprintln!("oracle 插件包未安装（先跑 tools\\plugin-drivers\\install.ps1），跳过本用例");
        return;
    };

    // 部署：command.program 应被替换为解析出的 java 解释器，args 保持 ["-jar", 壳路径]
    let deployment = Deployment::from_manifest(&manifest, &plugin_dir)
        .expect("oracle 包应当可部署")
        .expect("oracle 包应当有子进程形态");
    assert!(
        deployment
            .command
            .program
            .to_string_lossy()
            .to_lowercase()
            .contains("java"),
        "program 应为解析出的 java：{}",
        deployment.command.program.display()
    );
    assert_eq!(deployment.command.args[0], "-jar", "interpreter_args 应保留");
    assert!(
        deployment.command.args[1].ends_with("rds-jdbc-shell.jar"),
        "{}",
        deployment.command.args[1]
    );
    assert!(
        deployment.runtime_note.is_some(),
        "java 运行时解析应有来源说明"
    );

    // 注册：驱动 id = oracle，URL 模板来自清单（透传到 descriptor）
    let supervisor = Arc::new(Mutex::new(SidecarSupervisor::new()));
    let registered = register_sidecar_drivers(Arc::clone(&supervisor), &manifest, &plugin_dir)
        .await
        .expect("注册应当成功");
    assert!(registered.contains(&"oracle".to_string()), "{registered:?}");

    let factory = DriverRegistry::get("oracle").expect("注册表里应当有 oracle");
    assert_eq!(
        factory.descriptor().url_template.as_deref(),
        Some("jdbc:oracle:thin:@//{host}:{port}/{database}")
    );

    // 连接：H2 内存库（无 Oracle 服务器也能走协议/进程/会话/查询全链路）
    let mut config = DriverConnectionConfig::new("oracle");
    config.name = Some("H2 无证书验证".to_string());
    config.url_override = Some("jdbc:h2:mem:rds_pkg_test".to_string());
    let database = factory.create(config).await.expect("H2 连接应当成功");

    let result = database
        .query("select 42 as answer")
        .await
        .expect("查询应当成功");
    assert_eq!(result.total_rows(), 1, "H2 应返回一行");

    drop(database);
    assert!(
        wait_until_no_sessions(&supervisor).await,
        "句柄丢掉之后会话应当被交还"
    );
    supervisor
        .lock()
        .await
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}

/// postgres-py 包：部署（python 解释器替换）→ 注册（驱动 id = postgresql）。
///
/// 真连接需要用户环境里的 psycopg（壳按 DB-API 2.0 适配）；连接失败文案与 sqlite
/// 全链路由 `tools/rds-dbapi-shell/smoke.ps1` 覆盖，这里钉住「宿主能部署 + 下拉能出现」。
#[tokio::test]
async fn postgres_py_package_deploys_and_registers() {
    let Some((plugin_dir, manifest)) = installed_manifest("rds.driver.postgres-py") else {
        eprintln!("postgres-py 插件包未安装（先跑 tools\\plugin-drivers\\install.ps1），跳过本用例");
        return;
    };

    let deployment = Deployment::from_manifest(&manifest, &plugin_dir)
        .expect("postgres-py 包应当可部署")
        .expect("postgres-py 包应当有子进程形态");
    let program = deployment.command.program.to_string_lossy().to_lowercase();
    assert!(
        program.contains("python"),
        "program 应为解析出的 python：{}",
        deployment.command.program.display()
    );
    assert!(
        deployment
            .command
            .args
            .last()
            .is_some_and(|a| a.ends_with("rds_dbapi_shell.py")),
        "args 应保留脚本路径：{:?}",
        deployment.command.args
    );
    assert!(
        deployment.runtime_note.is_some(),
        "python 运行时解析应有来源说明"
    );

    let supervisor = Arc::new(Mutex::new(SidecarSupervisor::new()));
    let registered = register_sidecar_drivers(Arc::clone(&supervisor), &manifest, &plugin_dir)
        .await
        .expect("注册应当成功");
    assert!(
        registered.contains(&"postgresql".to_string()),
        "{registered:?}"
    );

    let factory = DriverRegistry::get("postgresql").expect("注册表里应当有 postgresql");
    assert_eq!(
        factory.descriptor().url_template.as_deref(),
        Some("postgresql://{user}:{password}@{host}:{port}/{database}")
    );

    supervisor
        .lock()
        .await
        .shutdown_all(Instant::now(), Duration::from_secs(10))
        .await;
}
