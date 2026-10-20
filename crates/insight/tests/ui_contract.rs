//! 洞察面板的 UI 契约测试（对齐 `crates/workbench/tests/ui_contract.rs` 的口径）。
//!
//! workbench 的 `ui_contract` 只扫 `view.rs` / `panels/`，**不覆盖本 crate**——洞察的
//! `ui.rs` 登记约定（结构尺寸进常量、零裸 px、零裸色）此前靠自觉。这里把同样的
//! 不变式固化成测试，防止后续改动静默破坏：
//!
//! 1. **尺寸常量契约**：`ui.rs` 的 M8 倍率 × 16 必须等于注释里的设计值；
//! 2. **源码契约**：视图层不出现裸 `px(<数字>` 字面量（结构尺寸走 `ui.rs` 常量，
//!    局部间距走 Tailwind 尺度方法）；
//! 3. **源码契约**：视图层不直接构造颜色（`rgb(` / `hsla(` / `Hsla::`），一律
//!    `cx.theme().colors.*`；
//! 4. **清单守卫**：`src/view/` 下每个 `.rs` 与显式点名的顶层视图文件必须登记进
//!    共享清单（漏登会让 2 / 3 对那个文件静默失效）。

/// 默认正文字号（`ui.rs` 倍率换算基准；与 workbench 契约同一口径）。
const BASE_FONT_SIZE: f32 = 16.0;

/// 契约 1：M8 专用结构尺寸倍率 × 16 必须等于设计值。
///
/// 数值来源：`crates/insight/src/ui.rs` 各常量的文档注释（新增常量时同步加一行）。
#[test]
fn ui_size_constants_match_design() {
    use rds_insight::ui::*;

    let cases: &[(f32, f32, &str)] = &[
        (INSIGHT_TAB_HEIGHT, 28.0, "Tab 条高度"),
        (INSIGHT_HISTOGRAM_BAR_HEIGHT, 8.0, "直方图条高度"),
        (INSIGHT_RATIO_BAR_HEIGHT, 6.0, "单条占比条高度"),
        (INSIGHT_INLINE_ICON_SIZE, 14.0, "行内小图标"),
        (INSIGHT_SECTION_TITLE_FONT, 12.0, "分区标题字号"),
        (INSIGHT_SCORE_FONT, 28.0, "评分卡总分字号"),
        (INSIGHT_HEALTH_SCORE_FONT, 36.0, "Schema 健康分字号"),
        (INSIGHT_RULES_DIALOG_WIDTH, 640.0, "规则管理对话框宽度"),
        (INSIGHT_TABLE_INDEX_WIDTH, 20.0, "表探查序号列宽"),
        (INSIGHT_TABLE_TYPE_WIDTH, 72.0, "表探查类型列宽"),
        (INSIGHT_TABLE_FLAG_WIDTH, 28.0, "表探查可空列宽"),
        (INSIGHT_TABLE_QUALITY_WIDTH, 52.0, "表探查质量列宽"),
        (INSIGHT_DIFF_LABEL_WIDTH, 64.0, "版本对比标签列宽"),
    ];

    for (ratio, expected_px, label) in cases {
        assert_eq!(
            ratio * BASE_FONT_SIZE,
            *expected_px,
            "{label} 常量 {ratio}rem 偏离设计值 {expected_px}px（见 ui.rs 注释）"
        );
    }
}

/// 契约 2a：视图层不得出现裸尺寸字面量。
///
/// 判定与 workbench 契约同一实现：只拦 `px(<数字>`（`px(` 后直接跟数字）；
/// `py_px()` / `px_3()` 等命名方法、`.px(rems(ui::…))` 的结构性写法都不命中。
#[test]
fn view_layer_has_no_raw_size_literals() {
    for (name, src) in view_sources() {
        assert!(
            !has_literal_px(src),
            "{name} 出现裸 `px(...)` 尺寸字面量；结构尺寸请用 ui.rs 常量，局部间距用 Tailwind 尺度方法"
        );
    }
}

