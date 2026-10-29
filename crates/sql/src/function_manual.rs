//! SQL 函数中文语义手册（#54 ③ 扩展：用户「想不起函数名」场景）
//!
//! ## 定位
//!
//! 内置函数名是英文，中文用户常见的场景是「知道要干什么、想不起函数名」。
//! 本模块提供**中文语义索引**：`中文关键词 → SQL 函数`，中英混搜。
//! 与 [`super::functions`] 的分工：
//!
//! - `functions`：**签名目录**（在线系统表全量 + 内置兜底），回答「函数怎么用」；
//! - 本模块：**语义手册**（中文索引 150+ 条高频常用），回答「用什么函数」。
//!
//! 交互由 workbench 侧实现（Quick Open 式函数手册：中英搜 + 分类分组 + 回车插入）。
//!
//! ## 口径（固定）
//!
//! - 只收**高频常用**函数（维护成本可控），不是全量——全量靠在线系统表。
//! - 条目 `name` 取主流写法（如 SUBSTR），别名放 `keywords`（substring…）。
//! - **不编造官方文档 URL**：跳转由 workbench 按「引擎 + 函数名」生成，真实环境验证后定。
//! - 搜索匹配小写化，中文原样；排序 = 函数名前缀 > 函数名包含 > 关键词 > 中文描述。

/// 手册分类（UI 分组依据）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManualCategory {
    String,   // 字符串
    Numeric,  // 数值
    DateTime, // 日期时间
    Aggregate,// 聚合
    Window,   // 窗口
    Json,     // JSON
    Cast,     // 类型转换
    Logic,    // 逻辑条件
    System,   // 系统信息
}

impl ManualCategory {
    /// 中文分组标签（UI 直接展示）
    pub fn label(self) -> &'static str {
        match self {
            ManualCategory::String => "字符串",
            ManualCategory::Numeric => "数值",
            ManualCategory::DateTime => "日期时间",
            ManualCategory::Aggregate => "聚合",
            ManualCategory::Window => "窗口",
            ManualCategory::Json => "JSON",
            ManualCategory::Cast => "类型转换",
            ManualCategory::Logic => "逻辑条件",
            ManualCategory::System => "系统信息",
        }
    }
}

/// 一条手册记录：函数名 + 分类 + 中文语义关键词 + 中文描述 + 一句话示例
#[derive(Debug, Clone, Copy)]
pub struct ManualEntry {
    pub name: &'static str,
    pub category: ManualCategory,
    /// 中文语义关键词（含别名），搜索入口
    pub keywords: &'static [&'static str],
    /// 中文一句话描述
    pub zh: &'static str,
    /// 一句话示例（正确性对齐 SQLite/PostgreSQL 通用写法）
    pub example: &'static str,
}

