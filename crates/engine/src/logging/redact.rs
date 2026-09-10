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

fn redact_url_password(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        // 查找 :// 模式
        if i + 3 < bytes.len() && bytes[i] == b':' && bytes[i + 1] == b'/' && bytes[i + 2] == b'/' {
            let scheme_end = i;
            let auth_start = i + 3;
            result.push_str(&s[..=scheme_end]);
            result.push_str("//");

            // 查找 @ 符号，@ 之前是 user:password
            let rest = &s[auth_start..];
            if let Some(at_pos) = rest.find('@') {
                let auth = &rest[..at_pos];
                if let Some(colon_pos) = auth.find(':') {
                    result.push_str(&auth[..=colon_pos]);
                    result.push_str("***");
                } else {
                    result.push_str(auth);
                }
                result.push('@');
                result.push_str(&rest[at_pos + 1..]);
            } else {
                result.push_str(rest);
            }
            return result;
        }
        i += 1;
    }
    s.to_string()
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
}
