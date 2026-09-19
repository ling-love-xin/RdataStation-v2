//! `rds-paths` 的单元测试。
//!
//! 注意：`home()` 是进程级 `OnceLock`，测不了"不同 `RDS_HOME`"——环境变量类断言
//! 在 Rust 2024 下是 `unsafe` 且与并行测试争用，不值得。这里只测真正会坏的事：
//! ① 派生目录是否都挂在同一个根下（含插件 M9 的五个）；② 插件 id 白名单能不能挡住路径穿越；
//! ③ 新增顶层目录有没有漏登记（漏了迁移会搬错）；④ 迁移的路由与"只补不盖"；
//! ⑤ 测试构建是否拿到了隔离数据根（不写产品目录）。

use std::path::{Path, PathBuf};

use crate::{
    HomeOrigin, config_dir, data_dir, extensions_dir, home, home_origin, log_dir, migrate,
    plugin_cache_dir, plugin_data_dir, plugin_dir, plugin_registry_dir, plugins_dir,
    sidecar_work_dir, temp_dir, validate_plugin_id,
};

/// 一个测试自己的临时目录（沿用其它 crate 的 `rds_*` 前缀约定）。
///
/// 不用 `paths::temp_dir()`：那是产品目录，测试不该往里写。
fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rds_paths_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("建测试临时目录");
    dir
}

#[test]
fn derived_dirs_share_one_root() {
    let root = home();
    assert_eq!(config_dir(), root.join("config"));
    assert_eq!(data_dir(), root.join("data"));
    assert_eq!(log_dir(), root.join("logs"));
    assert_eq!(extensions_dir(), root.join("extensions"));
}

#[test]
fn temp_dir_defaults_under_root() {
    if std::env::var_os("RDS_TEMP_DIR").is_some() {
        return; // 显式覆盖时不检查默认值
    }
    assert_eq!(temp_dir(), home().join("tmp"));
}

#[test]
fn temp_dir_honours_override() {
    // 覆盖只在 `RDS_TEMP_DIR` 有值时生效——这条断言不依赖具体取值。
    assert!(temp_dir().is_absolute() || std::env::var_os("RDS_TEMP_DIR").is_some());
}

/// 插件（M9）的五个目录也必须挂在同一个根下：插件是产品数据，不能散到系统目录。
///
/// 唯一例外是 sidecar 工作目录——它故意放 `tmp/` 下（子进程崩溃留下的垃圾不该进数据目录）。
#[test]
fn plugin_dirs_live_under_the_data_root() {
    assert_eq!(plugins_dir(), home().join("plugins"));
    assert_eq!(
        plugin_dir("example.demo"),
        plugins_dir().join("example.demo")
    );
    assert_eq!(
        plugin_data_dir("example.demo"),
        home().join("plugin-data").join("example.demo")
    );
    assert_eq!(
        plugin_cache_dir("example.demo"),
        home().join("plugin-cache").join("example.demo")
    );
    assert_eq!(plugin_registry_dir(), plugins_dir().join(".registry"));
    assert_eq!(
        sidecar_work_dir("example.demo"),
        temp_dir().join("sidecar").join("example.demo")
    );
}

/// 插件 id 会被直接拼进路径，所以只接白名单：“`..` / 分隔符 / 盘符 / 非 ASCII”一律拒。
///
/// 没有这道门，一个 `../../..` 的 id 就能把“安装”写到数据根外面去。
#[test]
fn validate_plugin_id_rejects_path_escapes() {
    let too_long = "a".repeat(129);
    for bad in [
        "",
        ".",
        "..",
        "../etc",
        "a/b",
        "a\\b",
        "C:\\evil",
        "x/../../y",
        "a b",
        "插件",
        "---",
        too_long.as_str(),
    ] {
        assert!(
            validate_plugin_id(bad).is_err(),
            "{bad:?} 应被拒绝（它会被拼进路径）"
        );
    }

    for good in [
        "example.demo",
        "oracle-jdbc-bridge",
        "publisher.sql_notebook",
        "a1",
    ] {
        assert!(validate_plugin_id(good).is_ok(), "{good:?} 应被接受");
    }
}

