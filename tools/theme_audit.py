# 一次性审计脚本：RDS 主题资产 token 覆盖率 + WCAG 对比度实测
# 用法: python tools/theme_audit.py
import json, re, pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
SCHEMA = ROOT / "vendor/gpui-component/src/theme/schema.rs"
THEME = ROOT / "assets/themes/rds-theme.json"

# 1) 从 schema.rs 抽取 ThemeConfigColors 的全部 JSON 键名
src = SCHEMA.read_text(encoding="utf-8")
start = src.index("pub struct ThemeConfigColors {")
end = src.index("\nimpl ThemeColor {", start)
body = src[start:end]
schema_keys = re.findall(r'#\[serde\(rename = "([^"]+)"\)\]', body)
schema_keys = list(dict.fromkeys(schema_keys))
print(f"[schema] ThemeConfigColors 可配键总数: {len(schema_keys)}")

# 2) 读取主题资产
data = json.loads(THEME.read_text(encoding="utf-8"))
themes = {t["name"]: t for t in data["themes"]}

for name in ("RDS Light", "RDS Dark"):
    colors = themes[name]["colors"]
    missing = [k for k in schema_keys if k not in colors]
    unknown = [k for k in colors if k not in schema_keys]
    print(f"\n=== {name} ===")
    print(f"已定义 {len(colors)} / {len(schema_keys)}  缺失 {len(missing)}  未知键 {len(unknown)}")
    print("缺失:")
    for k in missing:
        print("  -", k)
    if unknown:
        print("未知键(schema 不接受):", unknown)


# 3) WCAG 对比度
def srgb_to_lin(c):
    c = c / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def lum(hexstr):
    h = hexstr.lstrip("#")
    if len(h) == 8:  # 带 alpha，按不透明处理并标注
        h = h[:6]
    r, g, b = (int(h[i : i + 2], 16) for i in (0, 2, 4))
    return 0.2126 * srgb_to_lin(r) + 0.7152 * srgb_to_lin(g) + 0.0722 * srgb_to_lin(b)


