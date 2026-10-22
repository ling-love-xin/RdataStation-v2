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

/// 用户自定驱动 jar（`backend.driver_jars`）全链路：清单 → 部署 env 注入 →
/// 进程带 `RDS_DRIVER_JARS` 起壳 → 壳从外部目录加载 h2 → 真连查询。
///
/// 布置：插件目录 `lib/` 只放壳 jar（同目录扫不到任何驱动 jar），h2 单独放
/// 外部目录、仅由 driver_jars 指向——能连 H2 即证明外部加载链路完整。
#[tokio::test]
async fn driver_jars_inject_env_and_load_external_driver_jar() {
    // 壳与 h2 必须真实存在（tools/rds-jdbc-shell 已构建，install.ps1 也跑过）
    let shell_src = repo_root().join("tools/rds-jdbc-shell/lib/rds-jdbc-shell.jar");
    let h2_src = repo_root().join("tools/rds-jdbc-shell/lib/h2-2.5.252.jar");
    if !shell_src.exists() || !h2_src.exists() {
        eprintln!("rds-jdbc-shell 未构建（先跑 tools\\rds-jdbc-shell\\build.ps1），跳过本用例");
        return;
    }

    let tmp = std::env::temp_dir().join(format!("rds_extjar_it_{}", std::process::id()));
    let plugin_dir = tmp.join("plugin");
    let ext_dir = tmp.join("external");
    let lib = plugin_dir.join("lib");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).expect("清理旧临时目录");
    }
    std::fs::create_dir_all(&lib).expect("建 lib 目录");
    std::fs::create_dir_all(&ext_dir).expect("建 external 目录");
    std::fs::copy(&shell_src, lib.join("rds-jdbc-shell.jar")).expect("复制壳 jar");
    std::fs::copy(&h2_src, ext_dir.join("h2-2.5.252.jar")).expect("复制 h2 jar");

    // 路径统一正斜杠（TOML 里反斜杠是转义符）
    let ext_str = ext_dir.to_string_lossy().replace('\\', "/");
    let manifest_text = format!(
        r#"
[plugin]
id = "com.example.extjar"
name = "外部 jar 验证"
version = "0.1.0"
publisher = "example"

[plugin.engines]
rdatastation = ">=0.1"

[backend]
kind = "script"
executable = "lib/rds-jdbc-shell.jar"
runtime = "java"
interpreter_args = ["-jar"]
driver_jars = ["{ext_str}"]

[capabilities.driver]

[[contributes.drivers]]
id = "jdbc"
display_name = "外部 jar JDBC"
default_port = 1521
"#
    );
    std::fs::write(plugin_dir.join("plugin.toml"), manifest_text).expect("写清单");

    let manifest = ManifestParser::parse(&plugin_dir.join("plugin.toml"))
        .expect("清单应当可解析");
    let deployment = Deployment::from_manifest(&manifest, &plugin_dir)
        .expect("部署应当成功")
        .expect("应当有子进程形态");
    assert_eq!(
        deployment.env,
        vec![("RDS_DRIVER_JARS".to_string(), ext_str.clone())],
        "driver_jars 应经环境变量注入"
    );

    let supervisor = Arc::new(Mutex::new(SidecarSupervisor::new()));
    let registered = register_sidecar_drivers(Arc::clone(&supervisor), &manifest, &plugin_dir)
        .await
        .expect("注册应当成功");
    assert!(registered.contains(&"jdbc".to_string()), "{registered:?}");

    let factory = DriverRegistry::get("jdbc").expect("注册表里应当有 jdbc");
    let mut config = DriverConnectionConfig::new("jdbc");
    config.name = Some("外部 h2".to_string());
    config.url_override = Some("jdbc:h2:mem:ext_jar_it".to_string());
    let database = factory.create(config).await.expect("外部 h2 连接应当成功");
    let result = database
        .query("select 42 as answer")
        .await
        .expect("查询应当成功");
    assert_eq!(result.total_rows(), 1, "应返回一行");
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
    std::fs::remove_dir_all(&tmp).expect("清理临时目录");
}