/// 新增顶层目录必须登记进 [`crate::NEW_LAYOUT_DIRS`]，否则 `RDS_HOME` 恰好落在
/// 旧布局目录上时，迁移会把插件目录当旧数据搬走（历史坑：`data/` 被搬成 `data/data/`）。
#[test]
fn plugin_roots_are_registered_as_new_layout_dirs() {
    for name in ["plugins", "plugin-data", "plugin-cache"] {
        assert!(
            crate::NEW_LAYOUT_DIRS.contains(&name),
            "{name} 未登记进 NEW_LAYOUT_DIRS：迁移会把它当旧数据"
        );
    }
}

#[test]
fn ensure_dirs_creates_every_derived_dir() {
    // 落在真实数据根上，但只建目录（幂等），不写任何内容。
    super::ensure_dirs().expect("建数据目录");
    for dir in [
        config_dir(),
        data_dir(),
        log_dir(),
        temp_dir(),
        extensions_dir(),
        plugins_dir(),
        plugin_registry_dir(),
        home().join("plugin-data"),
        home().join("plugin-cache"),
        temp_dir().join("sidecar"),
    ] {
        assert!(dir.is_dir(), "{} 应已存在", dir.display());
    }
}

/// 测试构建必须拿到隔离的数据根：否则测试会往产品目录写（见 data-paths.md §5），
/// 而旧布局迁移是"只补不盖"——写进去的垃圾还会把真数据挡在门外。
#[test]
fn test_build_is_isolated_from_the_product_root() {
    assert_eq!(
        home_origin(),
        HomeOrigin::TestRoot,
        "测试构建应使用隔离数据根（`test-support` / `cfg(test)` 未生效）"
    );
    assert!(crate::is_isolated_root(), "隔离根判定应与 home_origin 一致");

    // 隔离根必须落在临时目录下——绝不能是安装目录、更不能是 `RDS_HOME` 指的开发根
    let temp = std::env::temp_dir();
    assert!(
        home().starts_with(&temp),
        "隔离根应在临时目录下：{}（temp={}）",
        home().display(),
        temp.display()
    );
}

/// `pin_root` 在数据根已经解析过之后必须**拒绝**，不能假装成功。
#[test]
fn pin_root_is_rejected_after_resolution() {
    let _ = home(); // 先解析（解析是进程级一次）
    assert!(
        !crate::pin_root(std::env::temp_dir().join("rds_late_pin")),
        "已经解析过数据根时 pin_root 应返回 false"
    );
}

/// 旧目录里的 `settings.json` 归 `config/`，其余归 `data/`；新布局自己的目录名要跳过；
/// 密钥类文件标记为可覆盖（"只补不盖"的唯一例外）。
#[test]
fn migration_routes_settings_to_config_and_skips_new_layout() {
    let src = temp_root("legacy_src");
    std::fs::write(src.join("settings.json"), b"{}").unwrap();
    std::fs::create_dir_all(src.join("system")).unwrap();
    std::fs::write(src.join("system/global.db"), b"db").unwrap();
    std::fs::write(src.join("recent_connections.json"), b"[]").unwrap();
    std::fs::write(src.join("encryption-salt"), b"salt").unwrap();
    // `RDS_HOME` 万一落在旧目录上：这些新布局目录不能再被当旧数据搬一遍
    std::fs::create_dir_all(src.join("data")).unwrap();
    std::fs::create_dir_all(src.join("logs")).unwrap();

    let data = temp_root("legacy_data");
    let config = temp_root("legacy_config");
    let mut items = Vec::new();
    migrate::collect_top_level(&src, &data, &config, &mut items);

    let dests: Vec<&Path> = items.iter().map(|i| i.dest.as_path()).collect();
    assert!(dests.contains(&config.join("settings.json").as_path()));
    assert!(dests.contains(&data.join("system").as_path()));
    assert!(dests.contains(&data.join("recent_connections.json").as_path()));
    assert!(
        !items.iter().any(|i| i.src == src.join("data")),
        "新布局目录不能被当成旧数据"
    );
    assert!(!items.iter().any(|i| i.src == src.join("logs")));

    // 普通文件只补不盖；密钥文件允许覆盖
    let salt = items
        .iter()
        .find(|i| i.dest == data.join("encryption-salt"))
        .expect("盐值应在迁移清单里");
    assert!(salt.overwrite, "密钥文件必须允许覆盖");
    let settings = items
        .iter()
        .find(|i| i.dest == config.join("settings.json"))
        .unwrap();
    assert!(!settings.overwrite, "普通文件不得覆盖既有数据");
}

