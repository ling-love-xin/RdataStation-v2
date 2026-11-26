//! 全工作区 UI 契约守门（25 个 crate 全覆盖）。
//!
//! 与 `crates/{workbench,insight}/tests/ui_contract.rs` 的关系：那两份只覆盖各自 crate，
//! 本守门扫 `crates/**/src/**/*.rs` 全量，是它们的**超集补充**而非替代。
//!
//! ## 口径：冻结基线，只拦新增
//! 存量命中登记在 `BASELINE` 里 —— 其中绝大多数是各 crate `ui.rs` 中的**合法常量定义**
//! （`pub const COLUMN_DIALOG_WIDTH: Pixels = px(840.)`），这类写法是本项目推荐做法，不是违规。
//! 因此守门不要求一次性清零，而是：
//! - 命中数**上升** → 立即失败（有人在视图层新写了裸色值 / 裸尺寸）；
//! - 命中数**下降** → 只提示不失败（鼓励顺手把基线调下来）。
//!
//! 新增豁免时必须同步上调 `BASELINE` 并写明理由，否则守门形同虚设。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// (相对仓库根的路径, 裸颜色构造命中数, 裸 `px(<数字>` 命中数)
const BASELINE: &[(&str, usize, usize)] = &[
    ("crates/editor/src/lsp/document_color.rs", 1, 0),
    ("crates/editor/src/support/ui.rs", 0, 2),
    ("crates/mock/src/mock_view/progress.rs", 0, 3),
    // 3 = 几何原语，都是**物理像素**、不随字号缩放，放在本文件（样式原语层）单一来源：
    // `px(0.)` 标识条左缘归零 · `px(1.)` 卡片内分隔发丝线 · `px(3.)` chip 圆角。
    // 提去 ui.rs 反而把「形态在函数内单一来源」拆散，故登记不迁。
    ("crates/mock/src/mock_view/style.rs", 0, 3),
    ("crates/mock/src/schema_map.rs", 1, 0),
    // 26 = 原 12 + 拆分补齐的 8 个 Pixels 常量（`CHIP_RADIUS` / `ROW_CARD_PY` /
    // `DETAIL_COLUMNS_MAX_HEIGHT` / `FLEX_BASIS_ZERO` / `GENERATOR_BODY_HEIGHT` /
    // `GENERATOR_MENU_WIDTH` / `GENERATOR_MENU_MAX_HEIGHT` / `GENERATOR_PICK_INDENT`）。
    // 本文件是尺寸常量单一来源，`px(N.)` 在此是**推荐写法**，不是违规。
    ("crates/mock/src/ui.rs", 0, 26),
    ("crates/notification/src/view.rs", 0, 2),
    // 1 = 标题栏项目下拉的最小宽 `.min_w(px(200.))`（2026-11-26 实测）：`PopupMenuItem` 的标签
    // 装在 `w_full()` 容器里、不参与菜单宽计算，默认 128 时超过 5 个全角字的文案会画出右缘
    // （弹层不裁剪）。它是「弹层最小宽」这一个形态事实，不是内距 / 字号档位，故就地写并登记。
    ("crates/project/src/ui/picker.rs", 0, 1),
    ("crates/result_panel/src/grid/header.rs", 0, 1),
    // 1 = 状态行左右两列之间的固定间距 `.gap(px(5.))`：DBeaver 外层 `GridLayout(2, false)`
    // 用的是 SWT 默认 `horizontalSpacing = 5`（未显式设置，`ResultSetFilterPanel.java:163-166`
    // 只改了 margin）。它不是字号档位，跟着 rem 缩放会让状态行在放大字号时挤到换行，
    // 故就地写物理像素、在这里登记。
    ("crates/result_panel/src/grid/status.rs", 0, 1),
    // 5 = 结果面板对话框的三处几何 + 本版两个新对话框各一处窗口宽度。
    // 3 = 列宽对话框的 `.w(px(680.))` / `.max_h(px(480.))` / `.h(px(360.))`（窗口尺寸按内容
    // 行宽定死的物理像素，同 `archive_dialog` / `export_dialog` 那一档，提去 `ui.rs` 会把
    // 「窗口多大」与「控件留白」混成一张表）。
    // 2026-11-26 3 → 5：新增两个对话框（`editor-result-auto-refresh-config` 自动刷新配置、
    // `editor-result-copy-names` 复制列名选项）各一处 `.w(px(420.))`——同一档窗口宽度原值。
    ("crates/result_panel/src/panel/dialog.rs", 0, 5),
    // 7 = 筛选条 / 对象格信息浮层的描边内距：`.offset(px(2.))` ×2 与 `.p/.py(px(2.))` ×5，
    // 都是 DBeaver 侧 `ResultSetFilterPanel` / 对象格浮层的原值（2px 边框内距），
    // 与 `RESULT_TOOLBAR_PADDING` 同属「跟 DBeaver 像素相等」的一类，不随字号缩放。
    ("crates/result_panel/src/panel/render.rs", 1, 7),
    ("crates/result_panel/src/panel/sidebars.rs", 0, 2),
    // 16 = 原 4 + 类型默认列宽 8 档（numeric / boolean / date·time / timestamp / uuid /
    // json / binary / text，`20fb50a4`）+ 列宽自适应 3 档（`RESULT_FIT_CELL_PADDING` /
    // `RESULT_FIT_HEADER_GAP` / `RESULT_FIT_BADGE_PADDING`，`44040e21` 起原先散在
    // `grid/fit.rs`，2026-11-08 按「结构尺寸进常量表」收上来）+ 注释表头两行高 1 档
    // （`RESULT_HEADER_COMMENT_ROW_HEIGHT`：注释开启时表头抬到 40px，否则两行挤在 22px
    // 内容高里各被裁一半）。列头菜单占宽那档（`RESULT_FIT_COLUMN_MENU`）已删除——菜单
    // 改成绝对定位、悬停才显形，不再占列宽也不再参与测宽。
    // 本文件是尺寸常量单一来源，`px(N.)` 在此是**推荐写法**，不是违规。
    // 2026-11-08 16 → 17：新增 `RESULT_TOOLBAR_PADDING = px(3.)`，对齐 DBeaver
    // `ResultSetFilterPanel` 的 `GridLayout.marginWidth = 3`（`ResultSetFilterPanel.java:165`）。
    // 3px 是 DBeaver 原值，不能拿 `px_1`(4px) 凑档位。
    // 2026-11-17 17 → 18：新增 `RESULT_CELL_VERTICAL_PADDING = px(2.)`（单元格上下内距，
    // 沿用组件 XSmall 档 `table_cell_padding()` 的上下原值；左右 6px 由单元格自己画，
    // 让竖分隔线落在列边界，见 `grid/table_impl.rs::cell_paddings`）。
    // 2026-11-18 18 → 20：新增 `RESULT_FIT_HEADER_ICON = px(12.)`（表头筛选漏斗图标宽，
    // 图标量不出字形推进宽，只能按 `Icon::size_3()` 的尺寸档记常量）与
    // `RESULT_FIT_HEADER_CHROME = px(20.)`（组件排序图标槽 16 + 右侧偏移 4：自适应宽度
    // 原来漏算这 20px，双击算出的列宽会把列名挤成省略号）。
    // 2026-11-18 20 → 21：新增 `RESULT_CELL_PADDING_X = px(6.)`（单元格**单侧**左右内距，
    // 渲染侧（`grid/cell.rs` / `grid/header.rs`）与列宽自适应（`grid/fit.rs`）共用这一个来源；
    // 原先渲染写 `rems(0.375)`、自适应写 `RESULT_FIT_CELL_PADDING = px(12.)`，两个独立编码
    // 一旦分叉，双击算出的列宽就会差 12px）。
    // 2026-11-25 21 → 22：**不是新增常量**，只多一处文档注释 —— `TAB_STATUS_SLOT` 的注释里
    // 引用了 Zed 的 `START_TAB_SLOT_SIZE = px(12.)` 作为出处。守门按全文匹配，注释也算命中。
    // 2026-11-26 22 → 25：行号槽补数组展开三角（真值 `GridRowRenderer.java:51-86`）带来三档
    // 新常量——`RESULT_ROW_HEAD_MARGIN_X = px(4.)`（行号槽单侧内距）、
    // `RESULT_ROW_EXPANDER_SIZE = px(16.)`（三角图标盒，SVG 原尺寸）、
    // `RESULT_ROW_EXPANDER_SPACING = px(2.)`（三角与数字间距）。
    ("crates/result_panel/src/ui.rs", 0, 25),
    ("crates/settings/src/skeleton.rs", 0, 1),
    ("crates/workbench/src/components/dialog_helpers.rs", 0, 1),
    (
        "crates/workbench/src/components/group_manager_dialog.rs",
        0,
        1,
    ),
    ("crates/workbench/src/components/saved_view_dialog.rs", 0, 1),
    ("crates/workbench/src/components/tag_assign_dialog.rs", 0, 1),
    (
        "crates/workbench/src/components/tag_manager_dialog.rs",
        0,
        1,
    ),
    // 1 = 归档对话框宽度 `w(px(560.0))`：窗口尺寸是物理像素，内容是最长一行不折行的宽度；
    // 它不是视图内距 / 字号档位，提去 `ui.rs` 会把「窗口多大」与「控件留白」混成一张表。
    ("crates/workbench/src/components/archive_dialog.rs", 0, 1),
    // 1 = 导出对话框宽度 `w(px(520.0))`：同上（窗口尺寸按内容行宽定死，DBeaver 导出页同档）。
    ("crates/workbench/src/components/export_dialog.rs", 0, 1),
    // 3 = 对象属性弹窗的三处几何：`label_width(px(96.0))`（属性名列宽，按最长中文标签量出）、
    // `w(px(560.0))`（窗口宽度）、`h(px(1.0))`（基本信息与列结构之间的发丝分隔线）。
    // 三者都是「这一张窗口的形态」，不是可复用的字号 / 内距档位，故就地写并登记。
    ("crates/workbench/src/components/object_props.rs", 0, 3),
    // 「关于」弹窗（`version_dialog.rs`）的 1 处裸 px 已随版本弹窗改 `DescriptionList` 收进
    // `ui.rs` 常量，本版把该文件从基线里删掉（再出现就算新增）。
    // 1 = 解锁窗尺寸 `size(px(440.), px(250.))`：窗口尺寸是**物理像素**（不随字号缩放，
    // 缩放会让口令框一屏放不下），且只有这一处来源；它不是视图内距/字号档位，
    // 提去 `ui.rs` 反而把「窗口多大」与「控件留白」两件事混在一张表里。
    ("crates/workbench/src/unlock.rs", 0, 1),
    ("crates/workbench_shell/src/ui.rs", 0, 7),
];

