//! SSH known_hosts 文件解析与校验
//!
//! 支持格式：
//! - IPv4/IPv6 地址和主机名条目
//! - 非哈希条目：`hostname key_type base64_key [comment]`
//! - 端口特定条目：`[hostname]:port key_type base64_key`
//!
//! 当前不支持：
//! - Hashed hostnames (`|1|salt|hash`)
//! - @cert-authority / @revoked 标记

use russh::keys::PublicKey;
use russh_keys::PublicKeyBase64;
use std::collections::HashMap;
use std::path::PathBuf;

const DEFAULT_KNOWN_HOSTS: &str = ".ssh/known_hosts";

#[derive(Debug, Clone)]
struct KnownHostEntry {
    public_key: PublicKey,
}

#[derive(Debug, Clone, Default)]
pub struct KnownHosts {
    entries: HashMap<String, Vec<KnownHostEntry>>,
    pub allow_unknown: bool,
    file_path: Option<PathBuf>,
    loaded: bool,
}

impl KnownHosts {
    pub fn new(allow_unknown: bool) -> Self {
        Self {
            allow_unknown,
            ..Default::default()
        }
    }

    pub fn allow_all() -> Self {
        Self {
            allow_unknown: true,
            ..Default::default()
        }
    }

    pub fn loaded(&self) -> bool {
        self.loaded
    }

    pub fn file_path(&self) -> Option<&PathBuf> {
        self.file_path.as_ref()
    }

    /// 加载默认路径 `~/.ssh/known_hosts`
    pub fn load_default(allow_unknown: bool) -> Result<Self, std::io::Error> {
        let home = dirs::home_dir().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "无法获取用户 HOME 目录")
        })?;

        let path = home.join(DEFAULT_KNOWN_HOSTS);
        Self::load(&path, allow_unknown)
    }

    /// 从指定路径加载 known_hosts 文件
    pub fn load(path: &PathBuf, allow_unknown: bool) -> Result<Self, std::io::Error> {
        let mut hosts = Self {
            allow_unknown,
            file_path: Some(path.clone()),
            ..Default::default()
        };

        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            hosts.parse(&content);
        }

        hosts.loaded = true;
        Ok(hosts)
    }

    fn parse(&mut self, content: &str) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if line.starts_with('|') || line.starts_with('@') {
                continue;
            }

            let parts: Vec<&str> = line.splitn(3, ' ').collect();
            if parts.len() < 3 {
                continue;
            }

            let hosts = parts[0];
            let key_b64 = parts[2].split_whitespace().next().unwrap_or("");

            let public_key = match PublicKey::from_openssh(key_b64) {
                Ok(key) => key,
                Err(_) => {
                    tracing::warn!(
                        target: "known_hosts",
                        "无法解析 known_hosts 条目中的公钥: host={}",
                        hosts
                    );
                    continue;
                }
            };

            let entry = KnownHostEntry { public_key };

            for host in hosts.split(',') {
                let host = host.trim();
                let normalized = normalize_host(host);
                self.entries
                    .entry(normalized)
                    .or_default()
                    .push(entry.clone());
            }
        }
    }

    /// 校验服务端 Host Key
    ///
    /// 返回：
    /// - `Ok(true)` — 密钥匹配或 allow_unknown 模式
    /// - `Ok(false)` — 密钥不匹配（MITM 攻击风险）
    /// - 如果 allow_unknown=true 且主机不在 known_hosts 中，返回 `Ok(true)`
    /// - 如果 allow_unknown=false 且主机不在 known_hosts 中，返回 `Ok(false)`
    pub fn verify(&self, host: &str, port: u16, server_key: &PublicKey) -> bool {
        let server_fingerprint = server_key.fingerprint(russh::keys::HashAlg::Sha256);
        let key_b64 = server_key.public_key_base64();
        let key_type = key_b64.split_whitespace().next().unwrap_or("unknown");

        let candidates = self.find_candidates(host, port);

        if candidates.is_empty() {
            if self.allow_unknown {
                tracing::info!(
                    target: "known_hosts",
                    host = %host,
                    port = port,
                    fingerprint = %server_fingerprint,
                    key_type = %key_type,
                    policy = "allow_unknown",
                    "Host 不在 known_hosts 中，策略允许通过"
                );
                return true;
            }

            tracing::warn!(
                target: "known_hosts",
                host = %host,
                port = port,
                fingerprint = %server_fingerprint,
                key_type = %key_type,
                policy = "deny",
                "Host 不在 known_hosts 中，策略拒绝"
            );
            return false;
        }

        for candidate in candidates {
            if candidate.public_key == *server_key {
                tracing::info!(
                    target: "known_hosts",
                    host = %host,
                    port = port,
                    fingerprint = %server_fingerprint,
                    key_type = %key_type,
                    "Host Key 校验通过"
                );
                return true;
            }
        }

        tracing::error!(
            target: "known_hosts",
            host = %host,
            port = port,
            fingerprint = %server_fingerprint,
            key_type = %key_type,
            "Host Key 不匹配！可能存在中间人攻击 (MITM)"
        );
        false
    }

    fn find_candidates(&self, host: &str, port: u16) -> Vec<&KnownHostEntry> {
        let mut results = Vec::new();

        let candidates = [host.to_string(), format!("[{}]:{}", host, port)];

        for key in &candidates {
            if let Some(entries) = self.entries.get(key.as_str()) {
                results.extend(entries);
            }
        }

        if let Some(entries) = self.entries.get(host) {
            results.extend(entries);
        }

        let hashed_pattern = format!("[{}]:{}", host, port);
        if let Some(entries) = self.entries.get(&hashed_pattern) {
            results.extend(entries);
        }

        results
    }
}

