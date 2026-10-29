//! 日志敏感数据脱敏
//!
//! 对日志消息中的连接字符串、密码等敏感信息进行掩码处理，
//! 防止密码、密钥等泄露到持久化存储。

/// 脱敏敏感信息后的日志消息
pub fn redact_sensitive(message: &str) -> String {
    let mut result = message.to_string();

    // URL 格式: scheme://user:password@host → scheme://user:***@host
    result = redact_url_password(&result);

    // key=value 格式: password=xxx, pwd=xxx, pass=xxx
    result = redact_key_value(&result, "password");
    result = redact_key_value(&result, "pwd");
    result = redact_key_value(&result, "pass");
    result = redact_key_value(&result, "passwd");
    result = redact_key_value(&result, "secret");

    result
}

/// 掩掉 URL 里的密码：`scheme://user:password@host` → `scheme://user:***@host`。
///
/// 一行里的**每个** URL 都要处理（旧实现命中第一个就 `return`，
/// 一行里有两个连接串时第二个的密码会原样进日志），并且不能把 URL 之前的内容重复输出
/// （旧实现从字符串开头拼接，带前缀的日志会多出一份前缀）。
fn redact_url_password(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 3 < bytes.len() && &bytes[i..i + 3] == b"://" {
            result.push_str("://");
            let auth_start = i + 3;
            let rest = &s[auth_start..];
            // 权限段止于路径 / 参数 / 空白 / 引号：不能在整个剩余串里找 `@`，
            // 否则 `mysql://host/db?u=a@b` 这种会把查询串里的 `@` 当权限段
            let end = rest
                .find(|c: char| matches!(c, '/' | '?' | '#' | ' ' | '\t' | '\n' | '\r' | '"' | '\''))
                .unwrap_or(rest.len());
            let authority = &rest[..end];
            match authority.rfind('@') {
                Some(at_pos) => {
                    let auth = &authority[..at_pos];
                    if let Some(colon_pos) = auth.find(':') {
                        result.push_str(&auth[..=colon_pos]);
                        result.push_str("***");
                    } else {
                        result.push_str(auth);
                    }
                    result.push_str(&authority[at_pos..]); // '@' 及其后（host:port）
                }
                None => result.push_str(authority),
            }
            i = auth_start + end;
            continue;
        }
        let ch = s[i..].chars().next().unwrap_or(' ');
        result.push(ch);
        i += ch.len_utf8();
    }
    result
}

fn redact_key_value(s: &str, key: &str) -> String {
    let lower = s.to_lowercase();
    let search = format!("{}=", key.to_lowercase());
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let remaining = &lower[i..];
        if remaining.starts_with(&search)
            && (i == 0
                || bytes[i - 1].is_ascii_whitespace()
                || bytes[i - 1] == b';'
                || bytes[i - 1] == b'&'
                || bytes[i - 1] == b'?')
        {
            result.push_str(&s[i..i + key.len() + 1]);
            i += key.len() + 1;
            // 跳过值直到遇到空格、分号、& 或字符串结束
            while i < bytes.len()
                && !bytes[i].is_ascii_whitespace()
                && bytes[i] != b';'
                && bytes[i] != b'&'
            {
                i += 1;
            }
            result.push_str("***");
        } else {
            let ch = s[i..].chars().next().unwrap_or(' ');
            result.push(ch);
            i += ch.len_utf8();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_url_password() {
        let input = "mysql://root:secret123@localhost:3306/mydb";
        let result = redact_sensitive(input);
        assert_eq!(result, "mysql://root:***@localhost:3306/mydb");
    }

    #[test]
    fn test_redact_key_value_password() {
        let input = "password=mysecret123 db=test";
        let result = redact_sensitive(input);
        assert_eq!(result, "password=*** db=test");
    }

    #[test]
    fn test_redact_key_value_pwd() {
        let input = "pwd=abc123 host=localhost";
        let result = redact_sensitive(input);
        assert_eq!(result, "pwd=*** host=localhost");
    }

    #[test]
    fn test_redact_no_sensitive() {
        let input = "SELECT * FROM users WHERE id = 1";
        let result = redact_sensitive(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_redact_multiple() {
        let input = "conn: password=abc pwd=def user=john";
        let result = redact_sensitive(input);
        assert_eq!(result, "conn: password=*** pwd=*** user=john");
    }

    #[test]
    fn test_redact_url_with_prefix_keeps_one_copy() {
        let input = "连接 mysq://x 失败：mysql://root:secret@localhost/db";
        let result = redact_sensitive(input);
        assert_eq!(result, "连接 mysq://x 失败：mysql://root:***@localhost/db");
    }

    #[test]
    fn test_redact_every_url_on_the_line() {
        let input = "primary mysql://u:p1@a:3306/x backup postgres://u:p2@b:5432/y";
        let result = redact_sensitive(input);
        assert_eq!(
            result,
            "primary mysql://u:***@a:3306/x backup postgres://u:***@b:5432/y"
        );
    }

    #[test]
    fn test_redact_url_without_credentials() {
        let input = "duckdb:///data/analytics.duckdb";
        assert_eq!(redact_sensitive(input), input);
        assert_eq!(
            redact_sensitive("postgres://user@host:5432/db"),
            "postgres://user@host:5432/db"
        );
    }

    #[test]
    fn test_at_sign_in_query_is_not_a_credential() {
        let input = "sqlite:///tmp/x.db?mode=ro&user=a@b";
        assert_eq!(redact_sensitive(input), input);
    }
}