/// 颜色构造（与既有 `ui_contract.rs` 同口径：视图层不得自己造颜色，只能用 token + `.opacity()`）
const COLOR_PATTERNS: &[&str] = &["hsla(", "Hsla::", "rgb(", "Rgba::"];

/// 允许「跨 crate 同名但取值不同」的常量（新增必须登记并写明理由，否则守门失败）。
///
/// 背景：2026-11-02 收敛 `RESULT_*` 时发现，`editor/src/support/ui.rs` 与
/// `result_panel/src/ui.rs` 各定义一份，且 `RESULT_PANE_HEIGHT` 已漂移到 20.0 / 18.0
/// 两个值 —— 两份都是「结果区默认高度」，却只有一个真正生效。同类漂移不会报编译错，
/// 只会表现为「改了常量没反应」，所以放在这里拦。
const CONST_DIVERGENCE_ALLOWED: &[(&str, &str)] = &[
    (
        "ALL",
        "各模块的枚举全集关联常量（`Self::A, Self::B`），同名纯属巧合、语义无关",
    ),
    (
        "HISTORY_LIMIT",
        "editor 的撤销历史栈(500) 与 mock 的生成历史(20) 是两个功能，不是同一档口径",
    ),
];

/// `product-tokens.json` 的合法角色名（`ProductTokens` 结构体字段决定，写错即静默丢弃）
const PRODUCT_ROLES: &[&str] = &[
    "activity_bar.background",
    "activity_bar.active_border",
    "activity_bar.icon.active",
    "activity_bar.icon.inactive",
    "title_bar.slot.background",
    "quick_open.group.header",
    "search.match.background",
    "staged.drop",
    "result.local_row_background",
    // 【结果集配色槽】对齐 DBeaver 结果集 colorDefinition 的两个高亮槽
    //（斑马行不设产品槽：2026-11-21 定案 A，斑马底色跟随主题 `table_even`）
    "result.cell.modified.background",
    "result.cell.error.background",
    // 表头分隔线：DBeaver 明暗两套强度不同（浅色系统阴影色 / 深色 css 明确 #696969），
    // 一个标准字段顶不住，见 docs/architecture/editor/result-panel.md §8.38
    "result.header.border",
];

