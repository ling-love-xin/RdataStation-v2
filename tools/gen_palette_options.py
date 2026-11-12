#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""生成「主题色板候选」色卡预览页与说明文档。

用法: python tools/gen_palette_options.py
输出:
  docs/architecture/theme/palette-options.html   色卡预览（每套明暗并排 + mini IDE 样张 + 对比度）
  docs/architecture/theme/palette-options.md     色值表 / 对比度口径 / 落地映射 / 选型建议

数据在本文件 SCHEMES 里（单一来源）；改色后重跑即可，页面与文档同源。
对比度口径与 tools/theme_audit.py 一致（WCAG 2.x，正文 4.5:1 / 非文本 3:1）。
"""

import datetime
import pathlib
ROOT = pathlib.Path(__file__).resolve().parents[1]
OUT_HTML = ROOT / "docs/architecture/theme/palette-options.html"
OUT_MD = ROOT / "docs/architecture/theme/palette-options.md"


# ===== 颜色数学（与 theme_audit.py / gen_theme_preview.py 同口径）=====


def to_rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


def to_hex(rgb):
    return "#" + "".join(f"{max(0, min(255, int(round(c)))):02X}" for c in rgb)


def lin(c):
    c = c / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def lum(h):
    h = h.lstrip("#")[:6]
    r, g, b = (int(h[i:i + 2], 16) for i in (0, 2, 4))
    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)


def ratio(a, b):
    la, lb = lum(a[:7]), lum(b[:7])
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


def mix(a, b, t):
    ra, ga, ba = to_rgb(a)
    rb, gb, bb = to_rgb(b)
    return to_hex((ra * (1 - t) + rb * t, ga * (1 - t) + gb * t, ba * (1 - t) + bb * t))


def alpha(h, a):
    return h + f"{int(round(a * 255)):02X}"


def over(fg, bg):
    """把带 alpha 的前景合成到背景（选中态可读性用）。"""
    f = fg.lstrip("#")
    b = bg.lstrip("#")
    if len(f) != 8:
        return fg
    a = int(f[6:8], 16) / 255.0
    rgb = tuple(int(f[i:i + 2], 16) * a + int(b[i:i + 2], 16) * (1 - a) for i in (0, 2, 4))
    return to_hex(rgb)


# ===== 候选色板 =====
#
# 角色口径（对应 140 token 的语义切片，见文档 §落地映射）：
#   content 内容区 / chrome 外层 chrome / popover 弹层 / status 状态栏 / group 分组容器
#   head 表头 / zebra 斑马行 / hover 悬停行
#   fg 正文 / muted 次要文字 / subtle 占位 / head_fg 表头文字
#   border 描边 / input_border 输入框描边 / row_border 行分隔
#   accent 主色（hover/active/soft/selection 由主色派生）
#   success/warning/danger/info 各带 on（实心底上的文字）
#   chart 5 系列 / up·down 涨跌 / grid 网格
#   odd·modified·error_cell·local_row 结果集四槽 / search 命中 / quick Quick Open 组头
#   syntax 语法高亮 6 个代表名（其余沿用 VS Code 对应色板）

SCHEMES = [
    {
        "id": "coral-graphite",
        "name": "Coral Graphite",
        "cn": "珊瑚石墨",
        "tag": "接现役品牌 · 中性骨架",
        "position": "中性灰骨架 + coral 品牌。沿用现役 140 token 的分层，把 chrome 与内容区再拉开半档，状态栏保持中性。主色在现役 #BC5340 基础上再压 2.5%（#B94F3B）：现役值放在 #F7F7F8 的侧栏底上只有 4.41:1。",
        "fit": "默认候选：分层与现役资产一致，迁移成本最低，图标 / 启动图等品牌资产不用动；唯一改动是主色压暗一档，肉眼几乎不可分辨。",
        "light": {
            "content": "#FFFFFF", "chrome": "#F7F7F8", "status": "#F2F2F3",
            "popover": "#FFFFFF", "group": "#F5F5F6", "head": "#F6F6F7",
            "head_fg": "#5E5E63", "zebra": "#FAFAFB", "hover": "#EFEFF0",
            "fg": "#333336", "muted": "#6A6A6F", "subtle": "#8C8C91",
            "border": "#D5D5D8", "input_border": "#AEAEB3", "row_border": "#E3E3E5",
            "accent": "#B94F3B", "accent_on": "#FFFFFF",
            "success": "#15803D", "success_on": "#FFFFFF",
            "warning": "#A16207", "warning_on": "#FFFFFF",
            "danger": "#B91C1C", "danger_on": "#FFFFFF",
            "info": "#1D4ED8", "info_on": "#FFFFFF",
            "chart": ["#2563EB", "#0891B2", "#BC5340", "#A9660A", "#9333EA"],
            "up": "#15803D", "down": "#DC2626", "grid": "#E3E3E5",
            "odd": "#EEF4FB", "modified": "#FFE9D2", "error_cell": "#FFE1EC",
            "local_row": "#E8F7D2", "search": "#FFF3C4", "quick": "#ECECEE",
            "overlay": "#00000073",
            "syntax": {"comment": "#008000", "keyword": "#0B4BC1", "string": "#A31515",
                        "number": "#098658", "type": "#267F99", "function": "#795E26"},
        },
        "dark": {
            "content": "#1E1E1E", "chrome": "#171717", "status": "#171717",
            "popover": "#232323", "group": "#1A1A1A", "head": "#303338",
            "head_fg": "#A7A7AC", "zebra": "#252527", "hover": "#2B2D2F",
            "fg": "#CFCFD2", "muted": "#96969B", "subtle": "#6B6B70",
            "border": "#3B3B3E", "input_border": "#4E4E52", "row_border": "#3B3B3E",
            "accent": "#E8846F", "accent_on": "#1E1E1E",
            "success": "#89D185", "success_on": "#0C1F14",
            "warning": "#E3B341", "warning_on": "#241C02",
            "danger": "#F14C4C", "danger_on": "#2A0C0C",
            "info": "#75BEFF", "info_on": "#0A1626",
            "chart": ["#3794FF", "#4EC9B0", "#E8846F", "#CCA700", "#C586C0"],
            "up": "#89D185", "down": "#F14C4C", "grid": "#3B3B3E",
            "odd": "#1C2129", "modified": "#3A3120", "error_cell": "#4A2131",
            "local_row": "#2A3A1E", "search": "#4A3F00", "quick": "#3A3D41",
            "overlay": "#00000080",
            "syntax": {"comment": "#6A9955", "keyword": "#569CD6", "string": "#CE9178",
                        "number": "#B5CEA8", "type": "#4EC9B0", "function": "#DCDCAA"},
        },
    },
    {
        "id": "slate-azure",
        "name": "Slate Azure",
        "cn": "石板深蓝",
        "tag": "冷灰蓝骨架 · 蓝主色",
        "position": "冷调蓝灰中性阶 + azure 主色，接近 DataGrip / TablePlus 一类数据工具的观感；info 让位给青，避免与主色撞色。",
        "fit": "结果集密集、长时段盯表时冷底比纯中性灰更稳；与数据库工具的用户预期一致。代价：品牌要重做，且主色与 info 同族需按文档口径分离。",
        "light": {
            "content": "#FFFFFF", "chrome": "#F2F5F8", "status": "#EBF0F5",
            "popover": "#FFFFFF", "group": "#F1F5F9", "head": "#EDF3F8",
            "head_fg": "#546270", "zebra": "#F7FAFC", "hover": "#E8F0F7",
            "fg": "#232B35", "muted": "#546170", "subtle": "#788797",
            "border": "#CFD8E2", "input_border": "#AAB9C6", "row_border": "#DDE6EF",
            "accent": "#1E63D0", "accent_on": "#FFFFFF",
            "success": "#15803D", "success_on": "#FFFFFF",
            "warning": "#A16207", "warning_on": "#FFFFFF",
            "danger": "#B91C1C", "danger_on": "#FFFFFF",
            "info": "#0D6E89", "info_on": "#FFFFFF",
            "chart": ["#1E63D0", "#0E8898", "#C2691F", "#5B7A1E", "#8C3FA8"],
            "up": "#15803D", "down": "#C0392B", "grid": "#DDE6EF",
            "odd": "#E9F2FB", "modified": "#FFEBD6", "error_cell": "#FFE0E6",
            "local_row": "#E4F7E0", "search": "#E6F4FF", "quick": "#E3EBF3",
            "overlay": "#00000073",
            "syntax": {"comment": "#00705A", "keyword": "#0B4BC1", "string": "#A31515",
                        "number": "#096B52", "type": "#267F99", "function": "#795E26"},
        },
        "dark": {
            "content": "#161A1F", "chrome": "#101418", "status": "#101418",
            "popover": "#1C2229", "group": "#141920", "head": "#232D38",
            "head_fg": "#9AA9B8", "zebra": "#1A2027", "hover": "#212A34",
            "fg": "#C9D3DD", "muted": "#8B99A7", "subtle": "#65727F",
            "border": "#2E3843", "input_border": "#414D5A", "row_border": "#2E3843",
            "accent": "#4E93FF", "accent_on": "#0B1420",
            "success": "#7ED9A6", "success_on": "#082018",
            "warning": "#E7BB55", "warning_on": "#241B03",
            "danger": "#FF7B72", "danger_on": "#2A0E0C",
            "info": "#6CC7E0", "info_on": "#06202A",
            "chart": ["#6FA6FF", "#4ECDC4", "#E59A5E", "#A6C46A", "#C08FD6"],
            "up": "#7ED9A6", "down": "#FF7B72", "grid": "#2E3843",
            "odd": "#1B2632", "modified": "#33280F", "error_cell": "#3A1A24",
            "local_row": "#1D2E1A", "search": "#123A47", "quick": "#27313C",
            "overlay": "#00000080",
            "syntax": {"comment": "#7C9A8C", "keyword": "#7CB4FF", "string": "#D7A98A",
                        "number": "#9ED1B4", "type": "#6FD0D8", "function": "#E2D59C"},
        },
    },
    {
        "id": "warm-umber",
        "name": "Warm Umber",
        "cn": "暖纸赭石",
        "tag": "暖灰骨架 · 赭石主色",
        "position": "暖纸白 / 暖炭黑中性阶 + 赭石主色，底色带一点黄，长文与报表阅读更柔和。",
        "fit": "以 EDA 报表、文档式检查器、Mock 数据浏览为主的使用场景。暖底对细网格更宽容；代价是纯黑 OLED 观感与技术冷感会丢，图表色需整体偏暖一档。",
        "light": {
            "content": "#FEFCF8", "chrome": "#F4EEE5", "status": "#EDE6DA",
            "popover": "#FFFDFA", "group": "#F3ECE1", "head": "#F0E9DD",
            "head_fg": "#5A4E3F", "zebra": "#F9F4EC", "hover": "#EFE6D8",
            "fg": "#332B22", "muted": "#6B5C4B", "subtle": "#8E7E6B",
            "border": "#D9CDBB", "input_border": "#B9A78F", "row_border": "#E6DAC7",
            "accent": "#A25423", "accent_on": "#FFFFFF",
            "success": "#2F7A3E", "success_on": "#FFFFFF",
            "warning": "#8A6408", "warning_on": "#FFFFFF",
            "danger": "#B3301F", "danger_on": "#FFFFFF",
            "info": "#22608F", "info_on": "#FFFFFF",
            "chart": ["#2B5E8C", "#0E7C86", "#A25423", "#6E7A18", "#8C3A5F"],
            "up": "#2F7A3E", "down": "#B3301F", "grid": "#E6DAC7",
            "odd": "#F3EEE3", "modified": "#FFE6C8", "error_cell": "#FBE0DC",
            "local_row": "#E7F0DA", "search": "#FDF0C8", "quick": "#E9E1D4",
            "overlay": "#1A120A73",
            "syntax": {"comment": "#00693A", "keyword": "#12459A", "string": "#9C2B12",
                        "number": "#0A6046", "type": "#1F6B78", "function": "#6E5010"},
        },
        "dark": {
            "content": "#1B1815", "chrome": "#141210", "status": "#141210",
            "popover": "#241F1A", "group": "#171411", "head": "#2C2621",
            "head_fg": "#B2A493", "zebra": "#211D19", "hover": "#2B2621",
            "fg": "#D8D0C4", "muted": "#9B8E7D", "subtle": "#756A5C",
            "border": "#38312A", "input_border": "#4B4136", "row_border": "#38312A",
            "accent": "#E39B5F", "accent_on": "#1A1109",
            "success": "#9BD391", "success_on": "#0C1F0B",
            "warning": "#E9C063", "warning_on": "#231A02",
            "danger": "#FF8A7A", "danger_on": "#2A0F0A",
            "info": "#8FC3E8", "info_on": "#071A26",
            "chart": ["#8AAED6", "#5FC6C2", "#E39B5F", "#CBB35F", "#CE93A8"],
            "up": "#9BD391", "down": "#FF8A7A", "grid": "#38312A",
            "odd": "#241F18", "modified": "#3D2C16", "error_cell": "#3F1D18",
            "local_row": "#2A3320", "search": "#4A3A12", "quick": "#332C25",
            "overlay": "#00000080",
            "syntax": {"comment": "#809B72", "keyword": "#8FB4E8", "string": "#DFA07C",
                        "number": "#9CC79A", "type": "#7FC6C0", "function": "#DCCB8C"},
        },
    },
    {
        "id": "forest-teal",
        "name": "Forest Teal",
        "cn": "松林青绿",
        "tag": "绿灰骨架 · 松青主色",
        "position": "带绿倾向的中性阶 + 松青主色（teal），冷而不蓝；状态绿与主色靠明度档位区分，不靠色相。",
        "fit": "偏「数据洞察 / EDA」气质的方案：青绿在深底上比蓝更耐看，也避开最常见的成功绿。风险是语义绿与主色同族，必须保持档位差。",
        "light": {
            "content": "#FFFFFF", "chrome": "#F1F6F3", "status": "#EAF0EC",
            "popover": "#FFFFFF", "group": "#F0F5F2", "head": "#ECF3EF",
            "head_fg": "#4F615A", "zebra": "#F6FAF8", "hover": "#E7F1EC",
            "fg": "#22302B", "muted": "#52655E", "subtle": "#788C85",
            "border": "#CBDAD4", "input_border": "#A6B8B1", "row_border": "#DCE8E2",
            "accent": "#0E6B58", "accent_on": "#FFFFFF",
            "success": "#2E7D32", "success_on": "#FFFFFF",
            "warning": "#8A6408", "warning_on": "#FFFFFF",
            "danger": "#B3301F", "danger_on": "#FFFFFF",
            "info": "#25639B", "info_on": "#FFFFFF",
            "chart": ["#25639B", "#0E6B58", "#C2622B", "#8A7A1E", "#8A3F8F"],
            "up": "#2E7D32", "down": "#B3301F", "grid": "#DCE8E2",
            "odd": "#EAF4EE", "modified": "#FBEBD8", "error_cell": "#F7DEE1",
            "local_row": "#E3F2DC", "search": "#E8F5E2", "quick": "#E4EDE8",
            "overlay": "#00000073",
            "syntax": {"comment": "#0A6B3F", "keyword": "#0B4BC1", "string": "#A31515",
                        "number": "#0A6046", "type": "#1F6B78", "function": "#795E26"},
        },
        "dark": {
            "content": "#171C1A", "chrome": "#111513", "status": "#111513",
            "popover": "#1D2421", "group": "#141917", "head": "#23302B",
            "head_fg": "#9DB0A8", "zebra": "#1B221F", "hover": "#232C28",
            "fg": "#CAD5CF", "muted": "#8B9C94", "subtle": "#657670",
            "border": "#2D3833", "input_border": "#404E47", "row_border": "#2D3833",
            "accent": "#45C9A5", "accent_on": "#06231C",
            "success": "#8FD79C", "success_on": "#0A1F12",
            "warning": "#E5BE58", "warning_on": "#231C02",
            "danger": "#FF7F73", "danger_on": "#2A0E0B",
            "info": "#7BC0F5", "info_on": "#061A26",
            "chart": ["#6FA8D6", "#45C9A5", "#DE9A66", "#C9C463", "#C194D6"],
            "up": "#8FD79C", "down": "#FF7F73", "grid": "#2D3833",
            "odd": "#1C2521", "modified": "#3A2C15", "error_cell": "#3D1D1C",
            "local_row": "#21331E", "search": "#163A2C", "quick": "#2A3630",
            "overlay": "#00000080",
            "syntax": {"comment": "#7FA98F", "keyword": "#7CB4FF", "string": "#D7A98A",
                        "number": "#9ED1B4", "type": "#6FD0D8", "function": "#E2D59C"},
        },
    },
    {
        "id": "ink-oled",
        "name": "Ink OLED",
        "cn": "近黑墨",
        "tag": "纯黑暗色 · 高对比明色",
        "position": "暗色直接落 #000（OLED 省电 + 极简），亮色改用高对比版（近黑文字 / 深描边）。主色仍是 coral，但在黑底上提亮到 #FF8F76。",
        "fit": "低光环境、外接 OLED 屏、或按 WCAG 接近 AAA 读正文的场景。代价：chrome 与内容区没有灰度差，层级完全靠描边，斑马纹也必须更细。",
        "light": {
            "content": "#FFFFFF", "chrome": "#F0F0F0", "status": "#E8E8E8",
            "popover": "#FFFFFF", "group": "#F4F4F4", "head": "#EDEDED",
            "head_fg": "#3A3A3A", "zebra": "#F7F7F7", "hover": "#E4E4E4",
            "fg": "#141414", "muted": "#474747", "subtle": "#6E6E6E",
            "border": "#ADADAD", "input_border": "#7A7A7A", "row_border": "#C9C9C9",
            "accent": "#A03420", "accent_on": "#FFFFFF",
            "success": "#0F6B33", "success_on": "#FFFFFF",
            "warning": "#8A5A00", "warning_on": "#FFFFFF",
            "danger": "#A31B1B", "danger_on": "#FFFFFF",
            "info": "#1846B8", "info_on": "#FFFFFF",
            "chart": ["#1348B0", "#0A6E7E", "#A03420", "#7A5A00", "#6E2B96"],
            "up": "#0F6B33", "down": "#A31B1B", "grid": "#C9C9C9",
            "odd": "#EDF3FA", "modified": "#FFE7CC", "error_cell": "#FFDDE4",
            "local_row": "#E5F5D9", "search": "#FFF0B0", "quick": "#E2E2E2",
            "overlay": "#00000080",
            "syntax": {"comment": "#005F2E", "keyword": "#0A3694", "string": "#8F1A0F",
                        "number": "#04543A", "type": "#14606E", "function": "#5E4408"},
        },
        "dark": {
            "content": "#000000", "chrome": "#000000", "status": "#000000",
            "popover": "#111111", "group": "#0A0A0A", "head": "#1C1C1C",
            "head_fg": "#C4C4C4", "zebra": "#0C0C0C", "hover": "#1E1E1E",
            "fg": "#EDEDED", "muted": "#A6A6A6", "subtle": "#7A7A7A",
            "border": "#3A3A3A", "input_border": "#575757", "row_border": "#262626",
            "accent": "#FF8F76", "accent_on": "#140A06",
            "success": "#9BE39C", "success_on": "#08210C",
            "warning": "#F2CE66", "warning_on": "#241C02",
            "danger": "#FF8A80", "danger_on": "#290B0B",
            "info": "#8CCBFF", "info_on": "#061726",
            "chart": ["#6FA8FF", "#5FD6C0", "#FF8F76", "#E0C04A", "#C9A0E8"],
            "up": "#9BE39C", "down": "#FF8A80", "grid": "#262626",
            "odd": "#101820", "modified": "#33260F", "error_cell": "#33151C",
            "local_row": "#152513", "search": "#4A3800", "quick": "#262626",
            "overlay": "#00000099",
            "syntax": {"comment": "#6E9B6E", "keyword": "#7CC0FF", "string": "#E0A57F",
                        "number": "#93D4A8", "type": "#62D2D8", "function": "#E6D79C"},
        },
    },
    # ---------- 中国风 ----------
    {
        "id": "qinghua",
        "name": "Qinghua",
        "cn": "青花",
        "tag": "瓷白 + 钴蓝",
        "style": "cn",
        "position": "瓷白底 + 青花钴蓝主色，暗色转「夜青花」（墨青底 + 月白蓝）。传统色里蓝与白最克制的搭配，接近数据库工具本身的冷静气质。",
        "fit": "想要“东方但不过分装饰”的内置中国风首选；info 让位给青瓷绿（#17607A），否则会和钴蓝主色读成同一个蓝。",
        "light": {
            "content": "#FCFDFE", "chrome": "#EEF3F7", "status": "#E6EDF3",
            "popover": "#FFFFFF", "group": "#F1F6FA", "head": "#E9F0F6",
            "head_fg": "#3E5468", "zebra": "#F6F9FC", "hover": "#E5EEF6",
            "fg": "#1E2A35", "muted": "#4C6073", "subtle": "#7A8FA2",
            "border": "#C5D5E2", "input_border": "#A2B7C8", "row_border": "#D7E4EE",
            "accent": "#1B4B8A", "accent_on": "#FFFFFF",
            "success": "#2C6B45", "success_on": "#FFFFFF",
            "warning": "#8A5E0D", "warning_on": "#FFFFFF",
            "danger": "#A83325", "danger_on": "#FFFFFF",
            "info": "#17607A", "info_on": "#FFFFFF",
            "chart": ["#2A5FA8", "#1E7F7A", "#B2532A", "#7C6A16", "#7B4E86"],
            "up": "#2C6B45", "down": "#A83325", "grid": "#D7E4EE",
            "odd": "#EAF1F8", "modified": "#FBEBD5", "error_cell": "#F9E3E6",
            "local_row": "#E6F1E7", "search": "#FDF1CB", "quick": "#E3EBF4",
            "overlay": "#00000073",
            "syntax": {"comment": "#34675A", "keyword": "#14417F", "string": "#9B2C17",
                        "number": "#1C6047", "type": "#1B6072", "function": "#6B4E12"},
        },
        "dark": {
            "content": "#131A24", "chrome": "#0E141C", "status": "#0E141C",
            "popover": "#1A232E", "group": "#111822", "head": "#1F2C3A",
            "head_fg": "#A9BDCE", "zebra": "#17202B", "hover": "#1E2A36",
            "fg": "#C7D5E2", "muted": "#8CA0B2", "subtle": "#65798A",
            "border": "#2A3947", "input_border": "#3D4F60", "row_border": "#2A3947",
            "accent": "#6FA9E8", "accent_on": "#07131F",
            "success": "#7FD0A6", "success_on": "#062016",
            "warning": "#E7BE5E", "warning_on": "#241B04",
            "danger": "#FF8873", "danger_on": "#2A0D09",
            "info": "#6EC9D8", "info_on": "#052027",
            "chart": ["#7BAAD9", "#5FC7BE", "#E09A6A", "#C7B35E", "#B896C9"],
            "up": "#7FD0A6", "down": "#FF8873", "grid": "#2A3947",
            "odd": "#141D27", "modified": "#332714", "error_cell": "#3A1B1E",
            "local_row": "#17291F", "search": "#4A3C0E", "quick": "#24313F",
            "overlay": "#00000099",
            "syntax": {"comment": "#7FA593", "keyword": "#8FB6E8", "string": "#DFA07C",
                        "number": "#9CC79A", "type": "#72C4D2", "function": "#DCCB8C"},
        },
    },
    {
        "id": "zhusha",
        "name": "Zhusha Ink",
        "cn": "朱砂",
        "tag": "朱砂 + 宣纸 / 徽墨",
        "style": "cn",
        "position": "宣纸暖底 + 朱砂主色，暗色转「徽墨」（墨底 + 提亮朱砂）。朱砂传统上用于印章与批注，正好对应工作台里的“强调 + 标记”。",
        "fit": "最有辨识度的一套中国风：朱砂与危险红同族，danger 已压到绛色（浅 #8E1B12 / 深 #FF6B6B），靠明度而非色相区分；表单校验场景要在真机上确认一眼能分清。",
        "light": {
            "content": "#FCFAF6", "chrome": "#F2ECE4", "status": "#EBE4D8",
            "popover": "#FFFDFA", "group": "#F1EBE1", "head": "#EDE6DA",
            "head_fg": "#4A3A2C", "zebra": "#F8F4EC", "hover": "#EFE7DA",
            "fg": "#2A241D", "muted": "#5C4E40", "subtle": "#83725F",
            "border": "#D6C8B6", "input_border": "#B5A38A", "row_border": "#E4D9C8",
            "accent": "#A33A22", "accent_on": "#FFFFFF",
            "success": "#2C6B45", "success_on": "#FFFFFF",
            "warning": "#8A5E0D", "warning_on": "#FFFFFF",
            "danger": "#8E1B12", "danger_on": "#FFFFFF",
            "info": "#1F5570", "info_on": "#FFFFFF",
            "chart": ["#2A5A85", "#1F6E63", "#B4432C", "#7A6A1A", "#7B4066"],
            "up": "#2C6B45", "down": "#8E1B12", "grid": "#E4D9C8",
            "odd": "#F4EEE3", "modified": "#FBE6CF", "error_cell": "#F6DDDA",
            "local_row": "#E7F0DF", "search": "#FBEFC8", "quick": "#EAE2D5",
            "overlay": "#1A120A73",
            "syntax": {"comment": "#3B6B4E", "keyword": "#16406E", "string": "#8F2A18",
                        "number": "#1E5A41", "type": "#1D5E6B", "function": "#6A4D12"},
        },
        "dark": {
            "content": "#171410", "chrome": "#110F0C", "status": "#110F0C",
            "popover": "#201B16", "group": "#14110D", "head": "#2A241D",
            "head_fg": "#B3A492", "zebra": "#1C1813", "hover": "#25201A",
            "fg": "#D6CABA", "muted": "#98897A", "subtle": "#726558",
            "border": "#332C25", "input_border": "#463D33", "row_border": "#332C25",
            "accent": "#E2735B", "accent_on": "#1A0B07",
            "success": "#93D2A4", "success_on": "#0A2112",
            "warning": "#E5BE6A", "warning_on": "#241B04",
            "danger": "#FF6B6B", "danger_on": "#2A0D0D",
            "info": "#8CC7E0", "info_on": "#071C26",
            "chart": ["#8AAED6", "#5FC6C2", "#E2735B", "#C7B35E", "#CE93A8"],
            "up": "#93D2A4", "down": "#FF6B6B", "grid": "#332C25",
            "odd": "#241F18", "modified": "#3D2C15", "error_cell": "#3A1C1A",
            "local_row": "#2A3320", "search": "#4A3A12", "quick": "#332C25",
            "overlay": "#00000099",
            "syntax": {"comment": "#87A07A", "keyword": "#8FB4E8", "string": "#DFA07C",
                        "number": "#9CC79A", "type": "#7FC6C0", "function": "#DCCB8C"},
        },
    },
    {
        "id": "mopin",
        "name": "Ink Gold",
        "cn": "墨金",
        "tag": "墨 + 缃金",
        "style": "cn",
        "position": "宣纸底 + 墨色文字 + 缃金主色，暗色是真正的墨黑 + 提亮金。金只出现在主色与强调条上，不做大面积铺色。",
        "fit": "想要“深色为主、金线点睛”的内置中国风；warning 已改成赭橙（#A6511B / #E98A5A），否则会和金色主色读成同一个暖色。",
        "light": {
            "content": "#FAF9F5", "chrome": "#F1EEE7", "status": "#E9E6DD",
            "popover": "#FEFDFB", "group": "#F0EDE5", "head": "#ECE9E0",
            "head_fg": "#4A4436", "zebra": "#F7F6F0", "hover": "#EDEAE1",
            "fg": "#262419", "muted": "#58523F", "subtle": "#7E7862",
            "border": "#D0CABB", "input_border": "#ADA68F", "row_border": "#E0DACB",
            "accent": "#825F10", "accent_on": "#FFFFFF",
            "success": "#2C6B45", "success_on": "#FFFFFF",
            "warning": "#A6511B", "warning_on": "#FFFFFF",
            "danger": "#A0301F", "danger_on": "#FFFFFF",
            "info": "#1E5573", "info_on": "#FFFFFF",
            "chart": ["#2F5F8C", "#1F6E63", "#B07A1E", "#8E5A2C", "#6E4A86"],
            "up": "#2C6B45", "down": "#A0301F", "grid": "#E0DACB",
            "odd": "#F2F0E8", "modified": "#FBE8CF", "error_cell": "#F7DEDA",
            "local_row": "#E8F1DF", "search": "#FAEFC4", "quick": "#E8E3D6",
            "overlay": "#1A160A73",
            "syntax": {"comment": "#4A6B3A", "keyword": "#254E7A", "string": "#93301C",
                        "number": "#1F5A42", "type": "#1E5C68", "function": "#7A5A10"},
        },
        "dark": {
            "content": "#12110E", "chrome": "#0C0B09", "status": "#0C0B09",
            "popover": "#1B1916", "group": "#0F0E0B", "head": "#26231C",
            "head_fg": "#B0A68F", "zebra": "#171510", "hover": "#201D17",
            "fg": "#D3CBBA", "muted": "#948A78", "subtle": "#6E6659",
            "border": "#302B22", "input_border": "#433C30", "row_border": "#302B22",
            "accent": "#D9B45A", "accent_on": "#14100A",
            "success": "#93D2A4", "success_on": "#0A2112",
            "warning": "#E98A5A", "warning_on": "#241303",
            "danger": "#FF7E72", "danger_on": "#2A0E0B",
            "info": "#8CC7E0", "info_on": "#071C26",
            "chart": ["#8AAED6", "#5FC6C2", "#D9B45A", "#E09A6A", "#C194D6"],
            "up": "#93D2A4", "down": "#FF7E72", "grid": "#302B22",
            "odd": "#1D1B16", "modified": "#3A2E13", "error_cell": "#3A1D1A",
            "local_row": "#29331F", "search": "#4A3C10", "quick": "#2A2620",
            "overlay": "#00000099",
            "syntax": {"comment": "#8A9B6E", "keyword": "#95B8E0", "string": "#DFA07C",
                        "number": "#9CC79A", "type": "#7FC6C0", "function": "#DCCB8C"},
        },
    },
]


def expand(roles, mode):
    """补出 hover / active / soft / selection（向黑或白混合，保持色相稳定）。"""
    r = dict(roles)
    if mode == "light":
        mixhover, mixactive = ("#000000", 0.08), ("#000000", 0.17)
    else:
        mixhover, mixactive = ("#FFFFFF", 0.10), ("#000000", 0.05)
    for k in ("accent", "success", "warning", "danger", "info"):
        r[k + "_hover"] = mix(r[k], mixhover[0], mixhover[1])
        r[k + "_active"] = mix(r[k], mixactive[0], mixactive[1])
    r["soft"] = alpha(r["accent"], 0.2)
    r["selection"] = alpha(r["accent"], 0.3)
    return r


ROLE_GROUPS = [
    ("表面 / Chrome", [
        ("chrome", "chrome（侧栏·活动栏·标题栏·tab 栏）"),
        ("content", "内容区（编辑器 / 列表 / 表格底）"),
        ("popover", "弹层"),
        ("status", "状态栏"),
        ("group", "分组容器"),
        ("head", "表头底"),
        ("zebra", "斑马行"),
        ("hover", "悬停行"),
    ]),
    ("文字", [
        ("fg", "正文"),
        ("muted", "次要文字"),
        ("head_fg", "表头文字"),
        ("subtle", "占位 / 灰化"),
    ]),
    ("描边", [
        ("border", "描边"),
        ("input_border", "输入框描边"),
        ("row_border", "行分隔"),
    ]),
    ("品牌强调", [
        ("accent", "主色"),
        ("accent_hover", "主色 hover"),
        ("accent_active", "主色 active"),
        ("accent_on", "主色上的文字"),
        ("soft", "选中底（α 0.2）"),
        ("selection", "文本选区（α 0.3）"),
        ("overlay", "模态遮罩"),
    ]),
    ("语义", [
        ("success", "成功"),
        ("success_on", "成功上的文字"),
        ("warning", "警告"),
        ("warning_on", "警告上的文字"),
        ("danger", "危险"),
        ("danger_on", "危险上的文字"),
        ("info", "信息"),
        ("info_on", "信息上的文字"),
    ]),
    ("图表", [
        ("chart.1", "系列 1"),
        ("chart.2", "系列 2"),
        ("chart.3", "系列 3"),
        ("chart.4", "系列 4"),
        ("chart.5", "系列 5"),
        ("up", "涨 / 正向"),
        ("down", "跌 / 负向"),
        ("grid", "网格线"),
    ]),
    ("产品槽", [
        ("odd", "结果集斑马行"),
        ("modified", "已修改单元格"),
        ("error_cell", "错误单元格"),
        ("local_row", "本地 / 新增行"),
        ("search", "搜索命中底"),
        ("quick", "Quick Open 组头"),
    ]),
    ("语法高亮（代表名）", [
        ("syntax.comment", "注释"),
        ("syntax.keyword", "关键字"),
        ("syntax.string", "字符串"),
        ("syntax.number", "数字"),
        ("syntax.type", "类型"),
        ("syntax.function", "函数"),
    ]),
]


def value(r, key):
    if key.startswith("chart."):
        return r["chart"][int(key.split(".")[1]) - 1]
    if key.startswith("syntax."):
        return r["syntax"][key.split(".")[1]]
    return r[key]


def checks(r, mode):
    """返回 [(用途, 前景 hex, 背景 hex, 比值, 阈值)]。"""
    out = []

    def add(fg, bg, need, label):
        fh = r[fg] if isinstance(fg, str) and fg in r else fg
        bh = r[bg] if isinstance(bg, str) and bg in r else bg
        out.append((label, fh, bh, ratio(fh, bh), need))

    add("fg", "content", 4.5, "正文 on 内容区")
    add("fg", "chrome", 4.5, "正文 on chrome")
    add("fg", "popover", 4.5, "正文 on 弹层")
    add("fg", "group", 4.5, "正文 on 分组容器")
    add("fg", "hover", 4.5, "正文 on 悬停行")
    add("fg", "head", 4.5, "正文 on 表头（极端情形）")
    add("muted", "content", 4.5, "次要文字 on 内容区")
    add("muted", "chrome", 4.5, "次要文字 on chrome")
    add("muted", "group", 4.5, "次要文字 on 分组容器")
    add("muted", "status", 4.5, "状态栏文字（组件硬编码 muted）")
    add("muted", "popover", 4.5, "次要文字 on 弹层")
    add("head_fg", "head", 4.5, "表头文字")
    add("accent", "content", 4.5, "主色文字 / 链接 on 内容区")
    add("accent_hover", "content", 4.5, "主色 hover（链接 hover）")
    add("accent_active", "content", 4.5, "主色 active（链接 active）")
    add("accent", "chrome", 4.5, "主色文字 on chrome")
    add("accent_on", "accent", 4.5, "主色按钮文字")
    add("accent_on", "accent_hover", 4.5, "主色 hover")
    add("accent_on", "accent_active", 4.5, "主色 active")
    add("accent", "content", 3.0, "焦点环（取主色，非文本 3:1）")
    for k, lab in (("success", "成功"), ("warning", "警告"), ("danger", "危险"), ("info", "信息")):
        add(k + "_on", k, 4.5, f"{lab}按钮文字")
        add(k + "_on", k + "_hover", 4.5, f"{lab} hover")
        add(k + "_on", k + "_active", 4.5, f"{lab} active")
        add(k, "content", 4.5, f"{lab}文字 on 内容区")
    for i, c in enumerate(r["chart"], 1):
        out.append((f"chart.{i}", c, r["content"], ratio(c, r["content"]), 3.0))
    add("up", "content", 3.0, "涨 / 正向 on 内容区")
    add("down", "content", 3.0, "跌 / 负向 on 内容区")
    for k, lab in (("odd", "结果集斑马行"), ("modified", "已修改单元格"),
                   ("error_cell", "错误单元格"), ("local_row", "本地 / 新增行"),
                   ("search", "搜索命中底"), ("quick", "Quick Open 组头")):
        out.append((f"正文 on {lab}", r["fg"], r[k], ratio(r["fg"], r[k]), 4.5))
    out.append(("组头 note（info 当文字）", r["info"], r["quick"], ratio(r["info"], r["quick"]), 4.5))
    comp = over(r["soft"], r["content"])
    out.append(("正文 on 选中底（α 0.2 合成）", r["fg"], comp, ratio(r["fg"], comp), 4.5))
    comp2 = over(r["selection"], r["content"])
    out.append(("正文 on 文本选区（α 0.3 合成）", r["fg"], comp2, ratio(r["fg"], comp2), 4.5))
    for k, lab, need in (("comment", "语法·注释", 3.0), ("keyword", "语法·关键字", 4.5),
                         ("string", "语法·字符串", 4.5), ("number", "语法·数字", 4.5),
                         ("type", "语法·类型", 4.5), ("function", "语法·函数", 4.5)):
        out.append((lab, r["syntax"][k], r["content"], ratio(r["syntax"][k], r["content"]), need))
    return out


# ===== HTML 渲染 =====

css = """
  * { margin:0; padding:0; box-sizing:border-box; }
  body { background:#141414; color:#cfcfcf; font-family:"Microsoft YaHei","Segoe UI",sans-serif; padding:22px 26px 40px; }
  h1 { font-size:17px; color:#fff; font-weight:600; }
  h2 { font-size:13px; color:#fff; font-weight:600; margin-bottom:2px; }
  .sub { font-size:11px; color:#9a9a9a; line-height:1.7; margin:6px 0 18px; }
  .sum { width:100%; border-collapse:collapse; font-size:11px; margin-bottom:26px; }
  .sum th { text-align:left; color:#e6e6e6; font-weight:600; padding:5px 8px; border-bottom:1px solid #3c3c3c; }
  .sum td { padding:5px 8px; border-bottom:1px solid #2a2a2a; color:#bfbfbf; vertical-align:top; }
  .sum .hx { font-family:Consolas,monospace; }
  .scheme { margin-bottom:30px; }
  .scheme-head { display:flex; flex-wrap:wrap; align-items:baseline; gap:8px; margin-bottom:4px; }
  .scheme-head .tag { font-size:10px; color:#9a9a9a; border:1px solid #3c3c3c; border-radius:9px; padding:1px 7px; }
  .scheme-head .id { font-size:10px; color:#7f7f7f; font-family:Consolas,monospace; }
  .pos { font-size:11px; color:#a8a8a8; line-height:1.6; margin-bottom:2px; }
  .fit { font-size:11px; color:#8f8f8f; line-height:1.6; margin-bottom:10px; }
  .cols { display:flex; gap:16px; align-items:flex-start; }
  .panel { flex:1; min-width:430px; border:1px solid #3c3c3c; border-radius:8px; overflow:hidden; }
  .phead { font-size:12px; font-weight:600; padding:7px 12px; display:flex; justify-content:space-between; border-bottom:1px solid; }
  .phead .n { font-size:10px; font-weight:400; opacity:.65; }
  .win { margin:10px 12px 0; border-radius:6px; overflow:hidden; font-size:9px; }
  .tbar { display:flex; align-items:center; height:24px; padding:0 7px; gap:6px; }
  .logo { width:11px; height:11px; border-radius:3px; }
  .cutout { padding:1px 7px; border-radius:4px; font-size:8px; }
  .body { display:flex; height:150px; }
  .act { width:20px; display:flex; flex-direction:column; align-items:center; padding-top:5px; gap:7px; }
  .act .on { width:100%; text-align:center; box-shadow:inset 2px 0 0; padding-left:2px; }
  .side { width:86px; padding:5px 4px; line-height:1.9; border-right:1px solid; }
  .side .row { display:block; padding:0 4px; border-radius:2px; white-space:nowrap; overflow:hidden; }
  .side .sel { box-shadow:inset 2px 0 0; }
  .main { flex:1; display:flex; flex-direction:column; }
  .editor { padding:6px 8px; font-family:Consolas,monospace; line-height:1.65; flex:1; }
  .tbl { font-size:8px; }
  .tbl .th { display:flex; padding:3px 6px; border-bottom:1px solid; }
  .tbl .tr { display:flex; padding:2px 6px; }
  .tbl .c { flex:1; white-space:nowrap; overflow:hidden; }
  .sbar { display:flex; align-items:center; height:17px; padding:0 7px; font-size:8px; justify-content:space-between; }
  .ramps { padding:8px 12px 12px; }
  .rname { font-size:10px; font-weight:600; margin:9px 0 4px; }
  .chips { display:grid; grid-template-columns:repeat(auto-fill,minmax(74px,1fr)); gap:4px; }
  .chip { border-radius:4px; overflow:hidden; border:1px solid; }
  .chip .c { height:26px; }
  .chip .m { padding:2px 4px; font-size:8px; line-height:1.3; }
  .chip .m .k { display:block; opacity:.8; }
  .chip .m .v { display:block; font-family:Consolas,monospace; }
  details { padding:0 12px 12px; }
  details summary { font-size:10px; font-weight:600; cursor:pointer; padding:4px 0; }
  .chk { width:100%; border-collapse:collapse; font-size:9px; }
  .chk td, .chk th { padding:2px 4px; text-align:left; border-bottom:1px solid rgba(128,128,128,.22); }
  .ok { color:#7BD88F; } .bad { color:#FF7A7A; font-weight:600; }
  .note { font-size:11px; color:#8a8a8a; line-height:1.8; border-top:1px solid #333; padding-top:14px; margin-top:8px; }
  .grp { margin:26px 0 12px; border-top:1px solid #3c3c3c; padding-top:14px; }
  .grp-h { font-size:15px; color:#fff; font-weight:600; display:flex; gap:8px; align-items:baseline; margin:0; }
  .grp-h span { font-size:11px; color:#8a8a8a; font-weight:400; }
  .grp-d { font-size:11px; color:#8f8f8f; margin:4px 0 2px; line-height:1.6; }
"""


def chip_html(label, hexv, bg_for_meta, fg_for_meta, border):
    return (
        f'<div class="chip" style="border-color:{border}">'
        f'<div class="c" style="background:{hexv}"></div>'
        f'<div class="m" style="background:{bg_for_meta};color:{fg_for_meta}">'
        f'<span class="k">{label}</span><span class="v">{hexv}</span></div></div>'
    )


def preview_html(r, mode):
    chrome = r["chrome"]
    content = r["content"]
    return f"""
  <div class="win" style="border:1px solid {r['border']}">
    <div class="tbar" style="background:{chrome};color:{r['muted']};border-bottom:1px solid {r['border']}">
      <span class="logo" style="background:{r['accent']}"></span><span>RdataStation</span>
      <span class="cutout" style="background:{content};color:{r['fg']};border:1px solid {r['border']}">本地 · 项目库</span>
    </div>
    <div class="body">
      <div class="act" style="background:{chrome};color:{r['subtle']};border-right:1px solid {r['border']}">
        <span class="on" style="color:{r['accent']};box-shadow:inset 2px 0 0 {r['accent']}">▤</span>
        <span>◇</span><span>◈</span><span>⌗</span>
      </div>
      <div class="side" style="background:{chrome};color:{r['muted']};border-right-color:{r['border']}">
        <span class="row">▾ PostgreSQL</span>
        <span class="row sel" style="background:{r['soft']};color:{r['fg']};box-shadow:inset 2px 0 0 {r['accent']}">public.orders</span>
        <span class="row">public.users</span>
        <span class="row">▾ MySQL</span>
        <span class="row">▾ DuckDB</span>
      </div>
      <div class="main">
        <div class="editor" style="background:{content};color:{r['fg']}">
          <div><span style="color:{r['syntax']['keyword']}">SELECT</span> <span style="color:{r['syntax']['type']}">order_id</span>, <span style="color:{r['syntax']['function']}">sum</span>(amount)</div>
          <div style="padding-left:10px"><span style="color:{r['syntax']['keyword']}">FROM</span> orders</div>
          <div style="padding-left:10px"><span style="color:{r['syntax']['keyword']}">GROUP BY</span> 1</div>
          <div style="color:{r['syntax']['comment']}">-- 12 rows · 34 ms</div>
        </div>
        <div class="tbl" style="border-top:1px solid {r['border']}">
          <div class="th" style="background:{r['head']};color:{r['head_fg']};border-bottom-color:{r['row_border']}">
            <span class="c">order_id</span><span class="c">amount</span><span class="c">status</span>
          </div>
          <div class="tr" style="background:{content};color:{r['fg']}"><span class="c">1001</span><span class="c">280.00</span><span class="c">paid</span></div>
          <div class="tr" style="background:{r['zebra']};color:{r['fg']}"><span class="c">1002</span><span class="c" style="background:{r['modified']}">310.00</span><span class="c">paid</span></div>
          <div class="tr" style="background:{content};color:{r['fg']}"><span class="c">1003</span><span class="c">96.00</span><span class="c" style="background:{r['error_cell']}">timeout</span></div>
        </div>
        <div class="sbar" style="background:{r['status']};color:{r['muted']};border-top:1px solid {r['border']}">
          <span>就绪 · duckdb</span>
          <span><span class="cutout" style="background:{r['accent']};color:{r['accent_on']}">UTF-8</span> Ln 4, Col 12</span>
        </div>
      </div>
    </div>
  </div>"""


def panel_html(s, mode):
    r = expand(s[mode], mode)
    cks = checks(r, mode)
    bad = [c for c in cks if c[3] < c[4]]
    ramps = ""
    for gname, items in ROLE_GROUPS:
        chips = "".join(
            chip_html(lab, value(r, key), r["content"], r["fg"], r["border"])
            for key, lab in items
        )
        ramps += f'<div class="rname" style="color:{r["fg"]};border-bottom:1px solid {r["border"]};padding-bottom:2px">{gname}</div><div class="chips">{chips}</div>'
    rows = "".join(
        f'<tr><td>{lab}</td><td class="hx">{fh}<br>{bg}</td>'
        f'<td class="{"ok" if rt >= need else "bad"}">{rt:.2f}:1</td></tr>'
        for lab, fh, bg, rt, need in cks
    )
    label = "Light" if mode == "light" else "Dark"
    pass_n = len(cks) - len(bad)
    return f"""
<div class="panel" style="background:{r['content']};border-color:{r['border']}">
  <div class="phead" style="background:{r['chrome']};color:{r['fg']};border-color:{r['border']}">
    <span>{label}</span><span class="n">{pass_n}/{len(cks)} 项对比度达标</span>
  </div>
  {preview_html(r, mode)}
  <div class="ramps" style="background:{r['content']}">{ramps}</div>
  <details style="background:{r['content']}">
    <summary style="color:{r['fg']}">对比度明细（{len(cks)} 项 · {'全部达标' if not bad else str(len(bad)) + ' 项不达标'}）</summary>
    <table class="chk"><tr><th>用途</th><th>前景 / 背景</th><th>比值</th></tr>{rows}</table>
  </details>
</div>"""


STYLES = {
    "modern": ("现代中性", "中性 / 冷 / 暖灰骨架 + 各自主色，按使用场景选；品牌色可保持不变"),
    "cn": ("中国风", "传统色系：青花钴蓝、朱砂、墨金；瓷白 / 宣纸 / 墨底，明暗成对"),
}


def scheme_style(s):
    return s.get("style", "modern")


def scheme_block(s):
    return f"""
<div class="scheme">
  <div class="scheme-head"><h2>{s["cn"]}</h2><span style="font-size:12px;color:#bdbdbd">{s["name"]}</span>
    <span class="tag">{s["tag"]}</span><span class="id">{s["id"]}</span></div>
  <div class="pos">{s["position"]}</div>
  <div class="fit">{s["fit"]}</div>
  <div class="cols">{panel_html(s, "light")}{panel_html(s, "dark")}</div>
</div>"""


def build_html():
    summary_rows = []
    total_checks = 0
    total_fail = 0
    for s in SCHEMES:
        light = expand(s["light"], "light")
        dark = expand(s["dark"], "dark")
        n_l = len(checks(light, "light"))
        bad_l = sum(1 for c in checks(light, "light") if c[3] < c[4])
        bad_d = sum(1 for c in checks(dark, "dark") if c[3] < c[4])
        total_checks += n_l * 2
        total_fail += bad_l + bad_d
        summary_rows.append(
            f'<tr><td>{STYLES[scheme_style(s)][0]}</td>'
            f'<td><b>{s["cn"]}</b><br><span class="hx" style="font-size:10px;color:#8a8a8a">{s["name"]}</span></td>'
            f'<td>{s["tag"]}</td>'
            f'<td><span class="hx">{s["light"]["accent"]}</span> / <span class="hx">{s["dark"]["accent"]}</span></td>'
            f'<td class="hx">{s["light"]["content"]} / {s["dark"]["content"]}</td>'
            f'<td>{"全部达标" if bad_l + bad_d == 0 else f"{bad_l + bad_d} 项不达标"}</td></tr>'
        )

    blocks = []
    for style, (label, desc) in STYLES.items():
        group = [s for s in SCHEMES if scheme_style(s) == style]
        if not group:
            continue
        blocks.append(
            f'<div class="grp"><h2 class="grp-h">{label}<span>{len(group)} 套 · 明暗成对</span></h2>'
            f'<p class="grp-d">{desc}</p></div>'
        )
        blocks.extend(scheme_block(s) for s in group)

    now = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
    html = f"""<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>RdataStation 主题色板候选 · {len(SCHEMES)} 套明暗色卡</title>
<style>{css}</style>
</head>
<body>
<h1>RdataStation 主题色板候选 · {len(SCHEMES)} 套（明 / 暗）</h1>
<p class="sub">
  由 <code>tools/gen_palette_options.py</code> 生成（{now}）—— 色值与对比度同源，改色后重跑即可。<br>
  角色口径对齐 <code>assets/themes/rds-theme.json</code> 的 140 token 语义切片；对比度按 WCAG 2.x（正文 4.5:1、非文本 3:1），
  与 <code>tools/theme_audit.py</code> 同算法。合计 {total_checks} 项配对，{total_checks - total_fail} 项达标。
</p>
<table class="sum">
  <tr><th>风格</th><th>方案</th><th>定位</th><th>主色 明 / 暗</th><th>内容区 明 / 暗</th><th>对比度</th></tr>
  {"".join(summary_rows)}
</table>
{"".join(blocks)}
<p class="note">
  读法：每套左 Light、右 Dark，各带一张 mini IDE 样张（标题栏 / 活动栏 / 侧栏 / 编辑器 / 结果集 / 状态栏），
  用来看 chrome 与内容区的分档、选中态、表头与斑马纹是否成立；色块区是可直接抄用的角色值。<br>
  选中底（α 0.2）与文本选区（α 0.3）是按 gpui-kit <code>clamp_alpha</code> 上限给的，超过会被静默压缩，表现为「选中看不见」。<br>
  语义色是「实心底 + 高对比文字」：浅色模式深底白字、深色模式亮底极深字，与现役资产同口径。<br>
  落地：每套已展开成 140 键的包文件（<code>docs/architecture/theme/palette-candidates/</code>，主题 + 产品 token 成对），
  由 <code>tools/gen_theme_packs.py</code> 生成、<code>tools/audit_theme_packs.py</code> 校验（8 包 × 明暗 0 不达标）。
</p>
</body>
</html>
"""
    return html, total_checks, total_fail


# ===== Markdown =====

MAPPING = """| 角色（本文件） | 对应 140 token 键 | 备注 |
| --- | --- | --- |
| `content` | `background` / `list.background` / `table.background` / `highlight.editor.background` | 编辑区 = 列表底 = 表格底，同值 |
| `chrome` | `sidebar.background` / `title_bar.background` / `tab.background` / `tab_bar.background` + 产品 token `activity_bar.background` | 外层 chrome 同底，靠激活条/图标区分 |
| `status` | `status_bar.background` | 必须中性：组件文字硬编码 `muted.foreground` |
| `popover` | `popover.background` / `popover.foreground` | 弹层比内容区高一档（暗色）/ 同白（亮色） |
| `group` | `group_box.background` / `accordion.background` | 分组容器比内容区再沉一档 |
| `head` / `head_fg` | `table.head.background` / `table.head.foreground` | 结果集表头 |
| `zebra` | `list.even.background` / `table.even.background` | 斑马行 |
| `hover` | `list.hover.background` / `table.hover.background` / `button.secondary.hover.background` | 悬停（不覆盖选中） |
| `fg` | `foreground` / `popover.foreground` / `list` 文字 | 正文 |
| `muted` | `muted.foreground` / `sidebar.foreground` / `tab.foreground` / `table.head.foreground` | 次要文字 |
| `border` / `input_border` / `row_border` | `border` / `window.border` / `input.border` / `table.row.border` | 描边三档 |
| `accent` | `primary.background` / `button.primary.*` / `link` / `caret` / `ring` / `slider.thumb.background` / `progress.bar.background` | 主色统一驱动 |
| `accent_on` | `primary.foreground` / `button.primary.foreground` / `sidebar.primary.foreground` | 实心底上的文字 |
| `soft` | `list.active.background` / `table.active.background` / `accent.background` | α 取 0.2（clamp 上限） |
| `selection` | `selection.background` | α 取 0.3（clamp 上限） |
| `accent_hover` / `accent_active` | `primary.hover.background` / `primary.active.background`（按钮同名键同理） | 由主色向黑/白混合派生 |
| 语义四色 | `success.*` / `warning.*` / `danger.*` / `info.*`（含按钮同名前缀） | `.on` → `*.foreground` |
| `chart.*` | `chart.1`–`chart.5` / `chart.bullish` / `chart.bearish` / `chart.grid` | 系列色与涨跌色分离 |
| `odd` / `modified` / `error_cell` / `local_row` | 产品 token `result.odd_row.background` 等 4 槽 | 结果集高亮槽 |
| `search` / `quick` | 产品 token `search.match.background` / `quick_open.group.header` | 命中底 / 分组头 |
| `overlay` | `overlay` | 模态遮罩 + Quick Open 遮罩共用（无 scrim 键） |"""


BUILTIN_NOTE = """> 三条都是本机源码实测（2026-11-12），不是推测。

1. **加载端本来就支持多主题**：`crates/app/src/main.rs::load_theme_assets` 扫 `assets/themes/*.json`，把每个文件里的 `themes[]` 全部注册进 `ThemeRegistry`（`product-tokens.json` / `theme-keys.json` 除外）。所以多套主题**文件本身就能被加载**，不用改加载逻辑。
2. **切换端只有一对明暗槽**：`Theme` 只有 `light_theme` / `dark_theme` 两个字段，`Theme::change(mode)` 只按模式应用这一对（`vendor/gpui-component/src/theme/mod.rs:388-406`，注释明确“调用方换过 `light_theme`/`dark_theme` 后，即便 mode 不变，change 也会重新应用”）；而 `attach_rds_theme` 目前按名字**硬编码** `"RDS Light"` / `"RDS Dark"`（`main.rs:649-661`）。换另一套 = 同时替换这两个字段再 `change`，API 支持，但要新增“主题包”概念与选择 UI。
3. **产品 token 没有主题维度**：`assets/themes/product-tokens.json` 只有 `light` / `dark` 两个映射，按模式解析成一个全局（`crates/workbench_shell/src/product_tokens.rs:17-59`）。换第二套主题后，活动栏底色、Quick Open 组头、搜索命中底、结果集四个高亮槽**仍是珊瑚那套的值** —— 表面只换了一半，且不报错。

**结论**：内置可以，但先定两条规矩 ——

- 主题以**包**为单位：一包 = 一个 Light + 一个 Dark + 一套产品 token，不做散装单套；
- 包名对外后**不能再改**：`attach_rds_theme` 与设置持久化都按名字查，改名等于用户配置失效。

建议节奏：内置 **2 个包**（默认 RDS + 1 个中国风），其余当可选包；要上第 3 个包之前，先把 `ProductTokenSet` 升级成“按包解析”，否则每多一包就多一处“看着只换了一半”的静默不一致。

> 这一节说的“包”已经按 §4.1 生成好了（8 个包，每个 = 主题文件 + 产品 token 文件，运行时副本在
> `assets/themes/packs/<id>/`）；剩下的只是“选哪包 + 改解析口径”。

**试切换（opt-in，默认完全不变）**

- 入口：环境变量 `RDS_THEME_PACK=<包 id>`，例如 `qinghua`、`zhusha`、`mopin`。
  不设 / 设成 `rds` / `default` → 走现役 `rds-theme.json` + `product-tokens.json`，行为零变化。
- 代码：`crates/app/src/theme_pack.rs`（选包 + 按 `mode` 读出一对配置，不依赖主题名）；
  `main.rs` 的 `attach_rds_theme` / `attach_product_tokens` 在设了变量时优先读包，包不可用则
  打一行 `[startup]` 日志并回落现役主题，不 panic、不阻塞启动。
- 为什么必须「主题 + 产品 token 一起换」：产品 token 只按明暗模式解析、没有主题维度，
  只换主题会让活动栏底色 / Quick Open 组头 / 搜索命中底 / 结果集四槽残留默认珊瑚包的值。
- 自检：`cargo test -p rds-app theme_pack` —— 断言每个出厂包都能读出 Light+Dark、字号 16、
  140 键、13 个产品角色齐全（包缺文件 / 少键会直接失败）。
- **还没做的**：主题选择 UI 与设置持久化（现在只有环境变量）；`ProductTokenSet` 仍是
  “按模式”的单例，多包**并存**切换需要先升级成按包解析。
"""


def build_md(total_checks, total_fail):
    now = datetime.datetime.now().strftime("%Y-%m-%d %H:%M")
    per_mode = total_checks // (len(SCHEMES) * 2)
    n_all = len(SCHEMES)
    groups = [(st, [s for s in SCHEMES if scheme_style(s) == st]) for st in STYLES]
    groups = [(st, g) for st, g in groups if g]
    lines = []
    lines.append(f"# RdataStation 主题色板候选（{n_all} 套 · 明暗）\n")
    lines.append(
        f"> 状态：**候选方案，未落地**（不改 `assets/themes/`）。生成于 {now}，"
        "由 `python tools/gen_palette_options.py` 输出本文件与 `palette-options.html`。\n"
        "> 色值权威性：本文件是候选色板的唯一来源；采纳某套后再并入 "
        "`theme-tokens-complete.md` 与 `rds-theme.json`。\n"
        f"> 验证口径：WCAG 2.x，正文/控件文字 ≥ 4.5:1、非文本 ≥ 3:1；"
        f"单套单模式 {per_mode} 项，{n_all} 套 × 明暗共 {total_checks} 项，**{total_checks - total_fail} 项达标**。\n"
    )
    lines.append("## 1. 方案一览\n")
    lines.append("| 风格 | 方案 | 定位 | 主色（明 / 暗） | 内容区（明 / 暗） | 适用 |")
    lines.append("| --- | --- | --- | --- | --- | --- |")
    for s in SCHEMES:
        lines.append(
            f"| {STYLES[scheme_style(s)][0]} | **{s['cn']}** `{s['name']}` | {s['tag']} "
            f"| `{s['light']['accent']}` / `{s['dark']['accent']}` "
            f"| `{s['light']['content']}` / `{s['dark']['content']}` | {s['fit']} |"
        )
    lines.append("")

    sec = 2
    for style, group in groups:
        label = STYLES[style][0]
        lines.append(f"## {sec}. {label}（{len(group)} 套）\n")
        lines.append(f"{STYLES[style][1]}。\n")
        for sub, s in enumerate(group, 1):
            rl = expand(s["light"], "light")
            rd = expand(s["dark"], "dark")
            bad = [c for c in checks(rl, "light") if c[3] < c[4]] + [c for c in checks(rd, "dark") if c[3] < c[4]]
            lines.append(f"### {sec}.{sub} {s['cn']} `{s['name']}` — {s['tag']}\n")
            lines.append(f"{s['position']}\n")
            lines.append(f"**适用**：{s['fit']}\n")
            for gname, items in ROLE_GROUPS:
                lines.append(f"**{gname}**\n")
                lines.append("| 角色 | Light | Dark |")
                lines.append("| --- | --- | --- |")
                for key, lab in items:
                    lines.append(f"| {lab} | `{value(rl, key)}` | `{value(rd, key)}` |")
                lines.append("")
            if bad:
                lines.append(f"**对比度**：不达标 {len(bad)} 项 —— " + "；".join(
                    f"{c[0]} {c[3]:.2f}:1（需 {c[4]}）" for c in bad
                ) + "\n")
            else:
                lines.append(f"**对比度**：明暗各 {per_mode} 项全部达标（明细见 `palette-options.html`）。\n")
        sec += 1

    lines.append(f"## {sec}. 角色 → 140 token 落地映射\n")
    lines.append("采纳任意一套时，按这张表把角色值铺到 `assets/themes/rds-theme.json`；"
                 "新增/改色后跑 `python tools/theme_audit.py` 与 `cargo test -p rds-ui-guard`。\n")
    lines.append(MAPPING + "\n")

    lines.append(f"### {sec}.1 已生成的包文件（140 键，可直接落地）\n")
    lines.append("每个包 = 一个 Light + 一个 Dark + 一套产品 token（13 角色）。同一份内容有两处副本："
                 "`docs/architecture/theme/palette-candidates/`（可读复审用）与 "
                 "`assets/themes/packs/<id>/`（运行时目录，`theme.json` + `product-tokens.json`）：\n")
    lines.append("| 包 | docs 复审副本 | 运行时目录 |")
    lines.append("| --- | --- | --- |")
    for s in SCHEMES:
        lines.append(f"| {s['cn']} `{s['id']}` | `{s['id']}-theme.json` / `{s['id']}-product-tokens.json` "
                     f"| `assets/themes/packs/{s['id']}/` |")
    lines.append("")
    lines.append("> 为什么放子目录：`ThemeRegistry::reload` 与 `load_theme_assets` 都只扫第一层 `*.json`，"
                 "`packs/` 子目录天然不注册——候选包不会出现在主题列表里，也不影响默认行为。"
                 "启用方式见 §5 的「试切换」。\n")

    lines.append(f"### {sec}.2 生成与校验\n")
    lines.append("- 生成：`python tools/gen_theme_packs.py` —— 从本文件 `SCHEMES` 的角色值展开 140 键 + 13 个产品角色，"
                 "派生的键（button 系列、scrollbar、`base.*` light 变体、语法补充名）在脚本里显式推导。")
    lines.append("- 校验：`python tools/audit_theme_packs.py` —— 复用 `tools/theme_audit.py` 的 schema 键集合 / 52 对 / "
                 "9 对产品 / 41 个语法名口径，逐包逐模式跑。")
    lines.append("- 当前结果：**8 包 × 明暗全部通过**（140/140 键、主题对照 0 不达标、产品对照 0 不达标、"
                 "41 个语法名 0 未覆盖、语法着色 0 不达标）。\n")

    lines.append(f"### {sec}.3 与现役资产的有意差异\n")
    lines.append("`audit_theme_packs.py` 会把「现役资产里同值、候选里拉开（通道差 ≥ 4）」的组逐条列出，"
                 "当前 **9 组**（8 条说明：表头文字在明暗两套各出现一次），均为有意区分：\n")
    lines.append("| 现役同值的组 | 候选里的区别 | 为什么 |")
    lines.append("| --- | --- | --- |")
    lines.append("| `table.head.foreground` 与其余次要文字（明、暗各 1 组） | 表头文字用 `head_fg`（略深/略亮一档） | 表头要在一眼扫视里压过单元格 |")
    lines.append("| `chart.3` 与主色 | 图表第 3 系列保留原珊瑚 `#BC5340` | 换品牌主色时，数据系列色不跟着漂 |")
    lines.append("| `slider.background` / `switch.background` 与 `border` | 取 `input_border`（描边更实） | 控件轨道需要比分割线更明确 |")
    lines.append("| 状态栏 / 按钮 / 分组底 / `muted.background` | 分别取 `status` / `button` / `group` / `hover` | 现役资产挤在同一个 `#F3F3F3`，候选按层次拆开 |")
    lines.append("| `table.foot.background` 与斑马行（暗） | 表尾取 `group`（比斑马行退一档） | 表尾是汇总带，不该和行底同色 |")
    lines.append("| 按钮 hover 与 `muted.background`（暗） | 按钮 hover 由 `head` 派生，`muted.background` 取 `hover` | 按钮是控件、muted 块是容器，层级不同 |")
    lines.append("| **`sidebar.foreground`（暗，现役取正文 `#CCCCCC`）** | 取 `muted` | 侧栏文字明暗统一走「次要文字」；若想更接近现役，把 `gen_theme_packs.py` 里该键改成 `fg` 重跑即可（对比度仍达标） |")
    lines.append("| `switch.thumb.background` 与 `fg_active`（暗） | 滑块取近白，激活标签略灰 | 滑块要压在 `input_border` 轨道上，必须更亮 |")
    lines.append("")
    lines.append(f"## {sec + 1}. 内置多主题：现在能做到哪一步\n")
    lines.append(BUILTIN_NOTE + "\n")

    sec += 2
    lines.append(f"## {sec}. 选型建议\n")
    lines.append("**现代中性**：")
    lines.append("1. **默认 → 珊瑚石墨**：现役资产就是它的近似版，迁移只需微调，品牌资产零改动。")
    lines.append("2. **要「数据库工具」专业感 → 石板深蓝**：与 DataGrip / TablePlus 的心智一致，"
                 "注意 `info` 已改成青，别和主色蓝混用。")
    lines.append("3. **以报表 / EDA / 长文阅读为主 → 暖纸赭石**：暖底对细网格更宽容，但截图、"
                 "图表色需一并偏暖，否则冷暖打架。")
    lines.append("4. **要品牌差异化且偏「洞察」气质 → 松林青绿**：主色与成功色同族，"
                 "必须保持该套配色表里的明度差，否则在深底上会读成同一个绿。")
    lines.append("5. **低光 / OLED / 无障碍 → 近黑墨**：暗色纯黑省电，亮色是高对比版；"
                 "代价是 chrome 与内容区无灰度差，层级全靠描边。\n")
    lines.append("**中国风**：")
    lines.append("6. **内置首选 → 青花**：蓝白最克制，与“数据库工作台”的冷静气质不冲突；info 换青瓷绿后语义仍清楚。")
    lines.append("7. **要辨识度 → 朱砂**：一眼记住，但朱砂与危险红同族，danger 已压到绛色；"
                 "表单校验、删除确认这类场景要真机确认能分辨。")
    lines.append("8. **要“深色为主 + 金线点睛” → 墨金**：warning 已改成赭橙；"
                 "金只做强调，不要铺大面积（大面积金色会压掉数据本身的颜色区分度）。")
    lines.append("9. **不建议一次内置 8 套**：每多一包 QA 成本 ×N（140 键 × 2 模式 + 41 个语法名 + 11 个产品角色 + 真机截图），"
                 "且会稀释品牌识别；先上 2 包，其余留在可选包。\n")

    sec += 1
    lines.append(f"## {sec}. 验证与边界\n")
    lines.append(f"- 两层校验：色卡页按**角色口径**验 {per_mode} 项/模式（正文、次要文字、表头、主色三态、"
                 "语义四色三态、图表 5 系列、涨跌、产品槽 6 个、选中态合成、语法 6 个代表名）；"
                 "`audit_theme_packs.py` 按**包文件口径**再验 140/140 覆盖、52 对主题对比度、9 对产品对比度、"
                 "41 个语法名覆盖与着色对比度。")
    lines.append("- 选中底 / 文本选区按 `clamp_alpha` 上限（0.2 / 0.3）给值，超限会被 schema 静默压缩。")
    lines.append("- **未做的**：真机截图比对（明暗各一张全界面）；`tools/theme_audit.py` 仍硬编码 "
                 "`RDS Light` / `RDS Dark` 与现役资产路径，接包文件要么改脚本参数要么用它跑根因；"
                 "多包并存时的切换 / 回滚未验；没改视图代码，故未跑 `cargo test -p rds-ui-guard`。")
    lines.append("- 落地某套时的完整步骤：把包文件拷进 `assets/themes/` → 改 `attach_rds_theme` 与产品 token 的解析口径 → "
                 "`python tools/theme_audit.py`（0 不达标）→ `python tools/gen_theme_preview.py` → "
                 "真机明暗切换核对 → 更新 `theme-tokens-complete.md`。\n")
    return "\n".join(lines)


def main():
    html, total_checks, total_fail = build_html()
    OUT_HTML.parent.mkdir(parents=True, exist_ok=True)
    OUT_HTML.write_text(html, encoding="utf-8")
    OUT_MD.write_text(build_md(total_checks, total_fail), encoding="utf-8")

    for s in SCHEMES:
        for mode in ("light", "dark"):
            r = expand(s[mode], mode)
            cks = checks(r, mode)
            bad = [c for c in cks if c[3] < c[4]]
            print(f"{s['name']:14s} {mode:5s} {len(cks) - len(bad)}/{len(cks)}"
                  + (f"  FAIL: {[(c[0], round(c[3], 2)) for c in bad]}" if bad else ""))
    print(f"\n合计 {total_checks} 项，不达标 {total_fail} 项")
    print(f"已生成 {OUT_HTML}")
    print(f"已生成 {OUT_MD}")


if __name__ == "__main__":
    main()
