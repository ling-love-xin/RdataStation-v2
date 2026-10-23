//! rds-search：内容搜索独立 crate（用户点名 2026-10-10 抽取，供命令面板 / 全局搜索复用）。
//!
//! ## 职责与边界
//!
//! - **后端**（本文件）：目录递归扫描（深度上限 4、跳过隐藏项，与草稿箱 `store.rs` 同一口径）
//!   + 文件内容逐行匹配 → [`SearchMatch`] / [`SearchResult`]。
//! - **面板渲染**（第二阶段迁入）：`render_search_pane` 按文件分组命中列表，
//!   类型图标（`type_icon`）由调用方注入，与 scratchpad 视图完全解耦。
//! - **不依赖** scratchpad 的任何类型；错误用 `anyhow`（调用方自行转字符串展示）。
//!
//! ## 口径（与草稿箱搜索一致）
//!
//! - 逐行 `is_match`（正则 / 大小写由调用方传入），上下文行在命中周围截取；
//! - 命中结果上限 [`MAX_SEARCH_RESULTS`]，超出置 `truncated`；
//! - 单文件读取超时 [`SEARCH_PER_FILE_TIMEOUT_SECS`] 防卡死。

/// 内容搜索命中项（行级）。
#[derive(Clone, Debug)]
pub struct SearchMatch {
    /// 模块内相对路径（调用方拼成绝对路径）。
    pub file: String,
    /// 1 基行号。
    pub line_number: usize,
    /// 命中行内容。
    pub line_content: String,
    /// 命中行前若干行（上下文）。
    pub before_context: Vec<String>,
    /// 命中行后若干行（上下文）。
    pub after_context: Vec<String>,
    /// 行内命中区间（字节偏移，首个命中在 `match_spans[0]`）。
    pub match_spans: Vec<(usize, usize)>,
}

/// 一次目录搜索的结果汇总。
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub matches: Vec<SearchMatch>,
    /// 实际扫描的文件数。
    pub total_files_scanned: usize,
    /// 命中是否被截断（达到 [`MAX_SEARCH_RESULTS`]）。
    pub truncated: bool,
}

/// 扫描深度上限（防异常深层目录拖垮搜索）。
pub const MAX_DEPTH: u32 = 4;
/// 命中结果上限。
pub const MAX_SEARCH_RESULTS: usize = 500;
/// 单文件读取超时（秒）。
pub const SEARCH_PER_FILE_TIMEOUT_SECS: u64 = 30;
/// 单行最多记录多少个命中区间（防正则贪婪吞内存）。
const MAX_SPANS_PER_LINE: usize = 16;

/// 简单 glob 匹配（VSCode「包含/排除文件」语义的轻量实现）。
///
/// - `*`：匹配任意字符序列（含路径分隔符，即跨目录）；`?`：匹配单个非分隔符字符；
/// - 其余字符字面匹配；模式整体与**相对路径全串**匹配（如 `*.log` 命中 `a.log` / `sub/a.log`；
///   `test-*` 命中 `test-run2.log`）。
/// - 不支持 `[]`、`{}`、`**` 的特殊语义（`**` 退化为 `*`）——足够覆盖常见过滤场景，
///   复杂 glob 后续按需升级。
pub fn glob_matches(pattern: &str, path: &str) -> bool {
    fn rec(p: &[u8], s: &[u8]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some(b'*') => (0..=s.len()).any(|i| rec(&p[1..], &s[i..])),
            Some(b'?') => {
                if s.is_empty() || s[0] == b'/' {
                    false
                } else {
                    rec(&p[1..], &s[1..])
                }
            }
            Some(c) => !s.is_empty() && s[0] == *c && rec(&p[1..], &s[1..]),
        }
    }
    rec(pattern.as_bytes(), path.as_bytes())
}

/// 递归扫描目录（深度上限、跳过隐藏项），返回扁平文件路径清单。
///
/// 口径与草稿箱 `store::scan_dir_tree` 一致：`.` 开头（`.git`、`.RSmeta`、点文件）不进清单。
/// `include` / `exclude` 为 VSCode 式文件过滤 glob（相对 root 的路径全串匹配，
/// 见 [`glob_matches`]）；扫描即过滤（被排除文件不进清单、不计扫描数）。
/// 迭代实现（async 递归会触发 E0733）。
pub async fn collect_files(
    root: &std::path::Path,
    include: Option<&str>,
    exclude: Option<&str>,
    out: &mut Vec<std::path::PathBuf>,
) {
    let mut stack: Vec<(std::path::PathBuf, u32)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_DEPTH {
            continue;
        }
        let Ok(mut read_dir) = tokio::fs::read_dir(&dir).await else {
            continue;
        };
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                stack.push((path, depth + 1));
            } else if entry
                .file_type()
                .await
                .map(|t| t.is_file())
                .unwrap_or(false)
            {
                // 过滤按相对 root 的路径全串匹配（分隔符统一 `/` 比较）。
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                if let Some(inc) = include {
                    if !glob_matches(inc, &rel) {
                        continue;
                    }
                }
                if let Some(exc) = exclude {
                    if glob_matches(exc, &rel) {
                        continue;
                    }
                }
                out.push(path);
            }
        }
    }
}