struct Hits {
    color: usize,
    px: usize,
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/ui_guard 的上两级即仓库根")
        .to_path_buf()
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// 匹配 `px(` 后（可含空白）紧跟数字：`px(280.)`、`gpui_kit::px(3.)` 命中；
/// `px(w)`（变量）不命中 —— 后者是随字号缩放的正确写法。
fn has_literal_px(line: &str) -> bool {
    line.match_indices("px(").any(|(i, _)| {
        matches!(
            line[i + 3..].chars().find(|c| !c.is_whitespace()),
            Some(c) if c.is_ascii_digit()
        )
    })
}

fn scan_hits(path: &Path) -> Hits {
    let Ok(content) = fs::read_to_string(path) else {
        return Hits { color: 0, px: 0 };
    };
    let mut hits = Hits { color: 0, px: 0 };
    for line in content.lines() {
        // 文件内的 `#[cfg(test)]` 之后是测试代码，裸色值在那里是合法的。
        if line.trim_start().starts_with("#[cfg(test)]") {
            break;
        }
        if COLOR_PATTERNS.iter().any(|p| line.contains(p)) {
            hits.color += 1;
        }
        if has_literal_px(line) {
            hits.px += 1;
        }
    }
    hits
}

#[test]
fn no_new_hardcoded_color_or_pixel() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);
    assert!(!files.is_empty(), "没扫到任何 .rs —— 仓库根解析有误");

    let mut failures = Vec::new();
    let mut lower = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.contains("/tests/") || rel.ends_with("/tests.rs") || rel.contains("/examples/") {
            continue;
        }
        let hits = scan_hits(path);
        match BASELINE.iter().find(|(p, _, _)| *p == rel) {
            Some((_, base_color, base_px)) => {
                if hits.color > *base_color {
                    failures.push(format!(
                        "{rel}: 裸颜色构造 {base_color} -> {}（新增，请改用主题 token）",
                        hits.color
                    ));
                }
                if hits.px > *base_px {
                    failures.push(format!(
                        "{rel}: 裸 px({base_px}) -> {}（新增，请提为 ui.rs 常量并随字号缩放）",
                        hits.px
                    ));
                }
                if hits.color < *base_color || hits.px < *base_px {
                    lower.push(format!(
                        "{rel}: 基线 ({base_color},{base_px}) 可下调为 ({},{})",
                        hits.color, hits.px
                    ));
                }
            }
            None => {
                if hits.color > 0 || hits.px > 0 {
                    failures.push(format!(
                        "{rel}: 未登记的新文件出现裸色值 {} / 裸 px {}（请登记 BASELINE 并写明理由）",
                        hits.color, hits.px
                    ));
                }
            }
        }
    }

    if !lower.is_empty() {
        eprintln!(
            "[提示] 以下文件命中数已低于基线，可顺手下调 BASELINE：\n  {}",
            lower.join("\n  ")
        );
    }
    assert!(
        failures.is_empty(),
        "新增了硬编码色值或裸 px：\n  {}",
        failures.join("\n  ")
    );
}

