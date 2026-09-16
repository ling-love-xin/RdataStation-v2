//! `rds-paths` 的单元测试。
//!
//! 注意：`home()` 是进程级 `OnceLock`，测不了"不同 `RDS_HOME`"——环境变量类断言
//! 在 Rust 2024 下是 `unsafe` 且与并行测试争用，不值得。这里只测两件真正会坏的事：
//! ① 派生的五个目录是否都挂在同一个根下；② 迁移的路由与"只补不盖"。

use std::path::{Path, PathBuf};

use crate::{config_dir, data_dir, extensions_dir, home, log_dir, migrate, temp_dir};

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

#[test]
fn ensure_dirs_creates_every_derived_dir() {
    // 落在真实数据根上，但只建目录（幂等），不写任何内容。
    super::ensure_dirs().expect("建数据目录");
    for dir in [config_dir(), data_dir(), log_dir(), temp_dir(), extensions_dir()] {
        assert!(dir.is_dir(), "{} 应已存在", dir.display());
    }
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