/// 手册数据（静态数组，零运行时构造）
pub static MANUAL: &[ManualEntry] = &[
    // ── 字符串 ──────────────────────────────────────────────
    ManualEntry { name: "CONCAT", category: ManualCategory::String, keywords: &["拼接", "连接字符串", "合并"], zh: "把多个字符串拼接成一个", example: "SELECT CONCAT('a', 'b');" },
    ManualEntry { name: "CONCAT_WS", category: ManualCategory::String, keywords: &["拼接带分隔符"], zh: "用分隔符拼接多个字符串", example: "SELECT CONCAT_WS('-', 'a', 'b');" },
    ManualEntry { name: "SUBSTR", category: ManualCategory::String, keywords: &["截取", "子串", "substring", "取中间", "切片"], zh: "从指定位置截取子串", example: "SELECT SUBSTR('hello', 2, 3);" },
    ManualEntry { name: "SUBSTRING", category: ManualCategory::String, keywords: &["截取", "子串", "substr"], zh: "截取子串（SQL 标准写法）", example: "SELECT SUBSTRING('hello' FROM 2 FOR 3);" },
    ManualEntry { name: "LEFT", category: ManualCategory::String, keywords: &["左侧取", "从头取", "前缀"], zh: "从左侧取 N 个字符", example: "SELECT LEFT('hello', 2);" },
    ManualEntry { name: "RIGHT", category: ManualCategory::String, keywords: &["右侧取", "从尾取", "后缀"], zh: "从右侧取 N 个字符", example: "SELECT RIGHT('hello', 2);" },
    ManualEntry { name: "TRIM", category: ManualCategory::String, keywords: &["去空格", "去空白", "去掉首尾"], zh: "去掉字符串首尾空白", example: "SELECT TRIM('  hi  ');" },
    ManualEntry { name: "LTRIM", category: ManualCategory::String, keywords: &["去左空格", "去左边"], zh: "去掉左侧空白", example: "SELECT LTRIM('  hi');" },
    ManualEntry { name: "RTRIM", category: ManualCategory::String, keywords: &["去右空格", "去右边"], zh: "去掉右侧空白", example: "SELECT RTRIM('hi  ');" },
    ManualEntry { name: "UPPER", category: ManualCategory::String, keywords: &["大写", "转大写", "uppercase"], zh: "转成大写字母", example: "SELECT UPPER('hello');" },
    ManualEntry { name: "LOWER", category: ManualCategory::String, keywords: &["小写", "转小写", "lowercase"], zh: "转成小写字母", example: "SELECT LOWER('HELLO');" },
    ManualEntry { name: "LENGTH", category: ManualCategory::String, keywords: &["长度", "字符数", "多少字符"], zh: "字符串字符个数", example: "SELECT LENGTH('hello');" },
    ManualEntry { name: "CHAR_LENGTH", category: ManualCategory::String, keywords: &["长度", "字符数"], zh: "字符数（SQL 标准）", example: "SELECT CHAR_LENGTH('hello');" },
    ManualEntry { name: "REPLACE", category: ManualCategory::String, keywords: &["替换", "换字符", "把a换成b"], zh: "把子串替换成另一子串", example: "SELECT REPLACE('abc', 'b', 'x');" },
    ManualEntry { name: "INSTR", category: ManualCategory::String, keywords: &["查找位置", "子串位置", "第几个", "在哪"], zh: "返回子串首次出现的位置", example: "SELECT INSTR('hello', 'l');" },
    ManualEntry { name: "STRPOS", category: ManualCategory::String, keywords: &["查找位置", "子串位置", "instr"], zh: "返回子串位置（PG 写法）", example: "SELECT STRPOS('hello', 'l');" },
    ManualEntry { name: "POSITION", category: ManualCategory::String, keywords: &["查找位置", "子串位置"], zh: "返回子串位置（SQL 标准）", example: "SELECT POSITION('l' IN 'hello');" },
    ManualEntry { name: "LOCATE", category: ManualCategory::String, keywords: &["查找位置", "子串位置"], zh: "返回子串位置（MySQL 写法）", example: "SELECT LOCATE('l', 'hello');" },
    ManualEntry { name: "LPAD", category: ManualCategory::String, keywords: &["左填充", "补零", "对齐"], zh: "左侧补足到指定长度", example: "SELECT LPAD('7', 3, '0');" },
    ManualEntry { name: "RPAD", category: ManualCategory::String, keywords: &["右填充", "补零", "对齐"], zh: "右侧补足到指定长度", example: "SELECT RPAD('7', 3, '0');" },
    ManualEntry { name: "REVERSE", category: ManualCategory::String, keywords: &["反转", "倒序", "逆序"], zh: "字符串反转", example: "SELECT REVERSE('abc');" },
    ManualEntry { name: "SPLIT_PART", category: ManualCategory::String, keywords: &["分割", "按分隔符取", "第几段"], zh: "按分隔符切分后取第 N 段", example: "SELECT SPLIT_PART('a-b-c', '-', 2);" },
    ManualEntry { name: "REGEXP_REPLACE", category: ManualCategory::String, keywords: &["正则替换", "正则", "regex"], zh: "按正则替换子串", example: "SELECT REGEXP_REPLACE('a1b2', '[0-9]', '#');" },
    ManualEntry { name: "REGEXP_MATCH", category: ManualCategory::String, keywords: &["正则匹配", "匹配", "regex"], zh: "判断是否匹配正则", example: "SELECT REGEXP_MATCH('abc', '[a-z]+');" },
    ManualEntry { name: "REGEXP_EXTRACT", category: ManualCategory::String, keywords: &["正则提取", "抓取", "regex"], zh: "按正则提取匹配片段", example: "SELECT REGEXP_EXTRACT('id-42', '[0-9]+');" },
    ManualEntry { name: "ASCII", category: ManualCategory::String, keywords: &["字符编码", "ascii"], zh: "取首字符的 ASCII 码", example: "SELECT ASCII('A');" },
    ManualEntry { name: "CHR", category: ManualCategory::String, keywords: &["编码转字符", "ascii 转"], zh: "ASCII 码转字符", example: "SELECT CHR(65);" },
    ManualEntry { name: "FORMAT", category: ManualCategory::String, keywords: &["格式化", "拼接模板"], zh: "按模板格式化（%s 占位）", example: "SELECT FORMAT('%s-%s', 'a', 1);" },
    ManualEntry { name: "REPEAT", category: ManualCategory::String, keywords: &["重复", "循环字符"], zh: "重复字符串 N 次", example: "SELECT REPEAT('ab', 3);" },
    ManualEntry { name: "SPACE", category: ManualCategory::String, keywords: &["空格串"], zh: "生成 N 个空格", example: "SELECT SPACE(3);" },
    ManualEntry { name: "INITCAP", category: ManualCategory::String, keywords: &["首字母大写", "驼峰"], zh: "每个单词首字母大写", example: "SELECT INITCAP('hello world');" },
    ManualEntry { name: "TRANSLATE", category: ManualCategory::String, keywords: &["字符映射", "替换多个"], zh: "按字符集一对一替换", example: "SELECT TRANSLATE('abc', 'abc', 'xyz');" },
    ManualEntry { name: "MD5", category: ManualCategory::String, keywords: &["哈希", "摘要"], zh: "计算 MD5 哈希", example: "SELECT MD5('abc');" },
    // ── 数值 ──────────────────────────────────────────────
    ManualEntry { name: "ABS", category: ManualCategory::Numeric, keywords: &["绝对值", "取正"], zh: "取绝对值", example: "SELECT ABS(-5);" },
    ManualEntry { name: "ROUND", category: ManualCategory::Numeric, keywords: &["四舍五入", "保留小数"], zh: "四舍五入到指定位数", example: "SELECT ROUND(3.14159, 2);" },
    ManualEntry { name: "CEIL", category: ManualCategory::Numeric, keywords: &["向上取整", "进一"], zh: "向上取整", example: "SELECT CEIL(2.1);" },
    ManualEntry { name: "CEILING", category: ManualCategory::Numeric, keywords: &["向上取整", "进一"], zh: "向上取整（MySQL 别名）", example: "SELECT CEILING(2.1);" },
    ManualEntry { name: "FLOOR", category: ManualCategory::Numeric, keywords: &["向下取整", "舍去"], zh: "向下取整", example: "SELECT FLOOR(2.9);" },
    ManualEntry { name: "TRUNC", category: ManualCategory::Numeric, keywords: &["截断小数", "不四舍五入"], zh: "直接截断到指定位数", example: "SELECT TRUNC(3.14159, 2);" },
    ManualEntry { name: "MOD", category: ManualCategory::Numeric, keywords: &["取余", "余数", "模"], zh: "取余数", example: "SELECT MOD(10, 3);" },
    ManualEntry { name: "POWER", category: ManualCategory::Numeric, keywords: &["幂", "次方", "平方"], zh: "幂运算", example: "SELECT POWER(2, 10);" },
    ManualEntry { name: "SQRT", category: ManualCategory::Numeric, keywords: &["平方根", "开方"], zh: "平方根", example: "SELECT SQRT(16);" },
    ManualEntry { name: "EXP", category: ManualCategory::Numeric, keywords: &["指数", "e 的"], zh: "e 的 N 次方", example: "SELECT EXP(1);" },
    ManualEntry { name: "LN", category: ManualCategory::Numeric, keywords: &["自然对数", "log"], zh: "自然对数", example: "SELECT LN(2.718);" },
    ManualEntry { name: "LOG10", category: ManualCategory::Numeric, keywords: &["常用对数", "log10"], zh: "以 10 为底的对数", example: "SELECT LOG10(100);" },
    ManualEntry { name: "SIGN", category: ManualCategory::Numeric, keywords: &["符号", "正负"], zh: "返回符号（-1/0/1）", example: "SELECT SIGN(-3);" },
    ManualEntry { name: "GREATEST", category: ManualCategory::Numeric, keywords: &["最大值", "取最大", "多个值"], zh: "多个值里取最大", example: "SELECT GREATEST(1, 5, 3);" },
    ManualEntry { name: "LEAST", category: ManualCategory::Numeric, keywords: &["最小值", "取最小", "多个值"], zh: "多个值里取最小", example: "SELECT LEAST(1, 5, 3);" },
    ManualEntry { name: "RANDOM", category: ManualCategory::Numeric, keywords: &["随机数", "随机"], zh: "随机数（0~1，PG/DuckDB）", example: "SELECT RANDOM();" },
    ManualEntry { name: "RAND", category: ManualCategory::Numeric, keywords: &["随机数", "随机"], zh: "随机数（MySQL/SQLite）", example: "SELECT RAND();" },
    ManualEntry { name: "PI", category: ManualCategory::Numeric, keywords: &["圆周率"], zh: "圆周率 π", example: "SELECT PI();" },
    ManualEntry { name: "DEGREES", category: ManualCategory::Numeric, keywords: &["弧度转角度"], zh: "弧度转角度", example: "SELECT DEGREES(3.14159);" },
    ManualEntry { name: "RADIANS", category: ManualCategory::Numeric, keywords: &["角度转弧度"], zh: "角度转弧度", example: "SELECT RADIANS(180);" },
    // ── 日期时间 ──────────────────────────────────────────
    ManualEntry { name: "NOW", category: ManualCategory::DateTime, keywords: &["当前时间", "现在", "时间戳"], zh: "当前时间戳", example: "SELECT NOW();" },
    ManualEntry { name: "CURRENT_DATE", category: ManualCategory::DateTime, keywords: &["今天", "当前日期", "日期"], zh: "当前日期", example: "SELECT CURRENT_DATE;" },
    ManualEntry { name: "CURRENT_TIMESTAMP", category: ManualCategory::DateTime, keywords: &["当前时间", "时间戳"], zh: "当前时间戳（SQL 标准）", example: "SELECT CURRENT_TIMESTAMP;" },
    ManualEntry { name: "CURRENT_TIME", category: ManualCategory::DateTime, keywords: &["当前时间"], zh: "当前时间（不含日期）", example: "SELECT CURRENT_TIME;" },
    ManualEntry { name: "DATE", category: ManualCategory::DateTime, keywords: &["取日期", "只要日期"], zh: "从时间戳取日期部分", example: "SELECT DATE('2024-01-02 10:00:00');" },
    ManualEntry { name: "TIME", category: ManualCategory::DateTime, keywords: &["取时间"], zh: "从时间戳取时间部分", example: "SELECT TIME('2024-01-02 10:00:00');" },
    ManualEntry { name: "EXTRACT", category: ManualCategory::DateTime, keywords: &["提取年月日", "年", "月", "日", "时分秒"], zh: "提取日期字段（YEAR/MONTH/DAY…）", example: "SELECT EXTRACT(YEAR FROM DATE '2024-01-02');" },
    ManualEntry { name: "YEAR", category: ManualCategory::DateTime, keywords: &["年", "年份"], zh: "取年份", example: "SELECT YEAR('2024-01-02');" },
    ManualEntry { name: "MONTH", category: ManualCategory::DateTime, keywords: &["月", "月份"], zh: "取月份", example: "SELECT MONTH('2024-01-02');" },
    ManualEntry { name: "DAY", category: ManualCategory::DateTime, keywords: &["日", "几号"], zh: "取日", example: "SELECT DAY('2024-01-02');" },
    ManualEntry { name: "HOUR", category: ManualCategory::DateTime, keywords: &["小时", "几点"], zh: "取小时", example: "SELECT HOUR('10:30:00');" },
    ManualEntry { name: "MINUTE", category: ManualCategory::DateTime, keywords: &["分钟"], zh: "取分钟", example: "SELECT MINUTE('10:30:00');" },
    ManualEntry { name: "SECOND", category: ManualCategory::DateTime, keywords: &["秒"], zh: "取秒", example: "SELECT SECOND('10:30:45');" },
    ManualEntry { name: "DATE_ADD", category: ManualCategory::DateTime, keywords: &["日期加", "加一天", "加几天", "往后"], zh: "日期加区间", example: "SELECT DATE_ADD('2024-01-02', INTERVAL 1 DAY);" },
    ManualEntry { name: "DATE_SUB", category: ManualCategory::DateTime, keywords: &["日期减", "减一天", "往前"], zh: "日期减区间", example: "SELECT DATE_SUB('2024-01-02', INTERVAL 1 DAY);" },
    ManualEntry { name: "DATEDIFF", category: ManualCategory::DateTime, keywords: &["日期差", "相差几天", "间隔"], zh: "两个日期相差的天数", example: "SELECT DATEDIFF('2024-01-05', '2024-01-01');" },
    ManualEntry { name: "TIMESTAMPDIFF", category: ManualCategory::DateTime, keywords: &["时间差", "相差"], zh: "按单位计算时间差", example: "SELECT TIMESTAMPDIFF(HOUR, '2024-01-01', '2024-01-02');" },
    ManualEntry { name: "DATE_TRUNC", category: ManualCategory::DateTime, keywords: &["截断日期", "归零", "按月归"], zh: "时间戳截断到指定粒度", example: "SELECT DATE_TRUNC('month', '2024-01-15');" },
    ManualEntry { name: "TO_CHAR", category: ManualCategory::DateTime, keywords: &["格式化日期", "日期转字符串", "格式"], zh: "日期按格式转字符串", example: "SELECT TO_CHAR('2024-01-02', 'YYYY-MM-DD');" },
    ManualEntry { name: "DATE_FORMAT", category: ManualCategory::DateTime, keywords: &["格式化日期", "日期转字符串"], zh: "日期格式化（MySQL）", example: "SELECT DATE_FORMAT('2024-01-02', '%Y-%m-%d');" },
    ManualEntry { name: "TO_DATE", category: ManualCategory::DateTime, keywords: &["字符串转日期", "文本转日期"], zh: "字符串按格式转日期", example: "SELECT TO_DATE('2024-01-02', 'YYYY-MM-DD');" },
    ManualEntry { name: "STR_TO_DATE", category: ManualCategory::DateTime, keywords: &["字符串转日期"], zh: "字符串按格式转日期（MySQL）", example: "SELECT STR_TO_DATE('2024-01-02', '%Y-%m-%d');" },
    ManualEntry { name: "TO_TIMESTAMP", category: ManualCategory::DateTime, keywords: &["字符串转时间戳"], zh: "字符串转时间戳", example: "SELECT TO_TIMESTAMP('2024-01-02 10:00:00', 'YYYY-MM-DD HH24:MI:SS');" },
    ManualEntry { name: "LAST_DAY", category: ManualCategory::DateTime, keywords: &["月末", "最后一天"], zh: "当月最后一天", example: "SELECT LAST_DAY('2024-02-10');" },
    ManualEntry { name: "WEEKDAY", category: ManualCategory::DateTime, keywords: &["星期几", "周几"], zh: "星期几（0=周一）", example: "SELECT WEEKDAY('2024-01-02');" },
    ManualEntry { name: "DAYOFWEEK", category: ManualCategory::DateTime, keywords: &["星期几", "周几"], zh: "星期几（1=周日）", example: "SELECT DAYOFWEEK('2024-01-02');" },
    ManualEntry { name: "DAYNAME", category: ManualCategory::DateTime, keywords: &["星期名", "周几"], zh: "星期英文名", example: "SELECT DAYNAME('2024-01-02');" },
    ManualEntry { name: "AGE", category: ManualCategory::DateTime, keywords: &["年龄", "时长", "至今"], zh: "日期距今天数/年龄（PG）", example: "SELECT AGE('2000-01-01');" },
    // ── 聚合 ──────────────────────────────────────────────
    ManualEntry { name: "COUNT", category: ManualCategory::Aggregate, keywords: &["计数", "数量", "多少行"], zh: "统计行数", example: "SELECT COUNT(*) FROM t;" },
    ManualEntry { name: "SUM", category: ManualCategory::Aggregate, keywords: &["求和", "合计", "总计"], zh: "求和", example: "SELECT SUM(amount) FROM t;" },
    ManualEntry { name: "AVG", category: ManualCategory::Aggregate, keywords: &["平均", "均值", "平均值"], zh: "平均值", example: "SELECT AVG(score) FROM t;" },
    ManualEntry { name: "MIN", category: ManualCategory::Aggregate, keywords: &["最小", "最小值"], zh: "最小值", example: "SELECT MIN(price) FROM t;" },
    ManualEntry { name: "MAX", category: ManualCategory::Aggregate, keywords: &["最大", "最大值"], zh: "最大值", example: "SELECT MAX(price) FROM t;" },
    ManualEntry { name: "GROUP_CONCAT", category: ManualCategory::Aggregate, keywords: &["拼接", "分组拼接", "合并值"], zh: "分组值拼成串（MySQL/SQLite）", example: "SELECT GROUP_CONCAT(name) FROM t;" },
    ManualEntry { name: "STRING_AGG", category: ManualCategory::Aggregate, keywords: &["拼接", "分组拼接", "合并值"], zh: "分组值拼成串（PG/DuckDB）", example: "SELECT STRING_AGG(name, ',') FROM t;" },
    ManualEntry { name: "ARRAY_AGG", category: ManualCategory::Aggregate, keywords: &["聚合成数组", "数组"], zh: "分组值聚合成数组", example: "SELECT ARRAY_AGG(id) FROM t;" },
    ManualEntry { name: "VARIANCE", category: ManualCategory::Aggregate, keywords: &["方差"], zh: "方差", example: "SELECT VARIANCE(x) FROM t;" },
    ManualEntry { name: "STDDEV", category: ManualCategory::Aggregate, keywords: &["标准差"], zh: "标准差", example: "SELECT STDDEV(x) FROM t;" },
    ManualEntry { name: "MEDIAN", category: ManualCategory::Aggregate, keywords: &["中位数", "中值"], zh: "中位数", example: "SELECT MEDIAN(x) FROM t;" },
    ManualEntry { name: "PERCENTILE_CONT", category: ManualCategory::Aggregate, keywords: &["百分位", "分位数"], zh: "连续百分位", example: "SELECT PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY x) FROM t;" },
    ManualEntry { name: "COUNT_DISTINCT", category: ManualCategory::Aggregate, keywords: &["去重计数", "distinct"], zh: "去重后计数", example: "SELECT COUNT(DISTINCT col) FROM t;" },
    // ── 窗口 ──────────────────────────────────────────────
    ManualEntry { name: "ROW_NUMBER", category: ManualCategory::Window, keywords: &["行号", "序号", "排名"], zh: "行号（窗口）", example: "SELECT ROW_NUMBER() OVER (ORDER BY id) FROM t;" },
    ManualEntry { name: "RANK", category: ManualCategory::Window, keywords: &["排名", "并列"], zh: "排名（并列跳号）", example: "SELECT RANK() OVER (ORDER BY score DESC) FROM t;" },
    ManualEntry { name: "DENSE_RANK", category: ManualCategory::Window, keywords: &["排名", "连续排名"], zh: "排名（并列不跳号）", example: "SELECT DENSE_RANK() OVER (ORDER BY score DESC) FROM t;" },
    ManualEntry { name: "NTILE", category: ManualCategory::Window, keywords: &["分桶", "等分", "分位"], zh: "分成 N 桶", example: "SELECT NTILE(4) OVER (ORDER BY id) FROM t;" },
    ManualEntry { name: "LAG", category: ManualCategory::Window, keywords: &["上一行", "前一行", "环比"], zh: "取前一行值", example: "SELECT LAG(price) OVER (ORDER BY day) FROM t;" },
    ManualEntry { name: "LEAD", category: ManualCategory::Window, keywords: &["下一行", "后一行"], zh: "取后一行值", example: "SELECT LEAD(price) OVER (ORDER BY day) FROM t;" },
    ManualEntry { name: "FIRST_VALUE", category: ManualCategory::Window, keywords: &["窗口首值", "第一个"], zh: "窗口内第一个值", example: "SELECT FIRST_VALUE(x) OVER (ORDER BY day) FROM t;" },
    ManualEntry { name: "LAST_VALUE", category: ManualCategory::Window, keywords: &["窗口末值", "最后一个"], zh: "窗口内最后一个值", example: "SELECT LAST_VALUE(x) OVER (ORDER BY day) FROM t;" },
    ManualEntry { name: "CUME_DIST", category: ManualCategory::Window, keywords: &["累计分布"], zh: "累计分布比例", example: "SELECT CUME_DIST() OVER (ORDER BY x) FROM t;" },
    ManualEntry { name: "PERCENT_RANK", category: ManualCategory::Window, keywords: &["百分比排名"], zh: "百分比排名", example: "SELECT PERCENT_RANK() OVER (ORDER BY x) FROM t;" },
    ManualEntry { name: "SUM_WINDOW", category: ManualCategory::Window, keywords: &["累计求和", "running total"], zh: "累计求和（窗口 SUM）", example: "SELECT SUM(x) OVER (ORDER BY day) FROM t;" },
    ManualEntry { name: "AVG_WINDOW", category: ManualCategory::Window, keywords: &["移动平均", "滚动平均"], zh: "移动平均（窗口 AVG）", example: "SELECT AVG(x) OVER (ORDER BY day ROWS 2 PRECEDING) FROM t;" },
    // ── JSON ──────────────────────────────────────────────
    ManualEntry { name: "JSON_EXTRACT", category: ManualCategory::Json, keywords: &["json取", "取字段", "路径"], zh: "按路径取 JSON 值", example: "SELECT JSON_EXTRACT('{\"a\":1}', '$.a');" },
    ManualEntry { name: "JSON_QUERY", category: ManualCategory::Json, keywords: &["json取对象", "取对象"], zh: "取 JSON 对象/数组（保持 JSON）", example: "SELECT JSON_QUERY('{\"a\":{\"b\":1}}', '$.a');" },
    ManualEntry { name: "JSON_VALUE", category: ManualCategory::Json, keywords: &["json取标量", "取字符串"], zh: "取 JSON 标量值（转文本）", example: "SELECT JSON_VALUE('{\"a\":1}', '$.a');" },
    ManualEntry { name: "JSON_OBJECT", category: ManualCategory::Json, keywords: &["构造json", "生成json", "转json"], zh: "把键值对构造成 JSON", example: "SELECT JSON_OBJECT('a', 1, 'b', 2);" },
    ManualEntry { name: "JSON_ARRAY", category: ManualCategory::Json, keywords: &["构造数组"], zh: "把值构造成 JSON 数组", example: "SELECT JSON_ARRAY(1, 2, 3);" },
    ManualEntry { name: "JSON_LENGTH", category: ManualCategory::Json, keywords: &["json长度", "元素数"], zh: "JSON 数组长度/对象键数", example: "SELECT JSON_LENGTH('[1,2,3]');" },
    ManualEntry { name: "JSON_CONTAINS", category: ManualCategory::Json, keywords: &["json包含", "判断"], zh: "JSON 是否包含目标", example: "SELECT JSON_CONTAINS('{\"a\":1}', '1', '$.a');" },
    ManualEntry { name: "JSON_SET", category: ManualCategory::Json, keywords: &["改json", "插入字段"], zh: "设置/插入 JSON 字段", example: "SELECT JSON_SET('{\"a\":1}', '$.b', 2);" },
    ManualEntry { name: "JSON_INSERT", category: ManualCategory::Json, keywords: &["插入json字段"], zh: "仅当字段不存在时插入", example: "SELECT JSON_INSERT('{\"a\":1}', '$.b', 2);" },
    ManualEntry { name: "JSON_ARRAY_LENGTH", category: ManualCategory::Json, keywords: &["数组长度"], zh: "JSON 数组元素个数", example: "SELECT JSON_ARRAY_LENGTH('[1,2]');" },
    ManualEntry { name: "JSON_UNQUOTE", category: ManualCategory::Json, keywords: &["去引号", "json转文本"], zh: "去掉 JSON 值的引号", example: "SELECT JSON_UNQUOTE(JSON_EXTRACT('{\"a\":\"x\"}', '$.a'));" },
    ManualEntry { name: "JSON_TYPE", category: ManualCategory::Json, keywords: &["类型", "值类型"], zh: "返回 JSON 值类型", example: "SELECT JSON_TYPE('{\"a\":1}', '$.a');" },
    ManualEntry { name: "JSON_EXTRACT_STRING", category: ManualCategory::Json, keywords: &["json取字符串"], zh: "取 JSON 字符串字段（DuckDB）", example: "SELECT json_extract_string('{\"a\":\"x\"}', '$.a');" },
    ManualEntry { name: "TO_JSON", category: ManualCategory::Json, keywords: &["转json", "行转json"], zh: "值/行转 JSON", example: "SELECT TO_JSON(row(1, 'a'));" },
    // ── 类型转换 ──────────────────────────────────────────
    ManualEntry { name: "CAST", category: ManualCategory::Cast, keywords: &["类型转换", "转类型", "强转"], zh: "显式类型转换", example: "SELECT CAST('42' AS INTEGER);" },
    ManualEntry { name: "TRY_CAST", category: ManualCategory::Cast, keywords: &["安全转换", "容错转换"], zh: "转换失败返回 NULL", example: "SELECT TRY_CAST('abc' AS INTEGER);" },
    ManualEntry { name: "CONVERT", category: ManualCategory::Cast, keywords: &["类型转换", "转类型"], zh: "类型转换（方言变体）", example: "SELECT CONVERT(INTEGER, '42');" },
    ManualEntry { name: "TO_NUMBER", category: ManualCategory::Cast, keywords: &["转数字", "字符串转数字"], zh: "字符串转数字", example: "SELECT TO_NUMBER('42.5');" },
    ManualEntry { name: "TO_INTEGER", category: ManualCategory::Cast, keywords: &["转整数"], zh: "转整数", example: "SELECT TO_INTEGER('42');" },
    ManualEntry { name: "TO_STRING", category: ManualCategory::Cast, keywords: &["转字符串", "转文本"], zh: "转字符串", example: "SELECT TO_STRING(42);" },
    ManualEntry { name: "TO_BOOLEAN", category: ManualCategory::Cast, keywords: &["转布尔"], zh: "转布尔值", example: "SELECT TO_BOOLEAN('true');" },
    ManualEntry { name: "TO_BINARY", category: ManualCategory::Cast, keywords: &["转二进制"], zh: "转二进制/字节串", example: "SELECT TO_BINARY('abc');" },
    ManualEntry { name: "BINARY", category: ManualCategory::Cast, keywords: &["转字节", "二进制"], zh: "转二进制（MySQL）", example: "SELECT BINARY('abc');" },
    ManualEntry { name: "UNNEST", category: ManualCategory::Cast, keywords: &["数组展开", "炸开", "行转多行"], zh: "数组/JSON 展开成多行", example: "SELECT UNNEST(ARRAY[1,2,3]);" },
    // ── 逻辑条件 ──────────────────────────────────────────
    ManualEntry { name: "COALESCE", category: ManualCategory::Logic, keywords: &["空值兜底", "第一个非空", "默认值"], zh: "返回第一个非空值", example: "SELECT COALESCE(col, 0) FROM t;" },
    ManualEntry { name: "NULLIF", category: ManualCategory::Logic, keywords: &["相等变空", "除零"], zh: "两值相等返回 NULL", example: "SELECT NULLIF(a, b);" },
    ManualEntry { name: "IFNULL", category: ManualCategory::Logic, keywords: &["空值兜底", "默认值"], zh: "为空时的兜底值（MySQL/SQLite）", example: "SELECT IFNULL(col, '未知');" },
    ManualEntry { name: "ISNULL", category: ManualCategory::Logic, keywords: &["空值兜底", "nvl"], zh: "为空时的兜底值（SQL Server）", example: "SELECT ISNULL(col, 0);" },
    ManualEntry { name: "NVL", category: ManualCategory::Logic, keywords: &["空值兜底", "默认值"], zh: "为空时的兜底值（Oracle/PG）", example: "SELECT NVL(col, 0);" },
    ManualEntry { name: "IF", category: ManualCategory::Logic, keywords: &["如果", "条件", "判断"], zh: "条件成立返回 A 否则 B", example: "SELECT IF(x > 0, '正', '负');" },
    ManualEntry { name: "IIF", category: ManualCategory::Logic, keywords: &["如果", "条件"], zh: "条件成立返回 A 否则 B（PG/DuckDB）", example: "SELECT IIF(x > 0, '正', '负');" },
    ManualEntry { name: "CASE", category: ManualCategory::Logic, keywords: &["分支", "多条件", "when"], zh: "多分支条件表达式", example: "SELECT CASE WHEN x > 0 THEN '正' ELSE '非正' END;" },
    ManualEntry { name: "BOOL_AND", category: ManualCategory::Logic, keywords: &["全真", "every"], zh: "组内是否全为真", example: "SELECT BOOL_AND(flag) FROM t;" },
    ManualEntry { name: "BOOL_OR", category: ManualCategory::Logic, keywords: &["任一为真", "any"], zh: "组内是否有真", example: "SELECT BOOL_OR(flag) FROM t;" },
    // ── 系统信息 ──────────────────────────────────────────
    ManualEntry { name: "VERSION", category: ManualCategory::System, keywords: &["版本", "数据库版本"], zh: "数据库版本号", example: "SELECT VERSION();" },
    ManualEntry { name: "DATABASE", category: ManualCategory::System, keywords: &["当前库", "库名"], zh: "当前数据库名", example: "SELECT DATABASE();" },
    ManualEntry { name: "CURRENT_USER", category: ManualCategory::System, keywords: &["当前用户", "用户名"], zh: "当前用户", example: "SELECT CURRENT_USER;" },
    ManualEntry { name: "USER", category: ManualCategory::System, keywords: &["当前用户"], zh: "当前用户（别名）", example: "SELECT USER();" },
    ManualEntry { name: "SESSION_USER", category: ManualCategory::System, keywords: &["会话用户"], zh: "会话用户", example: "SELECT SESSION_USER;" },
    ManualEntry { name: "UUID", category: ManualCategory::System, keywords: &["唯一标识", "guid", "随机id"], zh: "生成 UUID", example: "SELECT UUID();" },
    ManualEntry { name: "GEN_RANDOM_UUID", category: ManualCategory::System, keywords: &["唯一标识", "uuid"], zh: "生成 UUID（PG）", example: "SELECT gen_random_uuid();" },
    ManualEntry { name: "SHA256", category: ManualCategory::System, keywords: &["哈希", "摘要"], zh: "SHA-256 哈希", example: "SELECT SHA256('abc');" },
    ManualEntry { name: "HASH", category: ManualCategory::System, keywords: &["哈希", "散列"], zh: "通用哈希（DuckDB）", example: "SELECT HASH('abc');" },
    ManualEntry { name: "PG_BACKEND_PID", category: ManualCategory::System, keywords: &["会话id", "进程id"], zh: "当前会话进程号（PG）", example: "SELECT pg_backend_pid();" },
    ManualEntry { name: "CURRENT_SCHEMA", category: ManualCategory::System, keywords: &["当前模式", "schema"], zh: "当前 Schema（PG）", example: "SELECT CURRENT_SCHEMA();" },
    ManualEntry { name: "QUERY_PARAMETER", category: ManualCategory::System, keywords: &["参数", "绑定参数"], zh: "取查询参数（DuckDB）", example: "SELECT query_parameter('name');" },
];