/// 从 gpui-kit 的 schema 抽取 `ThemeConfigColors` 的全部合法 JSON 键名。
///
/// 注意 `base.*` 系列字段是**私有**的（`blue: Option<String>`，无 `pub`），
/// 只按 `pub` 匹配会漏掉 12 个键 —— 这里对可选 `pub` 都匹配。
fn schema_color_keys(root: &Path) -> Vec<String> {
    let src = fs::read_to_string(root.join("vendor/gpui-component/src/theme/schema.rs"))
        .expect("读 vendor schema.rs");
    let start = src
        .find("pub struct ThemeConfigColors {")
        .expect("找不到 ThemeConfigColors");
    let bytes = src.as_bytes();
    let mut depth = 0i32;
    let mut end = start;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    let mut keys: Vec<String> = Vec::new();
    let mut pending: Option<String> = None;
    for line in src[start..=end].lines() {
        if let Some(rest) = line.split_once("#[serde(rename = \"").map(|(_, r)| r) {
            if let Some((k, _)) = rest.split_once("\")]") {
                pending = Some(k.to_string());
            }
        }
        let t = line.trim_start();
        let t = t.strip_prefix("pub ").unwrap_or(t).trim_start();
        let b = t.as_bytes();
        let n = b
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || **c == b'_')
            .count();
        if n > 0 && n < b.len() && b[n] == b':' {
            if let Some(k) = pending.take() {
                keys.push(k);
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

#[test]
fn theme_asset_colors_only_use_schema_keys() {
    let root = repo_root();
    let keys: HashSet<String> = schema_color_keys(&root).into_iter().collect();
    assert_eq!(
        keys.len(),
        143,
        "ThemeConfigColors 键数变了：gpui-kit 升级了？升级后请重跑 tools/gen_theme_keys.py"
    );

    let content =
        fs::read_to_string(root.join("assets/themes/rds-theme.json")).expect("读主题资产");
    let value: serde_json::Value = serde_json::from_str(&content).expect("解析主题资产");
    let themes = value
        .get("themes")
        .and_then(|v| v.as_array())
        .expect("themes 数组");
    let empty = serde_json::Map::new();
    let mut unknown = Vec::new();
    for theme in themes {
        let name = theme.get("name").and_then(|v| v.as_str()).unwrap_or("?");
        let colors = theme
            .get("colors")
            .and_then(|v| v.as_object())
            .unwrap_or(&empty);
        for k in colors.keys() {
            if !keys.contains(k) {
                unknown.push(format!("{name}: {k}"));
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "主题资产含未知 colors 键 —— 会被 serde 静默忽略并回退默认色：{unknown:?}"
    );

    let wl_content = fs::read_to_string(root.join("assets/themes/theme-keys.json"))
        .expect("读 theme-keys.json（由 tools/gen_theme_keys.py 生成）");
    let wl: serde_json::Value = serde_json::from_str(&wl_content).expect("解析 theme-keys.json");
    let wl_keys: HashSet<String> = wl
        .get("keys")
        .and_then(|v| v.as_array())
        .expect("keys 数组")
        .iter()
        .filter_map(|v| v.as_str())
        .map(str::to_string)
        .collect();
    assert_eq!(
        wl_keys, keys,
        "theme-keys.json 与 schema 不一致，请重跑 tools/gen_theme_keys.py"
    );
}

/// 收集全仓 `pub const NAME: T = VALUE;`（跳过测试代码）。
fn collect_consts(root: &Path) -> HashMap<String, Vec<(String, String)>> {
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);
    let mut out: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for path in &files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.contains("/tests/") || rel.ends_with("/tests.rs") || rel.contains("/examples/") {
            continue;
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };
        for line in content.lines() {
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            let Some(rest) = line.trim_start().strip_prefix("pub const ") else {
                continue;
            };
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            let Some(eq) = rest.find('=') else {
                continue;
            };
            let value = rest[eq + 1..]
                .trim()
                .trim_end_matches(';')
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if value.is_empty() {
                continue;
            }
            out.entry(name).or_default().push((rel.clone(), value));
        }
    }
    out
}

/// 同名 `pub const` 跨 crate 定义时，取值必须一致（或登记在 `CONST_DIVERGENCE_ALLOWED`）。
#[test]
fn shared_constants_do_not_diverge() {
    let root = repo_root();
    let consts = collect_consts(&root);
    let allowed: HashSet<&str> = CONST_DIVERGENCE_ALLOWED.iter().map(|(n, _)| *n).collect();

    let mut failures = Vec::new();
    let mut duplicates = Vec::new();
    for (name, defs) in &consts {
        if defs.len() < 2 || allowed.contains(name.as_str()) {
            continue;
        }
        let values: HashSet<&str> = defs.iter().map(|(_, v)| v.as_str()).collect();
        if values.len() > 1 {
            let detail = defs
                .iter()
                .map(|(f, v)| format!("{v}  ({f})"))
                .collect::<Vec<_>>()
                .join("\n      ");
            failures.push(format!(
                "{name}: 跨文件取值分叉 —— 同名常量两个值只会有一个生效，\
                 改另一个不会报错也不会生效：\n      {detail}"
            ));
        } else {
            duplicates.push(format!(
                "{name} = {}  （{} 处：{}）",
                defs[0].1,
                defs.len(),
                defs.iter()
                    .map(|(f, _)| f.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }

    if !duplicates.is_empty() {
        duplicates.sort();
        eprintln!(
            "[提示] 以下同名常量重复定义且取值一致（收敛候选，改成 re-export 即可单一来源）：\n  {}",
            duplicates.join("\n  ")
        );
    }
    assert!(
        failures.is_empty(),
        "跨 crate 同名常量取值分叉：\n  {}",
        failures.join("\n  ")
    );
}

#[test]
fn product_tokens_use_declared_role_names() {
    let root = repo_root();
    let content =
        fs::read_to_string(root.join("assets/themes/product-tokens.json")).expect("读产品 token");
    let value: serde_json::Value = serde_json::from_str(&content).expect("解析产品 token");
    let known: HashSet<&str> = PRODUCT_ROLES.iter().copied().collect();
    let mut unknown = Vec::new();
    for mode in ["light", "dark"] {
        if let Some(obj) = value.get(mode).and_then(|v| v.as_object()) {
            for k in obj.keys() {
                if !known.contains(k.as_str()) {
                    unknown.push(format!("{mode}: {k}"));
                }
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "product-tokens.json 含未声明的角色名 —— ProductTokens 无此字段，值会被静默丢弃：{unknown:?}"
    );
}
