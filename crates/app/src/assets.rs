//! 应用资产源：组件库内置图标 + **运行时品牌包**。
//!
//! ## 为什么要有这一层
//!
//! 组件库自带的 Lucide 全量集里**没有任何数据库品牌标**（海豚 / 大象 / 鸭子 / 红标都是厂商
//! 注册商标，判断属产品与法务，见 `docs/architecture/ui/db-icons.md`）。工程侧因此把品牌标
//! 做成**可选资源**：不随包发布、不进仓库，用户自己丢进 `<RDS_HOME>/icons/db/<type_id>.svg`
//! 就能生效——这样"用不用品牌标"是配置，而不是发布内容。
//!
//! ## 规则
//!
//! - 路径 `icons/db/<name>.svg`：先查品牌包目录，命中即用；
//! - 未命中 / 读失败：**回落到组件库内置资产**（通用图标，界面照常显示，不会空白）；
//! - 其余路径：一律原样交给内置资产源（行为与替换前完全一致）。
//!
//! ## 两个实测约束
//!
//! **颜色要能跟主题走**：gpui 用元素的 `text_color` 解析 SVG 里的 `currentColor`
//! （`gpui/src/elements/svg.rs` 把 `style.text.color` 传进光栅化）。Lucide 是
//! `stroke="currentColor"`，天然满足；而 simple-icons 这类是 **fill 路径且根标签不带 `fill`**
//! （默认黑，明暗主题下永远是黑的），所以这里在加载时补一次 `fill="currentColor"`
//! —— 仅在根标签**既无 `fill` 也无 `stroke`** 时补，彩色品牌标与 Lucide 风格都不受影响。
//!
//! **不能在渲染路径上反复读盘**：图标每帧都要取资产，所以品牌包按路径**只读一次**并缓存
//! （含"这张不存在"的结论）。代价是换了图要重启应用才生效——对"用户自己放素材"这个场景
//! 可以接受，换来的是 render 里没有磁盘 I/O。

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use gpui_kit::{AssetSource, Result, SharedString};

/// 品牌包目录：`<RDS_HOME>/icons/db`（`RDS_HOME` 的解析见 `docs/architecture/runtime/data-paths.md`）。
pub fn brand_pack_dir() -> PathBuf {
    paths::home().join("icons").join("db")
}

/// 应用资产源：品牌包（磁盘，可选）→ 组件库内置资产。
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(name) = brand_icon_name(path) {
            if let Some(bytes) = cached_brand_icon(name) {
                return Ok(Some(Cow::Owned(bytes)));
            }
        }
        // 未命中就交给内置源：它覆盖全部 Lucide 图标，并对未知路径返回 Err（名字写错能在日志里看见，
        // 而不是静默空白——这正是"品牌包文件名打错"最容易踩的坑）。
        gpui_kit::assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        // 枚举以内置资产为准：品牌包是用户私有素材，不参与列表（避免把用户目录内容带进 UI）。
        gpui_kit::assets::AllAssets.list(path)
    }
}

/// `icons/db/<name>.svg` → `Some(name)`；其余路径 → `None`。
///
/// 只认**一层**文件名：不认子目录、不认路径分隔符（`..` / `\` 之类一律拒绝），
/// 免得上层传进来的路径把品牌包目录当成任意文件读取入口。
fn brand_icon_name(path: &str) -> Option<&str> {
    let name = path.strip_prefix("icons/db/")?;
    if name.is_empty() || name.contains(['/', '\\']) || !name.ends_with(".svg") {
        return None;
    }
    Some(name)
}

/// 按路径读一次品牌图标并缓存（含"不存在"）。
fn cached_brand_icon(name: &str) -> Option<Vec<u8>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Vec<u8>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    // 锁中毒不是致命错误：退化为"每次都读盘"，不至于让图标整体消失。
    let Ok(mut guard) = cache.lock() else {
        return read_brand_icon(name);
    };
    if let Some(hit) = guard.get(name) {
        return hit.clone();
    }
    let bytes = read_brand_icon(name);
    guard.insert(name.to_string(), bytes.clone());
    bytes
}

/// 读品牌包里的一个图标；不存在 / 读失败 → `None`（回落到内置图标）。
fn read_brand_icon(name: &str) -> Option<Vec<u8>> {
    let file = brand_pack_dir().join(name);
    let bytes = std::fs::read(&file).ok()?;
    // 非 UTF-8（二进制 SVG 极少见）原样使用，不做文本处理。
    match String::from_utf8(bytes) {
        Ok(text) => Some(current_color(text).into_bytes()),
        Err(e) => Some(e.into_bytes()),
    }
}

