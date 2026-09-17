#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""生成 crates/mock/src/generator_catalog.rs（M7 生成器目录）。

为什么用脚本生成：
- 143 个 GeneratorConfig 变体，手工维护标签/参数清单必然漂移（v1 的 mockGeneratorDefs.ts 就是手写 1354 行）；
- 目录由 models.rs 的枚举**穷尽派生**：新增变体时 `spec_of` 的 match 会编译失败，强制补齐；
- 标签与默认值在本脚本的字典里维护（一次性），参数默认值按字段名启发式给出。

用法：python tools/gen_mock_generator_catalog.py
"""

import re
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[1]
MODELS = ROOT / "crates/mock/src/models.rs"
OUT = ROOT / "crates/mock/src/generator_catalog.rs"

CATEGORY_FROM_COMMENT = {
    "数值类": "Numeric",
    "字符串/文本类": "Text",
    "Markdown": "Markdown",
    "个人信息": "Person",
    "地址类": "Address",
    "日期时间类": "DateTime",
    "商业类": "Business",
    "金融类": "Finance",
    "网络/技术类": "Tech",
    "Picsum 图片 URL": "Image",
    "颜色类": "Color",
    "Ferroid ID 类": "FerroidId",
    "条形码与标准编码": "Code",
    "汽车与行政": "Vehicle",
    "约束类": "Constraint",
}

CATEGORY_LABEL = {
    "Numeric": "数值",
    "Text": "文本",
    "Markdown": "Markdown",
    "Person": "个人信息",
    "Address": "地址与网络标识",
    "DateTime": "日期时间",
    "Business": "商业",
    "Finance": "金融",
    "Tech": "网络与技术",
    "Image": "图片",
    "Color": "颜色",
    "FerroidId": "Ferroid ID",
    "Code": "标准编码",
    "Vehicle": "汽车与行政",
    "Constraint": "约束",
}

# 变体 → 中文标签（面板显示名）。未列出的回退到变体名。
LABELS = {
    # 数值
    "AutoIncrement": "自增序列",
    "RandomInt": "随机整数",
    "RandomFloat": "随机小数",
    "RandomDecimal": "随机定点数",
    "Digit": "单个数字",
    "NumberWithFormat": "格式化数字",
    "Normal": "正态分布",
    "LogNormal": "对数正态分布",
    "RandomWalk": "随机游走",
    "Boolean": "布尔值",
    "Poisson": "泊松分布",
    "Exponential": "指数分布",
    "Pareto": "帕累托分布（长尾）",
    "Beta": "Beta 分布（比例）",
    "Binomial": "二项分布",
    "TimeSeries": "时序数值（趋势 + 周期）",
    # 文本
    "Constant": "固定值",
    "Words": "随机词组",
    "Sentence": "随机句子",
    "Sentences": "多句文本",
    "Paragraph": "段落",
    "Paragraphs": "多段文本",
    "Word": "随机单词",
    "Regex": "正则表达式",
    "Template": "模板字符串",
    # Markdown
    "MarkdownItalicWord": "Markdown 斜体词",
    "MarkdownBoldWord": "Markdown 粗体词",
    "MarkdownLink": "Markdown 链接",
    "MarkdownBulletPoints": "Markdown 无序列表",
    "MarkdownListItems": "Markdown 列表项",
    "MarkdownBlockQuoteSingle": "Markdown 单行引用",
    "MarkdownBlockQuoteMulti": "Markdown 多行引用",
    "MarkdownCode": "Markdown 代码块",
    # 个人信息
    "Name": "姓名",
    "NameWithTitle": "带称谓姓名",
    "FirstName": "名",
    "LastName": "姓",
    "Title": "称谓",
    "Suffix": "名字后缀",
    "Email": "邮箱地址",
    "SafeEmail": "安全邮箱",
    "FreeEmailProvider": "免费邮箱服务商",
    "DomainSuffix": "域名后缀",
    "FreeEmail": "免费邮箱",
    "PhoneNumber": "电话号码",
    "CellNumber": "手机号",
    "Username": "用户名",
    "Password": "密码",
    # 地址
    "Country": "国家",
    "CountryCode": "国家代码",
    "CountryName": "国家名",
    "City": "城市",
    "CityPrefix": "城市前缀",
    "CitySuffix": "城市后缀",
    "StateName": "省/州名",
    "StateAbbr": "省/州缩写",
    "StreetName": "街道名",
    "StreetSuffix": "街道后缀",
    "ZipCode": "邮政编码",
    "PostCode": "邮政编号",
    "BuildingNumber": "楼牌号",
    "SecondaryAddress": "次级地址",
    "SecondaryAddressType": "次级地址类型",
    "Latitude": "纬度",
    "Longitude": "经度",
    "Geohash": "Geohash",
    "TimeZone": "时区",
    "IpAddress": "IP 地址",
    "IPv4": "IPv4 地址",
    "IPv6": "IPv6 地址",
    "IP": "IP（v4/v6）",
    "MacAddress": "MAC 地址",
    # 日期时间
    "DateTime": "日期时间（区间）",
    "DateTimeBefore": "某时间之前",
    "DateTimeAfter": "某时间之后",
    "DateTimeBetween": "两时间之间",
    "Date": "日期（区间）",
    "Time": "时间",
    "Duration": "持续时间",
    "SequentialDate": "顺序日期",
    "SequentialDateWithGaps": "顺序日期（含缺口）",
    # 商业
    "CompanyName": "公司名",
    "CompanySuffix": "公司后缀",
    "JobTitle": "职位",
    "Profession": "职业",
    "Industry": "行业",
    "Seniority": "职级",
    "Field": "领域",
    "Position": "岗位",
    "Buzzword": "行业术语",
    "BuzzwordMiddle": "术语中段",
    "BuzzwordTail": "术语尾段",
    "CatchPhrase": "业务口号",
    "BsVerb": "套话动词",
    "BsAdj": "套话形容词",
    "BsNoun": "套话名词",
    "Bs": "套话短语",
    # 金融
    "CurrencyCode": "货币代码",
    "CurrencyName": "货币名称",
    "CurrencySymbol": "货币符号",
    "Bic": "BIC 银行代码",
    "Isin": "ISIN 证券代码",
    "CreditCardNumber": "信用卡号",
    # 技术
    "UuidV1": "UUID v1",
    "UuidV3": "UUID v3",
    "UuidV4": "UUID v4",
    "UuidV5": "UUID v5",
    "Url": "URL",
    "UserAgent": "User-Agent",
    "MimeType": "MIME 类型",
    "Semver": "语义化版本",
    "SemverStable": "语义化版本（稳定）",
    "SemverUnstable": "语义化版本（预发布）",
    "FilePath": "文件路径",
    "FileName": "文件名",
    "FileExtension": "文件扩展名",
    "DirPath": "目录路径",
    # 图片
    "ImageUrl": "图片 URL",
    "ImageUrlWithSeed": "图片 URL（固定种子）",
    "ImageUrlGrayscale": "图片 URL（灰度）",
    "ImageUrlBlur": "图片 URL（模糊）",
    "ImageUrlCustom": "图片 URL（自定义）",
    # 颜色
    "HexColor": "HEX 颜色",
    "RgbColor": "RGB 颜色",
    "RgbaColor": "RGBA 颜色",
    "HslColor": "HSL 颜色",
    "HslaColor": "HSLA 颜色",
    "Color": "颜色名",
    # Ferroid
    "FerroidULID": "ULID",
    "FerroidTwitterId": "Twitter ID",
    "FerroidInstagramId": "Instagram ID",
    "FerroidMastodonId": "Mastodon ID",
    "FerroidDiscordId": "Discord ID",
    # 编码
    "Isbn": "ISBN",
    "Isbn10": "ISBN-10",
    "Isbn13": "ISBN-13",
    "RfcStatusCode": "RFC 状态码",
    "ValidStatusCode": "有效 HTTP 状态码",
    # 汽车与行政
    "LicencePlate": "车牌号",
    "HealthInsuranceCode": "医保编号",
    # 约束
    "ForeignKey": "外键取值",
    "Sequence": "序列取值",
    "Weighted": "加权取值",
}

# 需要用户填写的复杂参数（列表/元组列表）：面板用多行文本编辑（一行一项 / 一行「值, 权重」）
COMPLEX_FIELDS = {"values", "choices"}

PARAM_LABEL = {
    "start": "起始值",
    "step": "步长",
    "min": "最小值",
    "max": "最大值",
    "precision": "精度",
    "scale": "小数位",
    "ratio": "占比（%）",
    "value": "固定值",
    "pattern": "正则表达式",
    "template": "模板",
    "fmt": "格式",
    "mean": "均值",
    "std_dev": "标准差",
    "median": "中位数",
    "dispersion": "离散度",
    "volatility": "波动率",
    "lambda": "强度 λ",
    "scale_value": "尺度（最小值）",
    "alpha": "形状参数 α",
    "beta": "形状参数 β",
    "trials": "试验次数 n",
    "probability": "概率 p",
    "trend": "趋势（每行增量）",
    "period": "周期（行数）",
    "amplitude": "周期振幅",
    "noise": "噪声强度",
    "count": "数量",
    "before": "早于",
    "after": "晚于",
    "end": "结束",
    "step_seconds": "步长（秒）",
    "miss_probability": "缺失概率",
    "width": "宽",
    "height": "高",
    "blur_amount": "模糊程度",
    "grayscale": "灰度",
    "seed": "种子",
    "cycle": "循环",
    "values": "取值集合",
    "choices": "加权选项",
}

# 参数默认值启发式（按字段名）
STRING_DEFAULTS = {
    "min": '"2020-01-01".to_string()',
    "before": '"2020-01-01".to_string()',
    "after": '"2020-01-01".to_string()',
    "end": '"2025-12-31".to_string()',
    "max": '"2025-12-31".to_string()',
    "pattern": '"\\\\d{3}-\\\\d{4}".to_string()',
    "template": '"{value}".to_string()',
    "fmt": '"000".to_string()',
}


def default_for(field, ty):
    # Option<T> → Some(T 的默认)；本轮 GeneratorConfig 里仅有 Option<u8/u32>
    if ty.startswith("Option<"):
        inner = ty[len("Option<") : -1]
        inner_default = default_for(field, inner)
        return None if inner_default is None else f"Some({inner_default})"
    if "Vec" in ty:
        return None  # 复杂参数
    if ty == "String":
        return STRING_DEFAULTS.get(field, '"".to_string()')
    if ty == "bool":
        return "false"
    if ty == "f64":
        return {
            "min": "0.0",
            "max": "10000.0",
            "step": "1.0",
            "mean": "0.0",
            "std_dev": "1.0",
            "median": "1.0",
            "dispersion": "0.5",
            "volatility": "0.1",
            "miss_probability": "0.1",
            "lambda": "1.0",
            "scale_value": "1.0",
            "alpha": "1.5",
            "beta": "1.0",
            "probability": "0.5",
            "trend": "0.0",
            "amplitude": "1.0",
            "noise": "0.1",
        }.get(field, "0.0")
    # 整数族
    return {
        "start": "1",
        "step": "1",
        "min": "1",
        "max": "10",
        "precision": "2",
        "scale": "2",
        "ratio": "50",
        "step_seconds": "86400",
        "width": "200",
        "height": "200",
        "blur_amount": "2",
        "seed": "42",
        "count": "1",
        "trials": "10",
        "period": "24",
    }.get(field, "0")


def parse_models():
    src = MODELS.read_text(encoding="utf-8")
    body = re.search(r"pub enum GeneratorConfig\s*\{(.*?)\n\}", src, re.S).group(1)
    variants = []  # (name, [(field, type)], category)
    category = None
    buf_fields = None
    for raw in body.splitlines():
        line = raw.strip()
        m_comment = re.match(r"//\s*(.+?)\s*$", line)
        if m_comment and m_comment.group(1) in CATEGORY_FROM_COMMENT:
            category = CATEGORY_FROM_COMMENT[m_comment.group(1)]
            continue
        if not line or line.startswith("//"):
            continue
        m_open = re.match(r"^([A-Z][A-Za-z0-9]*)\s*\{$", line)
        if m_open:
            buf_fields = []
            variants.append([m_open.group(1), buf_fields, category])
            continue
        m_unit = re.match(r"^([A-Z][A-Za-z0-9]*),$", line)
        if m_unit:
            variants.append([m_unit.group(1), [], category])
            buf_fields = None
            continue
        if line in ("}", "},"):
            buf_fields = None
            continue
        m_field = re.match(r"^([a-z_][a-z0-9_]*)\s*:\s*(.+?),$", line)
        if m_field and buf_fields is not None:
            buf_fields.append((m_field.group(1), m_field.group(2).strip()))
            continue
        raise SystemExit(f"无法解析的行: {raw!r}")
    return variants


def snake(name):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def main():
    variants = parse_models()
    assert len(variants) == 143, f"变体数异常: {len(variants)}"

    lines = []
    add = lines.append
    add("//! 生成器目录（M7）——由 `models.rs` 的 `GeneratorConfig` 穷尽派生。")
    add("//!")
    add("//! 本文件由 `tools/gen_mock_generator_catalog.py` 生成，**不要手改**：")
    add("//! 新增 / 删除 `GeneratorConfig` 变体后重跑脚本，`spec_of` 的穷尽 match 会强制补齐。")
    add("//!")
    add("//! 用途：面板展示（中文标签）、生成器选择器（分类浏览 + 搜索）、参数编辑表单。")
    add("")
    add("use crate::models::GeneratorConfig;")
    add("")
    add("/// 生成器分类（面板分组用；顺序即展示顺序）。")
    add("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
    add("pub enum GeneratorCategory {")
    for c in CATEGORY_LABEL:
        add(f"    {c},")
    add("}")
    add("")
    add("impl GeneratorCategory {")
    add("    /// 全部分类（展示顺序）")
    add("    pub const ALL: [GeneratorCategory; 15] = [")
    for c in CATEGORY_LABEL:
        add(f"        GeneratorCategory::{c},")
    add("    ];")
    add("")
    add("    /// 中文标签")
    add("    pub fn label(self) -> &'static str {")
    add("        match self {")
    for c, label in CATEGORY_LABEL.items():
        add(f'            GeneratorCategory::{c} => "{label}",')
    add("        }")
    add("    }")
    add("}")
    add("")
    add("/// 参数类型（决定参数编辑器渲染何种输入控件）。")
    add("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
    add("pub enum ParamKind {")
    add("    /// 整数（可带上下限）")
    add("    Int,")
    add("    /// 浮点数")
    add("    Float,")
    add("    /// 文本")
    add("    Text,")
    add("    /// 布尔")
    add("    Bool,")
    add("    /// 复杂参数（字符串列表 / 加权选项）——面板用多行文本编辑（一行一项）")
    add("    Complex,")
    add("}")
    add("")
    add("/// 一个可编辑参数。")
    add("#[derive(Debug, Clone, Copy)]")
    add("pub struct ParamField {")
    add("    /// 字段名（与 `GeneratorConfig` 变体字段同名；JSON 补丁按此键写回）")
    add("    pub key: &'static str,")
    add("    /// 中文标签")
    add("    pub label: &'static str,")
    add("    /// 参数类型")
    add("    pub kind: ParamKind,")
    add("}")
    add("")
    add("/// 生成器规格。")
    add("#[derive(Debug, Clone, Copy)]")
    add("pub struct GeneratorSpec {")
    add("    /// 稳定标识（snake_case，与 v1 前端 `GeneratorType` 对齐）")
    add("    pub name: &'static str,")
    add("    /// 中文标签")
    add("    pub label: &'static str,")
    add("    /// 分类")
    add("    pub category: GeneratorCategory,")
    add("    /// 可编辑参数（顺序即表单顺序）")
    add("    pub params: &'static [ParamField],")
    add("}")
    add("")
    add("// ==================== 规格常量 ====================")
    add("")
    idents = {}
    for name, fields, category in variants:
        const = "SPEC_" + snake(name).upper()
        idents[name] = const
        add(f"/// `{name}`")
        add(f"static {const}: GeneratorSpec = GeneratorSpec {{")
        add(f'    name: "{snake(name)}",')
        add(f'    label: "{LABELS.get(name, name)}",')
        add(f"    category: GeneratorCategory::{category},")
        if fields:
            add(f"    params: &[")
            for fname, fty in fields:
                key, label = fname, PARAM_LABEL.get(fname, fname)
                if fname in COMPLEX_FIELDS or "Vec" in fty:
                    kind = "Complex"
                elif fty == "bool":
                    kind = "Bool"
                elif fty == "f64":
                    kind = "Float"
                elif fty == "String":
                    kind = "Text"
                else:
                    kind = "Int"
                add(f'        ParamField {{ key: "{key}", label: "{label}", kind: ParamKind::{kind} }},')
            add("    ],")
        else:
            add("    params: &[],")
        add("};")
        add("")
    add("// ==================== 查询入口 ====================")
    add("")
    add("/// 当前配置对应的规格（穷尽 match：新增变体将编译失败，强制补规格）。")
    add("pub fn spec_of(config: &GeneratorConfig) -> &'static GeneratorSpec {")
    add("    match config {")
    for name, fields, _ in variants:
        pat = f"GeneratorConfig::{name} {{ .. }}" if fields else f"GeneratorConfig::{name}"
        add(f"        {pat} => &{idents[name]},")
    add("    }")
    add("}")
    add("")
    add("/// 全部生成器规格（选择器浏览用；顺序 = 分类顺序 + 变体声明顺序）。")
    add("pub fn all_specs() -> &'static [&'static GeneratorSpec] {")
    add("    ALL_SPECS")
    add("}")
    add("")
    add("/// 规格表（static：避免每次调用构造临时数组）。")
    add("static ALL_SPECS: &[&GeneratorSpec] = &[")
    for name, _, _ in variants:
        add(f"    &{idents[name]},")
    add("];")
    add("")
    add("/// 某分类下的规格。")
    add("pub fn specs_in(category: GeneratorCategory) -> Vec<&'static GeneratorSpec> {")
    add("    all_specs()")
    add("        .iter()")
    add("        .copied()")
    add("        .filter(|s| s.category == category)")
    add("        .collect()")
    add("}")
    add("")
    add("/// 按标识查规格。")
    add("pub fn spec_by_name(name: &str) -> Option<&'static GeneratorSpec> {")
    add("    all_specs().iter().copied().find(|s| s.name == name)")
    add("}")
    add("")
    add("/// 由规格标识构造一个可用的默认配置（选择器切换生成器时使用）。")
    add("///")
    add("/// 参数取「可读默认值」（如区间 1~10、日期 2020-01-01~2025-12-31），")
    add("/// 用户可在参数区继续调整；复杂参数（列表 / 加权选项）默认留空，")
    add("/// 需在列编辑对话框里填写（留空会在生成前被拦住，不会 panic）。")
    add("pub fn default_of(name: &str) -> Option<GeneratorConfig> {")
    add("    let config = match name {")
    for name, fields, _ in variants:
        if fields:
            args = ", ".join(
                f"{fname}: {default_for(fname, fty)}"
                for fname, fty in fields
                if default_for(fname, fty) is not None
            )
            missing = [f for f, t in fields if default_for(f, t) is None]
            if missing:
                args = ", ".join(
                    [
                        f"{fname}: {default_for(fname, fty)}"
                        for fname, fty in fields
                        if default_for(fname, fty) is not None
                    ]
                    + [f"{m}: Vec::new()" for m in missing]
                )
            add(f'        "{snake(name)}" => GeneratorConfig::{name} {{ {args} }},')
        else:
            add(f'        "{snake(name)}" => GeneratorConfig::{name},')
    add("        _ => return None,")
    add("    };")
    add("    Some(config)")
    add("}")
    add("")
    add("#[cfg(test)]")
    add("mod tests {")
    add("    use super::*;")
    add("")
    add("    #[test]")
    add("    fn all_specs_cover_every_variant() {")
    add("        assert_eq!(all_specs().len(), 143, \"规格数与变体数应一致\");")
    add("        // 标识唯一")
    add("        let mut names: Vec<&str> = all_specs().iter().map(|s| s.name).collect();")
    add("        names.sort_unstable();")
    add("        let total = names.len();")
    add("        names.dedup();")
    add("        assert_eq!(names.len(), total, \"规格标识不得重复\");")
    add("    }")
    add("")
    add("    #[test]")
    add("    fn every_spec_has_label_and_default() {")
    add("        for spec in all_specs() {")
    add("            assert!(!spec.label.is_empty(), \"{} 缺标签\", spec.name);")
    add("            let cfg = default_of(spec.name);")
    add("            assert!(cfg.is_some(), \"{} 缺默认构造\", spec.name);")
    add("            // 默认构造必须能回到同一规格（往返一致）")
    add("            assert_eq!(spec_of(&cfg.unwrap()).name, spec.name);")
    add("        }")
    add("    }")
    add("")
    add("    #[test]")
    add("    fn every_variant_resolves_to_spec() {")
    add("        for spec in all_specs() {")
    add("            let cfg = default_of(spec.name).expect(\"默认构造\");")
    add("            assert_eq!(spec_of(&cfg).category, spec.category);")
    add("        }")
    add("    }")
    add("}")
    add("")

    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {OUT} ({len(lines)} 行, {len(variants)} 规格)")
    # 脚本产出的是紧凑写法，与仓库里 rustfmt 过的版本有排版差异；不格式化的话
    # 每次重跑都会在 diff 里混进几百行与逻辑无关的换行变动（README §设计与验证 写了这一步）。
    print("提示：请对生成文件跑一次 rustfmt（--edition 2024），否则会带进格式漂移")


if __name__ == "__main__":
    main()