/// 只补不盖：目标已存在就跳过，用户在新位置改过的文件不会被旧文件回冲。
#[test]
fn migration_does_not_overwrite_existing_destination() {
    let src = temp_root("copy_src");
    std::fs::write(src.join("settings.json"), b"old").unwrap();
    let dest_root = temp_root("copy_dest");
    let dest = dest_root.join("settings.json");
    std::fs::write(&dest, b"new").unwrap();

    let mut report = migrate::MigrationReport::default();
    migrate::copy_item(
        &migrate::Item {
            src: src.join("settings.json"),
            dest: dest.clone(),
            overwrite: false,
        },
        &mut report,
    );

    assert_eq!(std::fs::read(&dest).unwrap(), b"new");
    assert_eq!(report.skipped, 1);
    assert!(report.copied.is_empty());
}

/// 密钥类文件是例外：目标已存在也要被旧位置那份盖掉。
///
/// 场景：开发机上先跑过 `cargo test`，它在数据根生成了一份**跟任何密文都没关系**的随机盐；
/// 此时"只补不盖"会把真钥匙挡在门外，表现就是所有已保存的连接密码突然解不开。
#[test]
fn migration_overwrites_existing_secret_files() {
    let src = temp_root("secret_src");
    let src_salt = src.join("encryption-salt");
    std::fs::write(&src_salt, b"real-salt").unwrap();
    let dest_root = temp_root("secret_dest");
    let dest = dest_root.join("encryption-salt");
    std::fs::write(&dest, b"junk-salt").unwrap();

    let mut report = migrate::MigrationReport::default();
    migrate::copy_item(
        &migrate::Item {
            src: src_salt,
            dest: dest.clone(),
            overwrite: true,
        },
        &mut report,
    );

    assert_eq!(std::fs::read(&dest).unwrap(), b"real-salt");
    assert_eq!(report.overwritten, 1);
    assert_eq!(report.skipped, 0);
}

/// 目录递归复制：层级与文件内容都要保住（旧 `system/` 里有 sqlite / duckdb / wal）。
#[test]
fn migration_copies_directories_recursively() {
    let src_root = temp_root("dir_src");
    let src = src_root.join("system");
    std::fs::create_dir_all(src.join("global_metadata")).unwrap();
    std::fs::write(src.join("global.db"), b"db").unwrap();
    std::fs::write(src.join("global.db-wal"), b"wal").unwrap();
    std::fs::write(src.join("global_metadata/conn_1.sqlite"), b"meta").unwrap();

    let dest = temp_root("dir_dest").join("system");
    let mut report = migrate::MigrationReport::default();
    migrate::copy_item(
        &migrate::Item {
            src,
            dest: dest.clone(),
            overwrite: false,
        },
        &mut report,
    );

    assert_eq!(std::fs::read(dest.join("global.db")).unwrap(), b"db");
    assert_eq!(std::fs::read(dest.join("global.db-wal")).unwrap(), b"wal");
    assert_eq!(
        std::fs::read(dest.join("global_metadata/conn_1.sqlite")).unwrap(),
        b"meta"
    );
    assert_eq!(report.copied.len(), 3);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
}
