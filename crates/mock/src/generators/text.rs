// ==================== 文本辅助函数 ====================

use super::*;

pub(crate) fn generate_from_regex(pattern: &str, rng: &mut StdRng) -> String {
    let mut result = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let len = chars.len();
    while i < len {
        match chars[i] {
            '\\' if i + 1 < len => {
                i += 1;
                match chars[i] {
                    'd' => result.push((b'0' + (0..10).fake_with_rng::<u8, _>(rng)) as char),
                    'w' => {
                        let pool =
                            b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
                        let idx = (0..pool.len()).fake_with_rng::<usize, _>(rng);
                        result.push(pool[idx] as char);
                    }
                    's' => result.push(' '),
                    'n' => result.push('\n'),
                    't' => result.push('\t'),
                    c => result.push(c),
                }
            }
            '[' => {
                let mut class_chars = Vec::new();
                i += 1;
                let mut negated = false;
                if i < len && chars[i] == '^' {
                    negated = true;
                    i += 1;
                }
                while i < len && chars[i] != ']' {
                    if i + 2 < len && chars[i + 1] == '-' {
                        let start = chars[i] as u32;
                        let end = chars[i + 2] as u32;
                        if end >= start {
                            for c in (start..=end).take(256) {
                                class_chars.push(char::from_u32(c).unwrap_or(' '));
                            }
                        }
                        i += 3;
                    } else {
                        class_chars.push(chars[i]);
                        i += 1;
                    }
                }
                if i < len {
                    i += 1;
                }
                if negated {
                    let full: Vec<char> = (32..127).filter_map(char::from_u32).collect();
                    class_chars = full
                        .into_iter()
                        .filter(|c| !class_chars.contains(c))
                        .collect();
                }
                if !class_chars.is_empty() {
                    let idx = (0..class_chars.len()).fake_with_rng::<usize, _>(rng);
                    result.push(class_chars[idx]);
                }
            }
            '{' => {
                i += 1;
                let mut num_str = String::new();
                while i < len && chars[i] != '}' && chars[i] != ',' {
                    num_str.push(chars[i]);
                    i += 1;
                }
                let min: usize = num_str.parse().unwrap_or(1);
                let mut max = min;
                if i < len && chars[i] == ',' {
                    i += 1;
                    num_str.clear();
                    while i < len && chars[i] != '}' {
                        num_str.push(chars[i]);
                        i += 1;
                    }
                    max = num_str.parse().unwrap_or(min);
                }
                if i < len {
                    i += 1;
                }
                let count = if max > min {
                    (min..=max).fake_with_rng::<usize, _>(rng)
                } else {
                    min
                };
                if let Some(last) = result.pop() {
                    for _ in 0..count {
                        result.push(last);
                    }
                }
            }
            '+' | '*' | '?' | '.' => {}
            '(' | ')' | '^' | '$' => {}
            c => result.push(c),
        }
        i += 1;
    }
    if result.is_empty() {
        Faker.fake_with_rng::<String, _>(rng)
    } else {
        result
    }
}

type TemplateGenFn = fn(&mut StdRng) -> String;

/// 模板字符串替换：{{name}} → 生成值
pub(crate) fn generate_from_template(template: &str, rng: &mut StdRng) -> String {
    let mut result = template.to_string();

    let replacements: &[(&str, TemplateGenFn)] = &[
        ("name", |r| {
            use fake::faker::name::zh_cn::Name;
            Name().fake_with_rng::<String, _>(r)
        }),
        ("first_name", |r| {
            use fake::faker::name::zh_cn::FirstName;
            FirstName().fake_with_rng::<String, _>(r)
        }),
        ("last_name", |r| {
            use fake::faker::name::zh_cn::LastName;
            LastName().fake_with_rng::<String, _>(r)
        }),
        ("email", |r| {
            use fake::faker::internet::en::SafeEmail;
            SafeEmail().fake_with_rng::<String, _>(r)
        }),
        ("uuid", |r| fake::uuid::UUIDv4.fake_with_rng::<String, _>(r)),
        ("word", |r| {
            use fake::faker::lorem::en::Word;
            Word().fake_with_rng::<String, _>(r)
        }),
        ("sentence", |r| {
            use fake::faker::lorem::en::Sentence;
            Sentence(3..8).fake_with_rng::<String, _>(r)
        }),
        ("phone", |r| {
            use fake::faker::phone_number::zh_cn::PhoneNumber;
            PhoneNumber().fake_with_rng::<String, _>(r)
        }),
        ("date", |r| {
            use fake::faker::chrono::en::Date;
            Date()
                .fake_with_rng::<chrono::NaiveDate, _>(r)
                .format("%Y-%m-%d")
                .to_string()
        }),
        ("datetime", |r| {
            fake::faker::chrono::en::DateTime()
                .fake_with_rng::<chrono::DateTime<chrono::Utc>, _>(r)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        }),
    ];

    for (key, gen_fn) in replacements {
        let placeholder = format!("{{{{{}}}}}", key);
        if result.contains(&placeholder) {
            let value = gen_fn(rng);
            result = result.replace(&placeholder, &value);
        }
    }

    // 处理 {{int:MIN-MAX}} 格式（手动解析，避免额外依赖）
    let mut int_result = String::new();
    let template_bytes = result.as_bytes();
    let mut pos = 0;
    let prefix = b"{{int:";
    while pos < template_bytes.len() {
        if pos + prefix.len() <= template_bytes.len()
            && &template_bytes[pos..pos + prefix.len()] == prefix
        {
            let start_idx = pos + prefix.len();
            let mut end_pos = start_idx;
            while end_pos < template_bytes.len()
                && template_bytes[end_pos] != b'-'
                && template_bytes[end_pos] != b'}'
            {
                end_pos += 1;
            }
            let min_str = std::str::from_utf8(&template_bytes[start_idx..end_pos]).unwrap_or("0");
            if end_pos < template_bytes.len() && template_bytes[end_pos] == b'-' {
                end_pos += 1;
                let val_start = end_pos;
                while end_pos < template_bytes.len() && template_bytes[end_pos] != b'}' {
                    end_pos += 1;
                }
                let max_str =
                    std::str::from_utf8(&template_bytes[val_start..end_pos]).unwrap_or("100");
                if end_pos < template_bytes.len() && template_bytes[end_pos] == b'}' {
                    end_pos += 1;
                }
                let min: i64 = min_str.parse().unwrap_or(0);
                let max: i64 = max_str.parse().unwrap_or(100);
                let val: i64 = if max >= min {
                    (min..=max).fake_with_rng::<i64, _>(rng)
                } else {
                    min
                };
                int_result.push_str(&val.to_string());
            } else {
                int_result.push_str(&result[pos..end_pos]);
            }
            pos = end_pos;
        } else {
            int_result.push(result.as_bytes()[pos] as char);
            pos += 1;
        }
    }
    result = int_result;

    result
}

#[cfg(test)]
mod tests;