/// 单色 SVG 补 `fill="currentColor"`，让图标跟随主题色（见模块头注）。
///
/// 只在**根标签既没有 `fill` 也没有 `stroke`** 时补：
/// - Lucide 风格（`stroke="currentColor"`）不动；
/// - 自带配色的彩色品牌标（根标签带 `fill="#…"`）不动；
/// - 硬编码黑色 fill 的图**也落在"根标签无 fill"之外**（fill 在 `<path>` 上）→ 这里会补根标签，
///   但 `<path>` 自己的 `fill` 优先级更高，所以那类图仍保持原色（这是有意的：不猜用户意图）。
fn current_color(svg: String) -> String {
    let Some(tag_end) = svg.find('>') else {
        return svg;
    };
    let tag = &svg[..tag_end];
    if !tag.trim_start().starts_with("<svg") {
        return svg;
    }
    let lower = tag.to_ascii_lowercase();
    if lower.contains("fill=") || lower.contains("stroke=") {
        return svg;
    }
    format!("{tag} fill=\"currentColor\"{}", &svg[tag_end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_flat_icons_under_the_brand_namespace_are_served_from_disk() {
        assert_eq!(brand_icon_name("icons/db/mysql.svg"), Some("mysql.svg"));
        assert_eq!(
            brand_icon_name("icons/db/postgresql.svg"),
            Some("postgresql.svg")
        );
        // 内置命名空间与其它路径一律不碰
        assert_eq!(brand_icon_name("icons/database.svg"), None);
        assert_eq!(brand_icon_name("icons/db"), None);
        // 只认一层：子目录与上跳一律拒绝（品牌包目录不是任意文件读取入口）
        assert_eq!(brand_icon_name("icons/db/sub/mysql.svg"), None);
        assert_eq!(brand_icon_name("icons/db/../secrets.svg"), None);
        assert_eq!(brand_icon_name("icons/db/..\\secrets.svg"), None);
        // 只认 SVG（内置图标资产也是 SVG；别的扩展名没有渲染路径）
        assert_eq!(brand_icon_name("icons/db/mysql.png"), None);
    }

    #[test]
    fn monochrome_svg_gets_a_current_color_fill() {
        // simple-icons 形态：根标签只有 role/viewBox/xmlns，无 fill / stroke
        let duckdb = r#"<svg role="img" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><title>DuckDB</title><path d="M12 0"/></svg>"#;
        let out = current_color(duckdb.to_string());
        assert!(
            out.starts_with(r#"<svg role="img" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" fill="currentColor">"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<path d="M12 0"/>"#),
            "路径部分不该被改动：{out}"
        );
    }

    #[test]
    fn stroke_icons_and_colored_logos_are_left_alone() {
        // Lucide 形态（stroke 取 currentColor）——已经能跟主题，不该被改
        let lucide = r#"<svg xmlns="http://www.w3.org/2000/svg" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 5V19"/></svg>"#;
        assert_eq!(current_color(lucide.to_string()), lucide);
        // 彩色品牌标（根标签自带 fill）不该被改色
        // （这里用 r##"…"##：SVG 里的 `"#` 会提前结束单层 raw string）
        let colored =
            r##"<svg xmlns="http://www.w3.org/2000/svg" fill="#00758f"><path d="M0 0"/></svg>"##;
        assert_eq!(current_color(colored.to_string()), colored);
        // 不是 SVG 根标签 / 没有 '>' 的输入原样返回（不 panic、不猜）
        assert_eq!(
            current_color("not svg at all".to_string()),
            "not svg at all"
        );
        assert_eq!(current_color("<svg".to_string()), "<svg");
    }

    #[test]
    fn brand_pack_dir_follows_the_data_root() {
        // 目录跟随 RDS_HOME（测试构建下由 `paths` 的 test-support 隔离到临时目录）
        let dir = brand_pack_dir();
        assert!(dir.ends_with("icons/db"), "{dir:?}");
        assert_eq!(
            dir.parent().and_then(|p| p.file_name()),
            Some("icons".as_ref())
        );
    }

    #[test]
    fn a_dropped_in_brand_icon_is_served_and_normalized() {
        let dir = brand_pack_dir();
        std::fs::create_dir_all(&dir).expect("建品牌包目录");
        let file = dir.join("__test_brand.svg");
        // simple-icons 形态：单色 fill 路径 + 根标签不带 fill
        std::fs::write(&file, r#"<svg viewBox="0 0 24 24"><path d="M0 0"/></svg>"#)
            .expect("写测试图标");

        let bytes = AppAssets
            .load("icons/db/__test_brand.svg")
            .expect("load 不该报错")
            .expect("品牌包应命中");
        let text = String::from_utf8(bytes.to_vec()).expect("SVG 是文本");
        assert!(text.contains(r#"fill="currentColor""#), "应补上主题色：{text}");

        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn a_missing_brand_icon_falls_back_to_the_builtin_source() {
        // 名字唯一：缓存按名字记住结论（含“不存在”），同名会让用例互相干扰
        assert!(
            AppAssets.load("icons/db/__test_absent.svg").is_err(),
            "品牌包未命中 → 交给内置源；内置也没这张 → Err（名字写错能在日志里看见）"
        );
        // 换资产源不能影响既有图标：内置图标照常可读
        assert!(
            AppAssets
                .load("icons/database.svg")
                .expect("内置可读")
                .is_some()
        );
    }
}