/// 读取单个文件并逐行匹配，产出 [`SearchMatch`]。
async fn search_open(
    path: std::path::PathBuf,
    query: String,
    case_sensitive: bool,
    query_lower: String,
    regex: Option<&regex::Regex>,
    rel_path: String,
    max_results: usize,
    context_lines: usize,
) -> anyhow::Result<Vec<SearchMatch>> {
    let file = tokio::fs::File::open(&path).await?;
    let reader = tokio::io::BufReader::new(file);
    use tokio::io::AsyncBufReadExt as _;
    let mut lines = reader.lines();
    let mut all_lines: Vec<String> = Vec::new();
    while let Ok(Some(line)) = lines.next_line().await {
        all_lines.push(line);
    }

    let mut matches = Vec::new();
    for (i, line) in all_lines.iter().enumerate() {
        if matches.len() >= max_results {
            break;
        }
        let line_number = i + 1;
        let found = match regex {
            Some(re) => re.is_match(line),
            None if case_sensitive => line.contains(&query),
            None => line.to_lowercase().contains(&query_lower),
        };
        if found {
            let before: Vec<String> = if context_lines > 0 {
                let start = i.saturating_sub(context_lines);
                all_lines[start..i].to_vec()
            } else {
                Vec::new()
            };
            let after: Vec<String> = if context_lines > 0 {
                let end = (i + 1 + context_lines).min(all_lines.len());
                all_lines[i + 1..end].to_vec()
            } else {
                Vec::new()
            };
            let match_spans = match regex {
                Some(re) => re
                    .find_iter(line)
                    .take(MAX_SPANS_PER_LINE)
                    .map(|m| (m.start(), m.end()))
                    .collect(),
                None => literal_match_spans(line, &query, case_sensitive),
            };
            matches.push(SearchMatch {
                file: rel_path.clone(),
                line_number,
                line_content: line.clone(),
                before_context: before,
                after_context: after,
                match_spans,
            });
        }
    }
    Ok(matches)
}

/// 字面量（非正则）命中的行内区间。
///
/// K5（2026-10-09）：大小写不敏感时先小写化再查找，但 Unicode 小写化可改变字节长度
/// （`İ` → `i̇`），旧实现直接放弃高亮，造成「命中但不高亮」。现改为**逐字符小写 + 字符级
/// 偏移映射**（`charwise_lowercase`），把小写文本里的命中区间正确回射到原行。
fn literal_match_spans(line: &str, query: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut spans = Vec::new();
    if case_sensitive {
        let mut from = 0usize;
        while spans.len() < MAX_SPANS_PER_LINE {
            let Some(pos) = line[from..].find(query) else {
                break;
            };
            let start = from + pos;
            let end = start + query.len();
            spans.push((start, end));
            from = end;
        }
        return spans;
    }

    let (lower_line, orig) = charwise_lowercase(line);
    let lower_query = query.to_lowercase();
    if lower_query.is_empty() {
        return Vec::new();
    }
    // 原行每个字符的字节起点，末尾补 `line.len()`（原字符区间 = `char_starts[a]..char_starts[b+1]`）。
    let mut char_starts: Vec<usize> = line.char_indices().map(|(i, _)| i).collect();
    char_starts.push(line.len());
    let mut from = 0usize;
    while spans.len() < MAX_SPANS_PER_LINE {
        let Some(pos) = lower_line[from..].find(&lower_query) else {
            break;
        };
        let hit_start = from + pos;
        let hit_end = hit_start + lower_query.len();
        // 命中区间的字节位置 → 小写文本的字符索引（`orig` 的单位就是字符序号）。
        let ci = lower_line[..hit_start].chars().count();
        let cj = lower_line[..hit_end].chars().count();
        if ci >= orig.len() || cj == 0 || cj > orig.len() || ci >= cj {
            break;
        }
        let start = char_starts[orig[ci]];
        let end = char_starts[orig[cj - 1] + 1];
        spans.push((start, end));
        from = hit_end;
    }
    spans
}

