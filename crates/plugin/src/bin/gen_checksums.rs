//! 生成插件包的 `checksums.json`（包内文件 + `[backend] driver_jars` 的外部文件）。
//!
//! 用法：
//!
//! ```text
//! cargo run -p rds-plugin --bin rds-plugin-checksums -- <插件目录>
//! ```
//!
//! 打包流程里跑一次，产物写进插件目录根，随后随包分发。安装时
//! `checksums::verify_package` 用它核对字节是否还是作者签发的那一份；运行时载荷
//! 变化触发的自动重启也会先过一遍同样的校验（见 `manager::restart_after_payload_verify`）。

use std::path::PathBuf;
use std::process::ExitCode;

use rds_plugin::checksums::{CHECKSUMS_FILE, generate_package};
use rds_plugin::manifest::ManifestParser;

fn main() -> ExitCode {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("用法：rds-plugin-checksums <插件目录>");
        return ExitCode::from(2);
    };
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        eprintln!("不是目录：{}", dir.display());
        return ExitCode::from(2);
    }

    let manifest_path = dir.join("plugin.toml");
    let manifest = match ManifestParser::parse(&manifest_path) {
        Ok(manifest) => manifest,
        Err(e) => {
            eprintln!("解析清单失败（{}）：{e}", manifest_path.display());
            return ExitCode::FAILURE;
        }
    };

    let json = match generate_package(&manifest, &dir) {
        Ok(json) => json,
        Err(e) => {
            eprintln!("生成校验清单失败：{e}");
            return ExitCode::FAILURE;
        }
    };

    let out = dir.join(CHECKSUMS_FILE);
    if let Err(e) = std::fs::write(&out, &json) {
        eprintln!("写入 {} 失败：{e}", out.display());
        return ExitCode::FAILURE;
    }
    println!("已生成 {}（{} 字节）", out.display(), json.len());
    ExitCode::SUCCESS
}