fn normalize_host(host: &str) -> String {
    host.trim().to_lowercase()
}

/// 创建默认的 known_hosts 校验器
///
/// 尝试加载 ~/.ssh/known_hosts，加载失败则回退到 allow_all 模式
pub fn create_known_hosts_checker(allow_unknown: bool) -> KnownHosts {
    match KnownHosts::load_default(allow_unknown) {
        Ok(hosts) => hosts,
        Err(e) => {
            tracing::warn!(
                target: "known_hosts",
                "无法加载 known_hosts 文件: {}，回退到 allow_all 模式",
                e
            );
            KnownHosts::allow_all()
        }
    }
}

// ========== 测试 ==========
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::{CommonError, CoreError};

    #[test]
    fn test_empty_known_hosts_allow_unknown() -> Result<(), CoreError> {
        let hosts = KnownHosts::new(true);
        let test_key = create_test_key()?;
        assert!(hosts.verify("example.com", 22, &test_key));
        Ok(())
    }

    #[test]
    fn test_empty_known_hosts_deny_unknown() -> Result<(), CoreError> {
        let hosts = KnownHosts::new(false);
        let test_key = create_test_key()?;
        assert!(!hosts.verify("example.com", 22, &test_key));
        Ok(())
    }

    #[test]
    fn test_parse_non_hashed_entry() -> Result<(), CoreError> {
        let test_key = create_test_key()?;
        let key_b64 = test_key.public_key_base64();

        let content = format!("example.com {}\n", key_b64);

        let mut hosts = KnownHosts::new(false);
        hosts.parse(&content);

        assert!(hosts.verify("example.com", 22, &test_key));
        assert!(!hosts.verify("other.com", 22, &test_key));
        Ok(())
    }

    #[test]
    fn test_parse_port_specific_entry() -> Result<(), CoreError> {
        let test_key = create_test_key()?;
        let key_b64 = test_key.public_key_base64();

        let content = format!("[example.com]:2222 {}\n", key_b64);

        let mut hosts = KnownHosts::new(false);
        hosts.parse(&content);

        assert!(hosts.verify("example.com", 2222, &test_key));
        assert!(!hosts.verify("example.com", 22, &test_key));
        Ok(())
    }

    #[test]
    fn test_skip_hashed_entries() {
        let content = "|1|salt|hash ssh-rsa AAA...\n";
        let mut hosts = KnownHosts::new(false);
        hosts.parse(content);

        assert!(hosts.entries.is_empty());
    }

    #[test]
    fn test_skip_at_marked_entries() {
        let content = "@cert-authority *.example.com ssh-rsa AAA...\n";
        let mut hosts = KnownHosts::new(false);
        hosts.parse(content);

        assert!(hosts.entries.is_empty());
    }

    #[test]
    fn test_key_mismatch_detected() -> Result<(), CoreError> {
        let test_key = create_test_key()?;
        let key_b64 = test_key.public_key_base64();

        let different_key = russh::keys::PrivateKey::random(
            &mut rand::thread_rng(),
            russh::keys::Algorithm::Ed25519,
        )
        .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?
        .public_key()
        .clone();

        let content = format!("example.com {}\n", key_b64);

        let mut hosts = KnownHosts::new(false);
        hosts.parse(&content);

        assert!(!hosts.verify("example.com", 22, &different_key));
        Ok(())
    }

    fn create_test_key() -> Result<PublicKey, CoreError> {
        let private = russh::keys::PrivateKey::random(
            &mut rand::thread_rng(),
            russh::keys::Algorithm::Ed25519,
        )
        .map_err(|e| CoreError::common(CommonError::General(e.to_string())))?;
        Ok(private.public_key().clone())
    }
}