/// 逐字符小写化，返回（小写文本, 小写字符 → 原行字符序号）。
///
/// Unicode 小写化可把一个字符展开成多个字符（`İ` → `i` + 组合附加符），
/// 因此小写文本里**一个字符对应原行一个字符**的映射不能用字节偏移，必须逐字符记录。
fn charwise_lowercase(line: &str) -> (String, Vec<usize>) {
    let mut lower = String::new();
    let mut orig = Vec::new();
    for (char_idx, (_, ch)) in line.char_indices().enumerate() {
        for c in ch.to_lowercase() {
            lower.push(c);
            orig.push(char_idx);
        }
    }
    (lower, orig)
}

/// 目录内容搜索：递归扫描 + 逐行匹配。
///
/// `root` 为待搜索目录（绝对路径）；命中 `file` 为相对 `root` 的路径。
/// `include` / `exclude` 为文件过滤 glob（VSCode「包含的文件 / 排除的文件」，见 [`glob_matches`]）。
/// `whole_word` = 全字匹配（VSCode `ab` 开关，2026-10-11 原型差异补齐）：非正则模式
/// 会把 query 转义后包 `\b(?:…)\b`，正则模式直接在用户表达式外包 `\b(?:…)\b`——两种
/// 模式都落到 regex 匹配（span 由 `Regex::find_iter` 给出，字节区间正确，无需走
/// [`literal_match_spans`]）。
pub async fn search_directory(
    root: &std::path::Path,
    query: &str,
    case_sensitive: bool,
    is_regex: bool,
    whole_word: bool,
    context_lines: usize,
    include: Option<&str>,
    exclude: Option<&str>,
) -> anyhow::Result<SearchResult> {
    let mut files = Vec::new();
    collect_files(root, include, exclude, &mut files).await;

    let mut matches = Vec::new();
    let mut total_scanned = 0usize;
    let mut truncated = false;
    let query_lower = query.to_lowercase();
    let query_owned = query.to_string();

    // whole_word 统一经 regex 编译（非正则模式转义后包字边界）。
    let regex = if is_regex || whole_word {
        let effective: String = match (is_regex, whole_word) {
            (true, true) => format!("\\b(?:{query})\\b"),
            (false, true) => format!("\\b(?:{})\\b", regex::escape(query)),
            (true, false) => query.to_string(),
            (false, false) => unreachable!(),
        };
        Some(
            regex::RegexBuilder::new(&effective)
                .case_insensitive(!case_sensitive)
                .build()?,
        )
    } else {
        None
    };

    for path in files {
        if truncated {
            break;
        }
        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        let remaining = MAX_SEARCH_RESULTS.saturating_sub(matches.len());
        if remaining == 0 {
            truncated = true;
            break;
        }
        let future = search_open(
            path.clone(),
            query_owned.clone(),
            case_sensitive,
            query_lower.clone(),
            regex.as_ref(),
            rel_path,
            remaining,
            context_lines,
        );
        match tokio::time::timeout(
            std::time::Duration::from_secs(SEARCH_PER_FILE_TIMEOUT_SECS),
            future,
        )
        .await
        {
            Ok(Ok(mut file_matches)) => {
                total_scanned += 1;
                matches.append(&mut file_matches);
            }
            _ => {
                total_scanned += 1;
            }
        }
        if matches.len() >= MAX_SEARCH_RESULTS {
            truncated = true;
        }
    }

    Ok(SearchResult {
        matches,
        total_files_scanned: total_scanned,
        truncated,
    })
}

/// 搜索面板渲染（VS Code 形态：按文件分组 + 命中数 + 行号命中行 + 替换栏）。
///
/// 与后端解耦：`type_icon` 由调用方注入（scratchpad / 命令面板各自提供文件类型图标）；
/// 视图模型 [`view::SearchView`] / [`view::SearchHit`] 由调用方从后端 `SearchResult` 组装。
pub mod view;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn search_directory_finds_lines_and_spans() {
        let dir = std::env::temp_dir().join(format!("rds-search-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.log"), "hello world\nCompiling foo\nnothing here\n").unwrap();
        std::fs::write(dir.join("sub/b.txt"), "Compiling bar\n").unwrap();
        std::fs::write(dir.join(".hidden"), "Compiling hidden\n").unwrap();