def ratio(a, b):
    la, lb = lum(a), lum(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


PAIRS = [
    ("foreground", "background", 4.5),
    ("muted.foreground", "background", 4.5),
    ("sidebar.foreground", "sidebar.background", 4.5),
    ("sidebar.accent.foreground", "sidebar.accent.background", 4.5),
    ("popover.foreground", "popover.background", 4.5),
    ("tab.foreground", "tab.background", 4.5),
    ("tab.active.foreground", "tab.active.background", 4.5),
    ("primary.foreground", "primary.background", 4.5),
    ("primary.foreground", "primary.hover.background", 4.5),
    ("primary.foreground", "primary.active.background", 4.5),
    ("secondary.foreground", "secondary.background", 4.5),
    ("success.foreground", "success.background", 4.5),
    ("danger.foreground", "danger.background", 4.5),
    ("warning.foreground", "warning.background", 4.5),
    ("info.foreground", "info.background", 4.5),
    ("table.head.foreground", "table.head.background", 4.5),
    ("table.foot.foreground", "table.foot.background", 4.5),
    ("accent.foreground", "background", 4.5),
    # 按钮系列
    ("button.foreground", "button.background", 4.5),
    ("button.primary.foreground", "button.primary.background", 4.5),
    ("button.primary.foreground", "button.primary.hover.background", 4.5),
    ("button.primary.foreground", "button.primary.active.background", 4.5),
    ("button.secondary.foreground", "button.secondary.background", 4.5),
    ("button.danger.foreground", "button.danger.background", 4.5),
    ("button.danger.foreground", "button.danger.hover.background", 4.5),
    ("button.danger.foreground", "button.danger.active.background", 4.5),
    ("button.success.foreground", "button.success.background", 4.5),
    ("button.success.foreground", "button.success.hover.background", 4.5),
    ("button.success.foreground", "button.success.active.background", 4.5),
    ("button.warning.foreground", "button.warning.background", 4.5),
    ("button.warning.foreground", "button.warning.hover.background", 4.5),
    ("button.warning.foreground", "button.warning.active.background", 4.5),
    ("button.info.foreground", "button.info.background", 4.5),
    ("button.info.foreground", "button.info.hover.background", 4.5),
    ("button.info.foreground", "button.info.active.background", 4.5),
    # 语义色 hover/active
    ("success.foreground", "success.hover.background", 4.5),
    ("success.foreground", "success.active.background", 4.5),
    ("danger.foreground", "danger.hover.background", 4.5),
    ("danger.foreground", "danger.active.background", 4.5),
    ("warning.foreground", "warning.hover.background", 4.5),
    ("warning.foreground", "warning.active.background", 4.5),
    ("info.foreground", "info.hover.background", 4.5),
    ("info.foreground", "info.active.background", 4.5),
    # 侧栏/状态栏/链接/描述列表
    ("sidebar.primary.foreground", "sidebar.primary.background", 4.5),
    # StatusBar 组件硬编码文字色 = muted_foreground（vendor/gpui-component/src/status_bar.rs:95）
    ("muted.foreground", "status_bar.background", 4.5),
    ("link", "background", 4.5),
    ("link.hover", "background", 4.5),
    ("link.active", "background", 4.5),
    ("description_list.label.foreground", "description_list.label.background", 4.5),
    ("group_box.foreground", "group_box.background", 4.5),
    ("group_box.title.foreground", "group_box.background", 4.5),
    ("popover.foreground", "popover.background", 4.5),
]

print("\n\n=== 对比度实测 (WCAG) ===")
problems = []
for name in ("RDS Light", "RDS Dark"):
    c = themes[name]["colors"]
    print(f"\n--- {name} ---")
    for fg, bg, need in PAIRS:
        if fg not in c or bg not in c:
            print(f"  {fg:32s} on {bg:28s}  [缺 token，走 gpui-kit 默认]")
            continue
        r = ratio(c[fg], c[bg])
        flag = "OK " if r >= need else "FAIL"
        if r < need:
            problems.append((name, fg, bg, round(r, 2), need))
        print(f"  {flag} {r:5.2f}:1 (需 {need})  {fg} on {bg}")

print("\n=== 不达标汇总 ===")
for p in problems:
    print(" ", p)
if not problems:
    print("  无")

# 4) clamp_alpha 影响：list/table active、selection 会被强制降 alpha
print("\n=== clamp_alpha 影响 (schema 强制 alpha<=0.2/0.3) ===")
for name in ("RDS Light", "RDS Dark"):
    c = themes[name]["colors"]
    for k in ("list.active.background", "table.active.background", "selection.background"):
        if k in c:
            v = c[k]
            a = 1.0 if len(v.lstrip("#")) <= 6 else int(v.lstrip("#")[6:8], 16) / 255
            print(f"  {name:11s} {k:26s} {v}  alpha={a:.2f} -> 渲染时被压到 0.2/0.3")


# 5) 产品语义 token（assets/themes/product-tokens.json）
# ⚠️ 这 9 个角色**不在** schema 的 140 键内（由 workbench_shell 单独加载为 GPUI global），
#    上面的覆盖率检查管不到它们；文件缺失 = 整块回退标准字段。此处补对比度校验。
PRODUCT = ROOT / "assets/themes/product-tokens.json"
pdata = json.loads(PRODUCT.read_text(encoding="utf-8"))

# (前景, 背景, 阈值, 依据) —— 阈值 3.0 = WCAG 1.4.11 非文本对比（图标 / 描边）；4.5 = 正文 AA
PRODUCT_PAIRS = [
    ("activity_bar.icon.active", "activity_bar.background", 3.0, "活动栏选中图标（非文本取 3:1）"),
    ("activity_bar.icon.inactive", "activity_bar.background", 3.0, "活动栏未选中图标"),
    ("activity_bar.active_border", "activity_bar.background", 3.0, "活动栏选中指示条"),
    ("foreground", "title_bar.slot.background", 4.5, "挖空槽内文字（假设取 foreground）"),
    ("foreground", "quick_open.group.header", 4.5, "Quick Open 组头（delegate.rs:354 实测取 foreground；2026-11-02 由 muted_foreground 改来）"),
    ("info.background", "quick_open.group.header", 4.5, "组头 note「搜索中…」文字色（delegate.rs:357 取 theme.colors.info = info.background）"),
    ("foreground", "search.match.background", 4.5, "搜索命中高亮底上的正文"),
    ("foreground", "result.local_row_background", 4.5, "本地行底色上的正文"),
    ("staged.drop", "background", 4.5, "staged.drop 被当**前景色**用（database/nav_view/rows.rs:2449,2548）"),
]

print("\n\n=== 产品语义 token 对比度（product-tokens.json）===")
pproblems = []
for name, mode in (("RDS Light", "light"), ("RDS Dark", "dark")):
    roles = pdata.get(mode, {})
    # 产品角色优先（同名覆盖主题字段），缺失的回退到主题资产的字段
    merged = dict(themes[name]["colors"])
    merged.update(roles)
    print(f"\n--- {name} ---  已定义角色 {len(roles)} 个")
    for fg, bg, need, why in PRODUCT_PAIRS:
        if fg not in merged or bg not in merged:
            print(f"  [缺 token] {fg} on {bg}  （{why}）")
            continue
        r = ratio(merged[fg], merged[bg])
        flag = "OK " if r >= need else "FAIL"
        if r < need:
            pproblems.append((name, fg, bg, round(r, 2), need, why))
        print(f"  {flag} {r:5.2f}:1 (需 {need})  {fg} on {bg}   [{why}]")

print("\n=== 产品 token 不达标汇总 ===")
for p in pproblems:
    print(" ", p)
if not pproblems:
    print("  无")


# 6) 语法高亮段：键名合法性 + 41 名覆盖 + 代码文字对比度
#
# 背景：SyntaxColors 是具名结构体，写未知键会被 serde **静默丢弃**（与 colors 同款陷阱）；
# 点号名（`keyword.return`）会回退到前缀，但没有前缀可退的名字会**完全不上色**。
REGISTRY = ROOT / "vendor/gpui-component/src/highlighter/registry.rs"
reg = REGISTRY.read_text(encoding="utf-8")
m = re.search(
    r"pub\(super\) const HIGHLIGHT_NAMES: \[&str; \d+\] = \[(.*?)\];", reg, re.S
)
hl_names = re.findall(r'"([^"]+)"', m.group(1))

print("\n\n=== 语法高亮（highlight.syntax）===")
print(f"[registry] HIGHLIGHT_NAMES 合法名总数: {len(hl_names)}")
hl_problems = []
for name in ("RDS Light", "RDS Dark"):
    hl = themes[name]["highlight"]
    syn = hl["syntax"]
    editor_bg = hl["editor.background"]
    unknown = [k for k in syn if k not in hl_names]

    def covered(n: str) -> bool:
        if n in syn:
            return True
        # 点号名回退到前缀（`registry.rs` 的 style() 行为）
        return "." in n and n.split(".")[0] in syn

    uncovered = [n for n in hl_names if not covered(n)]
    print(f"\n--- {name} ---  定义 {len(syn)} 个  非法键 {len(unknown)}  未覆盖 {len(uncovered)}")
    if unknown:
        print("  非法键（会被静默丢弃）:", unknown)
        hl_problems.append((name, "非法 syntax 键", unknown))
    if uncovered:
        print("  未覆盖（无前缀可退 → 完全不上色）:", uncovered)
        hl_problems.append((name, "未覆盖", uncovered))

    # 代码文字对比度：只校验显式给了 color 的项（emphasis 只给斜体，无颜色）
    for k in sorted(syn):
        c = syn[k].get("color")
        if not c:
            continue
        r = ratio(c, editor_bg)
        # 注释是刻意的低对比语义，按大文字/图形档 3:1 判定，其余按正文 4.5:1
        need = 3.0 if k.startswith("comment") else 4.5
        flag = "OK " if r >= need else "FAIL"
        if r < need:
            hl_problems.append((name, k, f"{r:.2f}:1 < {need}", editor_bg))
        print(f"  {flag} {r:5.2f}:1 (需 {need})  {k} {c} on editor.background {editor_bg}")

    # editor.* 自身对比度
    for fg, bg, need in (
        ("editor.foreground", "editor.background", 4.5),
        ("editor.active_line_number", "editor.active_line.background", 4.5),
        ("editor.line_number", "editor.background", 3.0),
    ):
        if fg in hl and bg in hl:
            r = ratio(hl[fg], hl[bg])
            flag = "OK " if r >= need else "FAIL"
            if r < need:
                hl_problems.append((name, fg, f"{r:.2f}:1 < {need}", hl[bg]))
            print(f"  {flag} {r:5.2f}:1 (需 {need})  {fg} on {bg}")

print("\n=== 语法高亮不达标汇总 ===")
for p in hl_problems:
    print(" ", p)
if not hl_problems:
    print("  无")
