# -*- coding: utf-8 -*-
import pathlib
p = pathlib.Path(r"D:\RdataStation\RDS\RdataStation-v2\crates\connection\src\known_hosts.rs")
t = p.read_text(encoding="utf-8")

# 1) parse 处补类型还原
old = """            let hosts = parts[0];
            // known_hosts 两种格式均支持：
            // - 完整格式：`host keytype base64key`（ssh-ed25519 AAAA...）
            // - 简写格式：`host base64key`
            let key_openssh = parts[1..].join(" ");

            let public_key = match PublicKey::from_openssh(&key_openssh) {"""
new = """            let hosts = parts[0];
            // known_hosts 两种格式均支持：
            // - 完整格式：`host keytype base64key`（ssh-ed25519 AAAA...）
            // - 简写格式：`host base64key`（russh public_key_base64 输出，需还原类型前缀）
            let key_openssh = key_openssh_with_type(&parts[1..].join(" "));

            let public_key = match PublicKey::from_openssh(&key_openssh) {"""
assert old in t
t = t.replace(old, new)

# 2) 加辅助函数（normalize_host 之前）
anchor = "fn normalize_host(host: &str) -> String {"
helper = '''/// 还原 OpenSSH 公钥类型前缀
///
/// - 已含类型前缀（`ssh-ed25519 AAAA...` / `ecdsa-...` / `sk-...`）→ 原样返回
/// - 裸 base64（russh `public_key_base64()` 输出）→ 从 OpenSSH wire 格式
///   （`[u32 len][type string][key bytes]`）解码出类型字符串并补回
fn key_openssh_with_type(key: &str) -> String {
    if key.starts_with("ssh-") || key.starts_with("ecdsa-") || key.starts_with("sk-") {
        return key.to_string();
    }
    let b64 = key.split_whitespace().next().unwrap_or(key);
    use base64::Engine;
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
        if bytes.len() >= 4 {
            let len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
            if len > 0 && 4 + len <= bytes.len() {
                if let Ok(ty) = std::str::from_utf8(&bytes[4..4 + len]) {
                    if ty.starts_with("ssh-") || ty.starts_with("ecdsa-") || ty.starts_with("sk-") {
                        return format!("{} {}", ty, b64);
                    }
                }
            }
        }
    }
    key.to_string()
}

'''
assert anchor in t
t = t.replace(anchor, helper + anchor)

# 3) 移除诊断
t = t.replace('        eprintln!("DIAG key_b64={:?}", key_b64);\n', "")
t = t.replace('        eprintln!("DIAG content={:?}", content);\n', "")
t = t.replace('        eprintln!("DIAG entries keys: {:?}", hosts.entries.keys().collect::<Vec<_>>());\n', "")
t = t.replace('        eprintln!("DIAG entry count: {:?}", hosts.entries.values().map(|v| v.len()).collect::<Vec<_>>());\n', "")

p.write_text(t, encoding="utf-8")
print("known_hosts type-recovery fixed")