        let res = search_directory(&dir, "Compiling", false, false, false, 0, None, None)
            .await
            .unwrap();
        assert_eq!(res.matches.len(), 2, "隐藏项与无命中文件不计入");
        assert_eq!(res.total_files_scanned, 2, "只统计被扫描的普通文件");
        // read_dir 顺序不保证：按文件名定位，不依赖返回顺序。
        let a = res.matches.iter().find(|m| m.file == "a.log").unwrap();
        assert_eq!(a.line_number, 2);
        assert_eq!(a.line_content, "Compiling foo");
        assert_eq!(a.match_spans, vec![(0, 9)], "命中区间字节偏移");
        // Windows 下相对路径分隔符是 `\`，统一转 `/` 再比。
        let b = res
            .matches
            .iter()
            .find(|m| m.file.replace('\\', "/") == "sub/b.txt")
            .unwrap();
        assert_eq!(b.line_content, "Compiling bar");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn search_directory_case_and_regex() {
        let dir = std::env::temp_dir().join(format!("rds-search-test2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.log"), "COMPILING upper\ncompile low\n").unwrap();

        // 大小写不敏感（默认）
        let res = search_directory(&dir, "compiling", false, false, false, 0, None, None)
            .await
            .unwrap();
        assert_eq!(res.matches.len(), 1, "小写查询命中大写内容");

        // 大小写敏感：`COMPILING upper` 不命中，`compile low` 命中。
        let res = search_directory(&dir, "compile", true, false, false, 0, None, None)
            .await
            .unwrap();
        assert_eq!(res.matches.len(), 1, "敏感模式只命中小写行");
        assert_eq!(res.matches[0].line_content, "compile low");

        // 正则
        let res = search_directory(&dir, r"compile\s+low", false, true, false, 0, None, None)
            .await
            .unwrap();
        assert_eq!(res.matches.len(), 1);
        assert_eq!(res.matches[0].line_content, "compile low");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn search_directory_whole_word() {
        let dir = std::env::temp_dir().join(format!("rds-search-test4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("w.log"), "id = 1\ninside grid\nsubid value\n").unwrap();

        // 全字匹配（ab）：`id` 只命中整词行，不命中 `inside` / `subid` 内的子串。
        let res = search_directory(&dir, "id", false, false, true, 0, None, None)
            .await
            .unwrap();
        assert_eq!(res.matches.len(), 1, "全字匹配只命中整词");
        assert_eq!(res.matches[0].line_content, "id = 1");
        assert_eq!(res.matches[0].match_spans, vec![(0, 2)], "span 仍是字节区间");

        // 非全字：三行都命中。
        let res = search_directory(&dir, "id", false, false, false, 0, None, None)
            .await
            .unwrap();
        assert_eq!(res.matches.len(), 3, "非全字命中所有包含子串的行");

        // 正则 + 全字：表达式外包 \b，`\d+` 整词匹配。
        let res = search_directory(&dir, r"\d+", false, true, true, 0, None, None)
            .await
            .unwrap();
        assert_eq!(res.matches.len(), 1, "正则全字命中 'id = 1' 的数字");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn search_directory_include_exclude_glob() {
        let dir = std::env::temp_dir().join(format!("rds-search-test3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.log"), "Compiling a\n").unwrap();
        std::fs::write(dir.join("b.sql"), "Compiling b\n").unwrap();
        std::fs::write(dir.join("sub/c.log"), "Compiling c\n").unwrap();

        // include=*.log：只搜 log，不搜 sql
        let res = search_directory(&dir, "Compiling", false, false, false, 0, Some("*.log"), None)
            .await
            .unwrap();
        let files: Vec<String> = res
            .matches
            .iter()
            .map(|m| m.file.replace('\\', "/"))
            .collect();
        assert_eq!(files.len(), 2, "a.log + sub/c.log");
        assert!(files.iter().all(|f| f.ends_with(".log")));

        // exclude=*.sql：排除 sql（与 include 独立生效）
        let res = search_directory(&dir, "Compiling", false, false, false, 0, None, Some("*.sql"))
            .await
            .unwrap();
        let files: Vec<String> = res
            .matches
            .iter()
            .map(|m| m.file.replace('\\', "/"))
            .collect();
        assert_eq!(files.len(), 2, "a.log + sub/c.log");

        // include=test-*：前缀匹配
        std::fs::write(dir.join("test-run2.log"), "Compiling t\n").unwrap();
        let res = search_directory(&dir, "Compiling", false, false, false, 0, Some("test-*"), None)
            .await
            .unwrap();
        assert_eq!(res.matches.len(), 1);
        assert_eq!(res.matches[0].file, "test-run2.log");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn glob_matches_basic() {
        assert!(glob_matches("*.log", "a.log"));
        assert!(glob_matches("*.log", "sub/a.log"), "`*` 跨目录");
        assert!(!glob_matches("*.log", "a.sql"));
        assert!(glob_matches("test-*", "test-run2.log"));
        assert!(glob_matches("*.log", "a.log"));
        assert!(glob_matches("a?c", "abc"));
        assert!(!glob_matches("a?c", "a/c"), "`?` 不匹配分隔符");
        assert!(glob_matches("sub/*.log", "sub/c.log"));
        assert!(!glob_matches("sub/*.log", "c.log"));
    }
}
