//! 动态链接 DuckDB 的**运行时**配套（构建脚本）。
//!
//! # 为什么需要
//!
//! 内核改为动态链接后（见 `docs/architecture/dependencies/duckdb-linking.md`），Windows 的
//! 隐式导入要求 `duckdb.dll` 能被加载器找到——搜索顺序是「exe 所在目录 → 系统目录 →
//! Windows 目录 → 当前目录 → PATH」。而本仓的可执行文件分两处落：
//! `target/<profile>/deps`（测试二进制）与 `target/<profile>`（`cargo run` 的应用），
//! 两处都放一份最省事，开发者也不必配 PATH。
//!
//! 与 `libduckdb-sys` 自带 `DUCKDB_DOWNLOAD_LIB` 路径的差别：那边把库下载进 `target/`
//! （`cargo clean` 即丢），我们把库固定放在仓库内 `third_party/duckdb/<版本>/`，
//! 构建期不联网、清理 `target` 不影响它。
//!
//! # 为什么落在 engine
//!
//! `duckdb` 依赖由本 crate 声明（其余 crate 经 workspace 别名继承），凡是链接 DuckDB 的
//! 目标都会跑到本脚本，因此这里是唯一省事的挂载点。
//!
//! # 何时什么都不做
//!
//! - `DUCKDB_LIB_DIR` 未设置（静态链接 / 别的取库方式）：**给一条可执行的提示**，不让
//!   链接器报「找不到 duckdb.lib」那种看不出该怎么办的错；
//! - 非 Windows：动态库靠 rpath / `LD_LIBRARY_PATH` 解析，拷到 exe 旁边不起作用
//!   （开发机按文档导出 `LD_LIBRARY_PATH` 即可）。

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=DUCKDB_LIB_DIR");

    let Some(lib_dir) = std::env::var_os("DUCKDB_LIB_DIR") else {
        println!(
            "cargo:warning=未设置 DUCKDB_LIB_DIR：DuckDB 走动态链接，请先执行 \
             tools/fetch-duckdb.sh 并在 .cargo/config.toml 的 [env] 指向库目录 \
             （见 docs/architecture/dependencies/duckdb-linking.md）"
        );
        return;
    };
    let lib_dir = PathBuf::from(lib_dir);
    if !lib_dir.is_dir() {
        println!(
            "cargo:warning=DUCKDB_LIB_DIR 指向的目录不存在：{}（先跑 tools/fetch-duckdb.sh）",
            lib_dir.display()
        );
        return;
    }

    if !cfg!(target_os = "windows") {
        return;
    }
    let Some(dll) = first_existing(&lib_dir, &["duckdb.dll"]) else {
        // 静态链接时目录里只有 `duckdb_static.lib`：没有动态库要拷，属正常
        return;
    };
    println!("cargo:rerun-if-changed={}", dll.display());

    let Some(profile_dir) = profile_dir() else {
        println!("cargo:warning=无法从 OUT_DIR 推断 target profile 目录：跳过 duckdb.dll 拷贝");
        return;
    };
    for dir in [profile_dir.clone(), profile_dir.join("deps")] {
        if let Err(e) = copy_if_stale(&dll, &dir) {
            println!(
                "cargo:warning=拷贝 duckdb.dll 到 {} 失败：{e}（可手动复制该文件）",
                dir.display()
            );
        }
    }
}

/// 按顺序取第一个存在的候选（找不到返回 `None`）。
fn first_existing(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// `target/<profile>`（`OUT_DIR` = `target/<profile>/build/<pkg>-<hash>/out`；带 `--target`
/// 时中间会多一层 triple，所以按祖先里的 `build` 定位而不是数层数）。
fn profile_dir() -> Option<PathBuf> {
    let out = PathBuf::from(std::env::var_os("OUT_DIR")?);
    let build = out
        .ancestors()
        .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "build"))?;
    build.parent().map(Path::to_path_buf)
}

/// 拷到目标目录（内容相同则跳过，避免每次构建都改文件时间）。
fn copy_if_stale(source: &Path, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let dest = dir.join(source.file_name().unwrap_or_default());
    if let (Ok(from), Ok(to)) = (source.metadata(), dest.metadata()) {
        if from.len() == to.len() {
            return Ok(());
        }
    }
    std::fs::copy(source, &dest).map(|_| ())
}
