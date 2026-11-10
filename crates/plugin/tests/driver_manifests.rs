//! `tools/plugin-drivers/` 下四个驱动插件包的清单契约验证（不需要真实数据库）。
//!
//! 直接走宿主自己的 `ManifestParser::parse`：解析 + `validate()` + 引擎版本闸。
//! 目的：改 `plugin.toml` 时（尤其是 Rust 轨新增的两个包）立刻发现字段写错、
//! 平台/形态不合法、驱动 id 与内置 id 冲突之类的问题，而不是装到 `.rds/plugins` 里才炸。

use std::path::{Path, PathBuf};

use rds_plugin::manifest::{BackendKind, BackendRuntime};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/plugin 的上级两级应当是仓库根")
        .to_path_buf()
}

fn plugin_dir(name: &str) -> PathBuf {
    repo_root().join("tools").join("plugin-drivers").join(name)
}

#[test]
fn all_packaged_driver_manifests_parse_and_validate() {
    let packages = [
        ("oracle-jdbc", "rds.driver.oracle", "oracle"),
        ("postgres-py", "rds.driver.postgres-py", "postgresql"),
        ("oracle-rust-plugin", "rds.driver.oracle-rust", "oracle"),
        ("mssql-rust-plugin", "rds.driver.mssql-rust", "mssql"),
    ];

    for (dir, plugin_id, driver_id) in packages {
        let manifest_path = plugin_dir(dir).join("plugin.toml");
        let manifest = rds_plugin::manifest::ManifestParser::parse(&manifest_path)
            .unwrap_or_else(|e| panic!("{} 清单不合法：{e}", manifest_path.display()));

        assert_eq!(manifest.plugin.id, plugin_id, "{dir} 的 plugin.id 口径");
        let drivers = &manifest.contributes.drivers;
        assert_eq!(drivers.len(), 1, "{dir} 应当只贡献一个驱动");
        assert_eq!(drivers[0].id, driver_id, "{dir} 的驱动 id 口径");
        assert!(
            !drivers[0].display_name.is_empty(),
            "{dir} 的驱动要有显示名"
        );
    }
}

#[test]
fn rust_driver_packages_declare_sidecar_rust_backend() {
    for dir in ["oracle-rust-plugin", "mssql-rust-plugin"] {
        let manifest_path = plugin_dir(dir).join("plugin.toml");
        let manifest = rds_plugin::manifest::ManifestParser::parse(&manifest_path)
            .unwrap_or_else(|e| panic!("{} 清单不合法：{e}", manifest_path.display()));
        let backend = manifest
            .backend
            .as_ref()
            .unwrap_or_else(|| panic!("{dir} 必须声明 [backend]"));

        assert_eq!(backend.kind, BackendKind::Sidecar, "{dir} 的形态");
        assert_eq!(
            backend.runtime,
            Some(BackendRuntime::Rust),
            "{dir} 的运行时"
        );
        assert_eq!(backend.executable.as_deref(), Some("bin/agent"));
        assert_eq!(backend.protocol.as_deref(), Some("rds-driver/1"));
        assert_eq!(backend.max_instances, 1);
        assert!(
            backend.supports_current_platform(),
            "{dir} 应当声明当前平台"
        );
        assert!(backend.kind.is_subprocess(), "{dir} 的 sidecar 要进进程池");

        // 二进制已构建时，顺手验一遍宿主的入口解析（bin/<platform>/agent 优先）。
        // 干净检出没有 bin/，这条只做条件断言，不把产物是否存在绑进清单契约。
        let expected = plugin_dir(dir)
            .join("bin")
            .join("win-x64")
            .join("agent.exe");
        if expected.is_file() {
            let resolved = rds_plugin::layout::resolve_executable(
                &plugin_dir(dir),
                "backend.executable",
                "bin/agent",
            )
            .unwrap_or_else(|e| panic!("{dir} 的入口解析失败：{e}"));
            assert_eq!(resolved, expected, "{dir} 应当解析到 bin/win-x64/agent.exe");
        }
    }
}