/// 契约 2b：UI 源码不得直接构造颜色，一律走主题 token。
#[test]
fn view_sources_have_no_raw_color_literals() {
    for (name, src) in view_sources() {
        assert!(
            !src.contains("rgb(") && !src.contains("hsla(") && !src.contains("Hsla::"),
            "{name} 出现裸色值构造；请从 `cx.theme().colors` 取色（透明用 transparent_black()）"
        );
    }
}

/// 契约 2c：受契约约束的每个视图文件必须登记进共享清单。
///
/// 契约 2a / 2b 依赖 [`view_sources`] 的显式清单——拆分 / 新增文件漏登记会让
/// 尺寸 / 颜色契约**静默失效**（扫不到就永不报错）。清单是编译期字面量、无法在
/// 运行期反射，故用本文件自身的源文本做覆盖校验。
#[test]
fn every_view_module_is_registered_in_the_manifests() {
    let manifest = include_str!("ui_contract.rs");
    for (name, _) in view_sources() {
        let quoted = format!("\"{name}\"");
        assert!(
            manifest.matches(quoted.as_str()).count() >= 1,
            "src/{name} 未登记进共享清单；漏登会让尺寸与颜色契约对该文件静默失效"
        );
    }
}

/// 受契约约束的视图文件清单（名字 + 源码；尺寸与颜色契约共用）。
///
/// - `src/view/` 目录递归收集（与 workbench `scan_rs_sources` 同模式）；
/// - 顶层视图文件（宿主 `InsightView` 渲染、Schema 报告、规则管理对话框）显式
///   点名——它们不在 `view/` 目录下，目录遍历发现不了。
///
/// 新增视图文件时：在 `view/` 目录内新增 → 在 `scan_view_sources` 自动出现，
/// 但必须在此处补 `include_str` 分支（守卫测试会拦住）；在 `src/` 顶层新增 →
/// 在此处的显式清单加一行。
fn view_sources() -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = vec![
        (
            "insight_view.rs".to_string(),
            include_str!("../src/insight_view.rs"),
        ),
        (
            "schema_view.rs".to_string(),
            include_str!("../src/schema_view.rs"),
        ),
        (
            "rule_view.rs".to_string(),
            include_str!("../src/rule_view.rs"),
        ),
    ];

    for rel in scan_view_sources() {
        let src: &'static str = match rel.as_str() {
            "view/mod.rs" => include_str!("../src/view/mod.rs"),
            "view/column.rs" => include_str!("../src/view/column.rs"),
            "view/header.rs" => include_str!("../src/view/header.rs"),
            "view/history.rs" => include_str!("../src/view/history.rs"),
            "view/multi.rs" => include_str!("../src/view/multi.rs"),
            "view/overview.rs" => include_str!("../src/view/overview.rs"),
            "view/schema.rs" => include_str!("../src/view/schema.rs"),
            "view/table.rs" => include_str!("../src/view/table.rs"),
            other => {
                panic!("view/ 目录出现未登记的源文件：{other}（请在本函数补 include_str 分支）")
            }
        };
        out.push((rel, src));
    }

    out
}

/// 递归收集 `src/view/` 下的全部 `.rs` 文件（返回相对 `src/` 的路径，`/` 分隔）。
fn scan_view_sources() -> Vec<String> {
    fn walk(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, base, out);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                let rel = path.strip_prefix(base).expect("已在 base 前缀约束下");
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }

    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    walk(&base.join("view"), &base, &mut out);
    out.sort();
    out
}

/// 是否存在 `px(<数字>` 形式的裸尺寸字面量（判定与 workbench 契约一致）。
fn has_literal_px(src: &str) -> bool {
    src.lines().any(|line| {
        line.find("px(")
            .map(|i| line.as_bytes().get(i + 3).is_some_and(u8::is_ascii_digit))
            .unwrap_or(false)
    })
}