/// 中英混搜：query 匹配 函数名 / 中文关键词 / 中文描述 任一维度
///
/// 排序优先级：函数名前缀 > 函数名包含 > 中文关键词 > 中文描述；
/// 同级按函数名长度（短者近）再按字典序，结果稳定。
pub fn search_manual(query: &str) -> Vec<&'static ManualEntry> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return MANUAL.iter().collect();
    }
    let mut scored: Vec<(u8, &'static ManualEntry)> = MANUAL
        .iter()
        .filter_map(|e| score(e, &q).map(|s| (s, e)))
        .collect();
    scored.sort_by(|(sa, ea), (sb, eb)| {
        sb.cmp(sa)
            .then_with(|| ea.name.len().cmp(&eb.name.len()))
            .then_with(|| ea.name.cmp(eb.name))
    });
    scored.into_iter().map(|(_, e)| e).collect()
}

fn score(e: &'static ManualEntry, q: &str) -> Option<u8> {
    let name = e.name.to_lowercase();
    if name.starts_with(q) {
        return Some(4);
    }
    if name.contains(q) {
        return Some(3);
    }
    if e.keywords.iter().any(|k| k.to_lowercase().contains(q)) {
        return Some(2);
    }
    if e.zh.contains(q) {
        return Some(1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_covers_all_categories() {
        for cat in [
            ManualCategory::String,
            ManualCategory::Numeric,
            ManualCategory::DateTime,
            ManualCategory::Aggregate,
            ManualCategory::Window,
            ManualCategory::Json,
            ManualCategory::Cast,
            ManualCategory::Logic,
            ManualCategory::System,
        ] {
            let n = MANUAL.iter().filter(|e| e.category == cat).count();
            assert!(n >= 10, "分类 {:?} 至少 10 条（当前 {}）", cat, n);
        }
        assert!(MANUAL.len() >= 150, "手册总量 >= 150（当前 {}）", MANUAL.len());
    }

    #[test]
    fn chinese_keyword_finds_substr() {
        let hits = search_manual("截取");
        assert!(
            hits.iter().any(|e| e.name == "SUBSTR"),
            "「截取」应命中 SUBSTR，实际 {:?}",
            hits.iter().map(|e| e.name).take(5).collect::<Vec<_>>()
        );
    }

    #[test]
    fn english_prefix_ranks_first() {
        let hits = search_manual("sub");
        assert!(!hits.is_empty());
        let first = hits[0].name.to_lowercase();
        assert!(first.starts_with("sub"), "前缀命中排第一（实际 {}）", first);
    }

    #[test]
    fn description_search_finds_coalesce_family() {
        let hits = search_manual("空值兜底");
        for want in ["COALESCE", "IFNULL", "NVL"] {
            assert!(
                hits.iter().any(|e| e.name == want),
                "「空值兜底」应命中 {}，实际 {:?}",
                want,
                hits.iter().map(|e| e.name).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn empty_query_returns_all() {
        assert_eq!(search_manual("").len(), MANUAL.len());
        assert_eq!(search_manual("   ").len(), MANUAL.len());
    }

    #[test]
    fn names_are_unique() {
        let mut names: Vec<&str> = MANUAL.iter().map(|e| e.name).collect();
        names.sort_unstable();
        let dup: Vec<_> = names.windows(2).filter(|w| w[0] == w[1]).collect();
        assert!(dup.is_empty(), "函数名不允许重复：{:?}", dup);
    }

    #[test]
    fn search_is_case_insensitive() {
        assert_eq!(
            search_manual("SUBSTR").len(),
            search_manual("substr").len(),
            "大小写不敏感"
        );
    }
}
