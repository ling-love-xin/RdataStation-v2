use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// 计算字符串的哈希值
pub fn hash_string(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// 计算任意可哈希类型的哈希值
pub fn hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
