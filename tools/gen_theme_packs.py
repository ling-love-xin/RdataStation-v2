#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把候选色板（tools/gen_palette_options.py 的 SCHEMES 角色口径）展开成「主题包」文件。

用法: python tools/gen_theme_packs.py

输出（每个候选一套包，一包 = Light + Dark + 一套产品 token）:
  docs/architecture/theme/palette-candidates/<id>-theme.json          可直接放进 assets/themes/
  docs/architecture/theme/palette-candidates/<id>-product-tokens.json 该包的产品语义 token

设计与口径：
- 角色 → 140 键的映射是本文件 KEY_ROLES 中的单表；键名集合与顺序取自现役
  assets/themes/rds-theme.json（140/140，键名合法性由 tools/audit_theme_packs.py 对照 schema 校验）。
- 由主色 / 灰阶派生出来的键（button 系列、scrollbar、base.* light 变体、语法补充名）都在
  derived() 里显式推导，不写死色值。
- 语法高亮补充名取 chart 系列色时，若在编辑器底上不足 4.5:1，按 2% 步长向对比方向微调，
  并打印调整记录（不静默处理）。
"""

import importlib.util
import json
import pathlib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parent
OUT_DIR = ROOT / "docs/architecture/theme/palette-candidates"
ASSET_DIR = ROOT / "assets/themes/packs"

_spec = importlib.util.spec_from_file_location("gpo", HERE / "gen_palette_options.py")
gpo = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gpo)

mix, alpha, ratio = gpo.mix, gpo.alpha, gpo.ratio

# ===== 角色 → 140 键映射（键名 / 顺序取自现役 rds-theme.json）=====

KEY_ROLES = {
    # 基础
    "background": "content",
    "foreground": "fg",
    "border": "border",
    "input.border": "input_border",
    "caret": "accent",
    "selection.background": "selection",
    "overlay": "overlay",
    "ring": "accent",
    "window.border": "border",
    "link": "accent",
    "link.hover": "accent_hover",
    "link.active": "accent_active",
    # 侧栏 / 标题栏 / 状态栏 / tab
    "sidebar.background": "chrome",
    "sidebar.foreground": "muted",
    "sidebar.border": "row_border",
    "sidebar.accent.background": "hover",
    "sidebar.accent.foreground": "fg_active",
    "sidebar.primary.background": "accent",
    "sidebar.primary.foreground": "accent_on",
    "title_bar.background": "chrome",
    "title_bar.border": "border",
    "status_bar.background": "status",
    "status_bar.border": "border",
    "tab.background": "chrome",
    "tab.foreground": "muted",
    "tab.active.background": "content",
    "tab.active.foreground": "fg_active",
    "tab_bar.background": "chrome",
    "tab_bar.segmented.background": "group",
    # 弹层 / 分组
    "popover.background": "popover",
    "popover.foreground": "fg",
    "group_box.background": "group",
    "group_box.foreground": "fg",
    "group_box.title.foreground": "muted",
    "accordion.background": "group",
    # 列表 / 表格
    "list.background": "content",
    "list.hover.background": "hover",
    "list.active.background": "soft",
    "list.active.border": "accent",
    "list.even.background": "zebra",
    "list.head.background": "zebra",
    "table.background": "content",
    "table.hover.background": "hover",
    "table.active.background": "soft",
    "table.active.border": "accent",
    "table.even.background": "zebra",
    "table.head.background": "head",
    "table.head.foreground": "head_fg",
    "table.foot.background": "group",
    "table.foot.foreground": "muted",
    "table.row.border": "row_border",
    "description_list.label.background": "group",
    "description_list.label.foreground": "muted",
    # 按钮
    "button.background": "button",
    "button.foreground": "fg",
    "button.hover.background": "button_hover",
    "button.active.background": "button_active",
    "button.primary.background": "accent",
    "button.primary.foreground": "accent_on",
    "button.primary.hover.background": "accent_hover",
    "button.primary.active.background": "accent_active",
    "button.secondary.background": "button",
    "button.secondary.foreground": "fg",
    "button.secondary.hover.background": "button_hover",
    "button.secondary.active.background": "button_active",
    "button.danger.background": "danger",
    "button.danger.foreground": "danger_on",
    "button.danger.hover.background": "danger_hover",
    "button.danger.active.background": "danger_active",
    "button.success.background": "success",
    "button.success.foreground": "success_on",
    "button.success.hover.background": "success_hover",
    "button.success.active.background": "success_active",
    "button.warning.background": "warning",
    "button.warning.foreground": "warning_on",
    "button.warning.hover.background": "warning_hover",
    "button.warning.active.background": "warning_active",
    "button.info.background": "info",
    "button.info.foreground": "info_on",
    "button.info.hover.background": "info_hover",
    "button.info.active.background": "info_active",
    # 语义
    "primary.background": "accent",
    "primary.foreground": "accent_on",
    "primary.hover.background": "accent_hover",
    "primary.active.background": "accent_active",
    "secondary.background": "button",
    "secondary.foreground": "fg",
    "secondary.hover.background": "button_hover",
    "secondary.active.background": "button_active",
    "accent.background": "soft",
    "accent.foreground": "fg",
    "muted.background": "hover",
    "muted.foreground": "muted",
    "success.background": "success",
    "success.foreground": "success_on",
    "success.hover.background": "success_hover",
    "success.active.background": "success_active",
    "danger.background": "danger",
    "danger.foreground": "danger_on",
    "danger.hover.background": "danger_hover",
    "danger.active.background": "danger_active",
    "warning.background": "warning",
    "warning.foreground": "warning_on",
    "warning.hover.background": "warning_hover",
    "warning.active.background": "warning_active",
    "info.background": "info",
    "info.foreground": "info_on",
    "info.hover.background": "info_hover",
    "info.active.background": "info_active",
    # 控件
    "scrollbar.background": "scrollbar_track",
    "scrollbar.thumb.background": "scroll_thumb",
    "scrollbar.thumb.hover.background": "scroll_thumb_hover",
    "skeleton.background": "hover",
    "slider.background": "input_border",
    "slider.thumb.background": "accent",
    "switch.background": "input_border",
    "switch.thumb.background": "switch_thumb",
    "progress.bar.background": "accent",
    "drag.border": "drag_border",
    "drop_target.background": "soft",
    # 图表
    "chart.1": "chart_1",
    "chart.2": "chart_2",
    "chart.3": "chart_3",
    "chart.4": "chart_4",
    "chart.5": "chart_5",
    "chart.bullish": "up",
    "chart.bearish": "down",
    "chart.grid": "grid",
    # base 调色板
    "base.blue": "base_blue",
    "base.blue.light": "base_blue_light",
    "base.cyan": "base_cyan",
    "base.cyan.light": "base_cyan_light",
    "base.green": "base_green",
    "base.green.light": "base_green_light",
    "base.magenta": "base_magenta",
    "base.magenta.light": "base_magenta_light",
    "base.red": "base_red",
    "base.red.light": "base_red_light",
    "base.yellow": "base_yellow",
    "base.yellow.light": "base_yellow_light",
}

# ===== 语法高亮：31 个具名条目（键名与现役资产一致；41 个合法名由 audit 校验覆盖）=====

SYNTAX_ROLES = {
    "comment": "syntax.comment",
    "string": "syntax.string",
    "number": "syntax.number",
    "boolean": "syntax.keyword",
    "constant": "syntax.keyword",
    "keyword": "syntax.keyword",
    "function": "syntax.function",
    "title": "syntax.function",
    "type": "syntax.type",
    "tag": "syntax.keyword",
    "property": "syn_property",
    "variable": "syn_property",
    "attribute": "syn_property",
    "label": "syn_property",
    "string.special": "syn_special",
    "operator": "fg",
    "punctuation": "fg",
    "constructor": "syntax.function",
    "enum": "syntax.type",
    "variant": "syntax.type",
    "preproc": "syn_preproc",
    "embedded": "fg",
    "hint": "muted",
    "predictive": "muted",
    "primary": "fg",
    "link_text": "accent",
    "link_uri": "muted",
    "text.literal": "syntax.string",
    "text.code.span": "syntax.string",
}
SYNTAX_EXTRA = {"emphasis": {"font_style": "italic"},
                "emphasis.strong": {"font_weight": 700}}

ADJUST_LOG = []


def role_of(e, name):
    if name.startswith("syntax."):
        return e["syntax"][name.split(".", 1)[1]]
    return e[name]


def ensure_text(color, bg, need, mode, label):
    """语法补充名取图表色时，按 2% 步长向对比方向微调，直到达到 need。"""
    cur = color
    target = "#000000" if mode == "light" else "#FFFFFF"
    for _ in range(13):
        if ratio(cur, bg) >= need:
            break
        cur = mix(cur, target, 0.02)
    if cur != color:
        ADJUST_LOG.append((label, color, cur, round(ratio(cur, bg), 2)))
    return cur


def derived(r, mode):
    """由角色值推导的键（不写死色值）。"""
    e = dict(r)
    e["chart_1"], e["chart_2"], e["chart_3"], e["chart_4"], e["chart_5"] = r["chart"]
    e["fg_active"] = r["fg"] if mode == "light" else mix(r["fg"], "#FFFFFF", 0.55)
    # 中性按钮：浅色取状态栏底、深色取表头底（在 chrome 上略抬一档）。
    # hover/active 从按钮自身派生而不是复用 hover：部分色板的 status 与 hover 几乎同亮度，
    # 直接拿 hover 当按钮 hover 会让按钮按下去「没有反应」（分层强度自检抓过这一点）。
    e["button"] = r["status"] if mode == "light" else r["head"]
    e["button_hover"] = (mix(r["status"], "#000000", 0.05) if mode == "light"
                         else mix(r["head"], r["fg"], 0.10))
    e["button_active"] = (mix(r["status"], "#000000", 0.11) if mode == "light"
                          else mix(r["head"], r["fg"], 0.22))
    e["scrollbar_track"] = alpha(r["content"], 0.0)
    e["scroll_thumb"] = alpha(mix(r["chrome"], r["fg"], 0.42), 0.9)
    e["scroll_thumb_hover"] = mix(r["chrome"], r["fg"], 0.55)
    e["switch_thumb"] = r["content"] if mode == "light" else mix(r["fg"], "#FFFFFF", 0.9)
    e["drag_border"] = alpha(r["accent"], 0.65)
    # base.* light 变体：明色向白提 35%，暗色向白提 25%
    t = 0.35 if mode == "light" else 0.25
    e["base_blue"], e["base_blue_light"] = r["chart"][0], mix(r["chart"][0], "#FFFFFF", t)
    e["base_cyan"], e["base_cyan_light"] = r["chart"][1], mix(r["chart"][1], "#FFFFFF", t)
    e["base_green"], e["base_green_light"] = r["up"], mix(r["up"], "#FFFFFF", t)
    e["base_magenta"], e["base_magenta_light"] = r["chart"][4], mix(r["chart"][4], "#FFFFFF", t)
    e["base_red"], e["base_red_light"] = r["down"], mix(r["down"], "#FFFFFF", 0.3 if mode == "light" else 0.2)
    e["base_yellow"], e["base_yellow_light"] = r["chart"][3], mix(r["chart"][3], "#FFFFFF", t)
    # 语法补充名
    e["syn_property"] = r["chart"][0]
    e["syn_preproc"] = r["chart"][4]
    e["syn_special"] = r["danger"]
    return e


def build_pack(scheme):
    """返回 (theme_json, product_tokens_json)。"""
    themes = []
    products = {}
    for mode, is_default in (("light", True), ("dark", False)):
        roles = gpo.expand(scheme[mode], mode)
        e = derived(roles, mode)
        editor_bg = e["content"]
        REPS = ("comment", "string", "number", "keyword", "type", "function")
        for k, src in SYNTAX_ROLES.items():
            v = role_of(e, src)
            if k not in REPS:
                need = 3.0 if k.startswith("comment") else 4.5
                v = ensure_text(v, editor_bg, need, mode, f"{scheme['id']}/{mode}/{k}")
            e["syn_" + k] = v

        colors = {k: e[role] for k, role in KEY_ROLES.items()}
        syntax = {k: {"color": e["syn_" + k]} for k in SYNTAX_ROLES}
        syntax.update(SYNTAX_EXTRA)
        highlight = {
            "editor.foreground": e["fg"],
            "editor.background": editor_bg,
            "editor.active_line.background": e["group"],
            "editor.line_number": e["subtle"],
            "editor.active_line_number": e["fg_active"],
            "syntax": syntax,
        }
        themes.append({
            "name": f"RDS {scheme['name']} {'Light' if mode == 'light' else 'Dark'}",
            "mode": mode,
            "is_default": is_default,
            "font.size": 16,
            "font.family": ".SystemUIFont",
            "mono_font.size": 13,
            "colors": colors,
            "highlight": highlight,
        })
        products[mode] = {
            "activity_bar.background": e["chrome"],
            "activity_bar.active_border": e["fg_active"],
            "activity_bar.icon.active": e["fg_active"],
            "activity_bar.icon.inactive": e["muted"],
            "title_bar.slot.background": e["content"],
            "quick_open.group.header": e["quick"],
            "search.match.background": e["search"],
            "staged.drop": e["danger"],
            "result.odd_row.background": e["odd"],
            "result.cell.modified.background": e["modified"],
            "result.cell.error.background": e["error_cell"],
            "result.local_row_background": e["local_row"],
            "result.header.border": e["border"],
        }

    theme_json = {
        "$schema": "https://github.com/longbridge/gpui-component/raw/refs/heads/main/.theme-schema.json",
        "name": f"RdataStation {scheme['cn']}",
        "author": "RDS",
        "url": "https://rds.local",
        "$comment": (
            f"候选主题包「{scheme['cn']} {scheme['name']}」：一 Light + 一 Dark + 一套产品 token。"
            "由 tools/gen_theme_packs.py 从 docs/architecture/theme/palette-options.md 的色板角色展开为 140 键；"
            "对比度由 tools/audit_theme_packs.py 校验。运行时副本在 assets/themes/packs/<id>/（子目录，"
            "不会被 ThemeRegistry 自动注册），用环境变量 RDS_THEME_PACK=<id> 启用。"
        ),
        "themes": themes,
    }
    product_json = {
        "$comment": (
            f"「{scheme['cn']}」主题包的产品语义 token（13 角色）。换包时必须与主题文件同步替换，"
            "否则活动栏底色 / Quick Open 组头 / 搜索命中底 / 结果集槽会残留默认珊瑚包的值。"
        ),
        "light": products["light"],
        "dark": products["dark"],
    }
    return theme_json, product_json


def main():
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    total_keys = None
    for scheme in gpo.SCHEMES:
        SYNTAX_ROLES_COLOR.clear()
        theme_json, product_json = build_pack(scheme)
        n_keys = len(theme_json["themes"][0]["colors"])
        total_keys = n_keys
        p1 = OUT_DIR / f"{scheme['id']}-theme.json"
        p2 = OUT_DIR / f"{scheme['id']}-product-tokens.json"
        theme_txt = json.dumps(theme_json, ensure_ascii=False, indent=2) + "\n"
        prod_txt = json.dumps(product_json, ensure_ascii=False, indent=2) + "\n"
        p1.write_text(theme_txt, encoding="utf-8")
        p2.write_text(prod_txt, encoding="utf-8")

        # 运行时副本：assets/themes/packs/<id>/{theme.json, product-tokens.json}
        # （子目录不会被 ThemeRegistry::reload / load_theme_assets 扫到，默认不生效）
        adir = ASSET_DIR / scheme["id"]
        adir.mkdir(parents=True, exist_ok=True)
        (adir / "theme.json").write_text(theme_txt, encoding="utf-8")
        (adir / "product-tokens.json").write_text(prod_txt, encoding="utf-8")

        print(f"{scheme['id']:15s} 140键 {n_keys}  产品角色 {len(product_json['light'])}"
              f"  -> docs/{p1.name} + assets/themes/packs/{scheme['id']}/")
    if ADJUST_LOG:
        print("\n语法补充名对比度微调（chart 色 -> 达标色）:")
        for lab, old, new, rr in ADJUST_LOG:
            print(f"  {lab:34s} {old} -> {new}  {rr}:1")
    else:
        print("\n语法补充名无需微调")
    print(f"\n输出目录 {OUT_DIR}（{len(gpo.SCHEMES)} 个包，每包 {total_keys} 键）")


SYNTAX_ROLES_COLOR = {}

if __name__ == "__main__":
    main()
