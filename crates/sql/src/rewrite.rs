//! 脚本级重写骨架：**逐条改写 + 原位回填**
//!
//! ## 为什么需要它
//!
//! sqlglot 的**整篇接口对脚本不安全**：`transpile("SELECT 1; SELECT 2;")` 会
//! **静默丢弃**第二条及以后的语句（实测，架构 §12 #19）——生产路径
//! `SqlEngine::transpile` 同样如此，而且调用方拿到的是 `Ok`，看不出少了东西。
//! 另一头，整篇 `parse_statements` 也不行：脚本里有一句没写完，**整篇**都失败。
//!
//! 所以凡“把脚本换成另一份文本”的动作（格式化、方言转译）都走同一条路：
//!
//! 1. 用 P0.4 的词法切分（[`super::split::split_statements`]）拿到每条语句的**字节区间**；
//! 2. 逐条交给调用方的改写函数（拿不到结果就**原样保留**并计数，不静默丢）；
//! 3. 回填原位——区间之外的注释 / 空行 / 没写完的那半句**一个字节都不动**。
//!
//! 语句之间的**空白**会被规整为「`;` + 两个换行」（读感，DBeaver 也这么做）；
//! 但只要那一段里含注释或其它内容，就**原样保留**（注释归用户，不归我们）。

use super::split::split_statements;

/// 一次脚本级改写的结果（[`super::formatter::FormatReport`] 与
/// [`super::transpiler::TranspileReport`] 都是它的投影）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRewrite {
    /// 改写后的整篇文本（改写不了的语句**逐字保留**）
    pub text: String,
    /// 成功改写的语句数
    pub rewritten: usize,
    /// 改写不了、逐字保留的语句数（要报给用户，别让人以为“按了没反应”）
    pub kept_verbatim: usize,
}

/// 逐条改写脚本
///
/// `rewrite` 收一条语句的原文，返回改写后的文本；返回 `None` 表示这一条改写不了
/// （解析失败 / 未写完），调用方（这里）会把它逐字保留。
pub fn rewrite_statements(
    sql: &str,
    mut rewrite: impl FnMut(&str) -> Option<String>,
) -> ScriptRewrite {
    let spans = split_statements(sql);
    if spans.is_empty() {
        return ScriptRewrite {
            text: sql.to_string(),
            rewritten: 0,
            kept_verbatim: 0,
        };
    }

    let mut out = String::with_capacity(sql.len() + sql.len() / 8);
    let mut cursor = 0usize;
    let mut rewritten = 0usize;
    let mut kept_verbatim = 0usize;

    for (index, span) in spans.iter().enumerate() {
        if span.start < cursor || span.end > sql.len() {
            continue; // 防御：区间不合法就跳过（不该发生）
        }
        // 语句之间的内容：注释与空行都在这里
        let gap = &sql[cursor..span.start];
        out.push_str(&normalize_gap(gap, index == 0));

        let body = span.text(sql);
        match rewrite(body) {
            Some(text) => {
                out.push_str(&text);
                rewritten += 1;
            }
            None => {
                out.push_str(body);
                kept_verbatim += 1;
            }
        }
        cursor = span.end;
    }
    // 尾部（最后一句之后的分号 / 空白 / 注释）
    out.push_str(&normalize_tail(&sql[cursor..]));

    ScriptRewrite {
        text: out,
        rewritten,
        kept_verbatim,
    }
}

/// 语句之间的空白：只有空白与分号时规整成 `;\n\n`；含注释或其它内容就原样保留
fn normalize_gap(gap: &str, is_first: bool) -> String {
    // 分号属于 gap（语句区间不含尾分号），所以“只有空白 + 分号”才是可规整的形状
    let only_separators = gap.chars().all(|ch| ch.is_whitespace() || ch == ';');
    if only_separators {
        if is_first {
            // 开头到第一条语句之间：只留空白（不凭空插换行）
            return String::new();
        }
        return ";\n\n".to_string();
    }
    // 含注释：原样（注释归用户）
    gap.to_string()
}

/// 尾部：空白规整成单个换行；有注释就原样
fn normalize_tail(tail: &str) -> String {
    if tail.trim().is_empty() {
        return if tail.is_empty() {
            String::new()
        } else {
            "\n".to_string()
        };
    }
    tail.to_string()
}

#[cfg(test)]
mod tests {
    // 安全模式：**不通配导入**
    use super::rewrite_statements;

    #[test]
    fn every_statement_gets_rewritten_in_place() {
        let out = rewrite_statements("select 1;\nselect 2;", |body| {
            Some(body.to_uppercase())
        });
        assert_eq!(out.rewritten, 2);
        assert_eq!(out.kept_verbatim, 0);
        assert!(out.text.contains("SELECT 1"), "{:?}", out.text);
        assert!(out.text.contains("SELECT 2"), "{:?}", out.text);
        assert!(out.text.contains(";\n\n"), "语句之间要拉开：{:?}", out.text);
    }

    #[test]
    fn statements_the_rewriter_rejects_are_kept_verbatim() {
        let out = rewrite_statements("select 1;\nselect (\nselect 3;", |body| {
            if body.contains("select (") {
                None
            } else {
                Some(body.to_uppercase())
            }
        });
        assert_eq!(out.rewritten, 1);
        assert_eq!(out.kept_verbatim, 1);
        assert!(out.text.contains("select (\nselect 3"), "{:?}", out.text);
    }

    #[test]
    fn comments_outside_statements_are_left_alone() {
        let out = rewrite_statements("-- 头注释\nselect 1; -- 尾注释\nselect 2;", |body| {
            Some(body.to_uppercase())
        });
        assert!(out.text.contains("-- 头注释"), "{:?}", out.text);
        assert!(out.text.contains("-- 尾注释"), "{:?}", out.text);
    }

    #[test]
    fn an_empty_script_comes_back_unchanged() {
        let out = rewrite_statements("   \n", |body| Some(body.to_uppercase()));
        assert_eq!(out.text, "   \n");
        assert_eq!(out.rewritten, 0);
    }
}
