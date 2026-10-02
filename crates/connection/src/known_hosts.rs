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
use russh::keys::PublicKeyBase64;
use std::collections::HashMap;
use std::path::PathBuf;

const DEFAULT_KNOWN_HOSTS: &str = ".ssh/known_hosts";

/// Host Key 校验结果（三态，供上层按策略决策——#37 TOFU）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyOutcome {
    /// 密钥与 known_hosts 中记录匹配（可信主机）
    Matched,
    /// 主机不在 known_hosts 中（首次连接，需 TOFU 确认 / 按策略放行或拒绝）
    Unknown,
    /// 主机存在但密钥不匹配（疑似 MITM，必须拒绝）
    Mismatched,
}

/// Host Key 校验策略（#37 决策：TOFU 默认，Strict 高安全，AllowUnknown 内网退路）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum HostKeyCheck {
    /// TOFU（默认）：首连自动信任并写入 known_hosts，后续同指纹匹配、异指纹拒绝
    Tofu,
    /// 严格校验：未知主机直接拒绝（公网 / 高安全场景）
    Strict,
    /// 放行未知主机且不落盘（内网退路，历史行为）
    AllowUnknown,
}

impl HostKeyCheck {
    /// 未知主机是否放行（决策入口；`Mismatched` 不经过此函数，恒拒绝）。
    pub fn allows_unknown(self) -> bool {
        matches!(self, Self::Tofu | Self::AllowUnknown)
    }

    /// TOFU 未知主机是否应写入 known_hosts。
    pub fn should_append_unknown(self) -> bool {
        matches!(self, Self::Tofu)
    }
}

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

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            let hosts = parts[0];
            // known_hosts 两种格式均支持：
            // - 完整格式：`host keytype base64key`（ssh-ed25519 AAAA...）
            // - 简写格式：`host base64key`（russh public_key_base64 输出，需还原类型前缀）
            let key_openssh = key_openssh_with_type(&parts[1..].join(" "));

            let public_key = match PublicKey::from_openssh(&key_openssh) {
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

    /// 校验服务端 Host Key（三态结果，供上层按 [`HostKeyCheck`] 策略决策）
    ///
    /// 返回：
    /// - [`HostKeyOutcome::Matched`] — 密钥与 known_hosts 记录匹配
    /// - [`HostKeyOutcome::Unknown`] — 主机不在 known_hosts 中（首连 / TOFU 场景）
    /// - [`HostKeyOutcome::Mismatched`] — 主机存在但密钥不匹配（MITM 风险，恒拒绝）
    pub fn verify_outcome(&self, host: &str, port: u16, server_key: &PublicKey) -> HostKeyOutcome {
        let candidates = self.find_candidates(host, port);

        if candidates.is_empty() {
            return HostKeyOutcome::Unknown;
        }

        for candidate in candidates {
            // 只比较密钥本体：`PublicKey` 的 PartialEq 会连带比较 comment 字段，
            // 而 known_hosts 行可能带行尾注释，导致密钥一致却判定不匹配（误报 MITM）
            if candidate.public_key.public_key_bytes() == server_key.public_key_bytes() {
                return HostKeyOutcome::Matched;
            }
        }

        HostKeyOutcome::Mismatched
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

        match self.verify_outcome(host, port, server_key) {
            HostKeyOutcome::Matched => {
                tracing::info!(
                    target: "known_hosts",
                    host = %host,
                    port = port,
                    fingerprint = %server_fingerprint,
                    key_type = %key_type,
                    "Host Key 校验通过"
                );
                true
            }
            HostKeyOutcome::Unknown => {
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
                    true
                } else {
                    tracing::warn!(
                        target: "known_hosts",
                        host = %host,
                        port = port,
                        fingerprint = %server_fingerprint,
                        key_type = %key_type,
                        policy = "deny",
                        "Host 不在 known_hosts 中，策略拒绝"
                    );
                    false
                }
            }
            HostKeyOutcome::Mismatched => {
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
        }
    }

    /// 追加写入一条 Host Key 记录（TOFU 首连信任落盘，决策 #37）。
    ///
    /// - 若本实例从文件加载（`file_path` 有值）→ 追加到该文件（同时更新内存表）；
    /// - 否则仅更新内存表并告警（测试 / 未加载场景不写盘）；
    /// - 端口 22 写 `host keytype base64`，非 22 写 `[host]:port keytype base64`。
    ///
    /// 幂等：同主机 + 同密钥已存在时跳过（不产生重复行）；同主机不同密钥（密钥轮换 /
    /// MITM）**不覆盖**——旧行保留，新行追加，`verify` 对旧密钥仍 Matched、新密钥 Mismatched，
    /// 由上层策略决定是否接受轮换。
    pub fn append(&mut self, host: &str, port: u16, server_key: &PublicKey) -> std::io::Result<()> {
        let normalized = normalize_host(host);
        // `public_key_base64()` 返回裸 base64（无类型前缀）——用 key_openssh_with_type
        // 还原 OpenSSH 前缀后再拆出 `类型 + base64`，落盘行与 known_hosts 标准格式一致。
        let key_openssh = key_openssh_with_type(&server_key.public_key_base64());
        let mut parts = key_openssh.split_whitespace();
        let key_type = parts.next().unwrap_or("ssh-ed25519").to_string();
        let key_b64 = parts.next().unwrap_or(key_openssh.as_str()).to_string();

        // 幂等：同 host + 同 key 已有 → 跳过
        let already = self.find_candidates(&normalized, port).iter().any(|e| {
            e.public_key.public_key_bytes() == server_key.public_key_bytes()
        });
        if already {
            return Ok(());
        }

        let line = if port == 22 {
            format!("{} {} {}", normalized, key_type, key_b64)
        } else {
            format!("[{}]:{} {} {}", normalized, port, key_type, key_b64)
        };

        // 内存表同步（后续同会话校验直接命中）
        if let Ok(public_key) = PublicKey::from_openssh(&key_openssh) {
            self.entries
                .entry(normalized.clone())
                .or_default()
                .push(KnownHostEntry { public_key });
        }

        let Some(path) = self.file_path.clone() else {
            tracing::warn!(
                target: "known_hosts",
                host = %host,
                port = port,
                "known_hosts 未加载文件路径，仅更新内存表（不落盘）"
            );
            return Ok(());
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut content = std::fs::read_to_string(&path).unwrap_or_default();
        if !content.ends_with('\n') && !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&line);
        content.push('\n');
        std::fs::write(&path, content)?;

        tracing::info!(
            target: "known_hosts",
            host = %host,
            port = port,
            path = %path.display(),
            "Host Key 已写入 known_hosts（TOFU）"
        );
        Ok(())
    }

    fn find_candidates(&self, host: &str, port: u16) -> Vec<&KnownHostEntry> {
        let mut results = Vec::new();

        // 查询侧与写入侧（normalize_host）保持一致：主机名归一化为小写。
        // 否则 known_hosts 行 "GitHub.com" 落库为 "github.com" 后，同名主机
        // 查询会因大小写不一致而命中失败（回归断言见 test_comma_separated_hosts）。
        let normalized = normalize_host(host);

        for key in [normalized.clone(), format!("[{}]:{}", normalized, port)] {
            if let Some(entries) = self.entries.get(key.as_str()) {
                results.extend(entries);
            }
        }

        results
    }
}

/// 还原 OpenSSH 公钥类型前缀
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
    use shared::error::{CommonError, CoreError};
    use std::path::PathBuf;

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
        let different_key = second_test_key()?;

        let content = format!("example.com {}\n", key_b64);

        let mut hosts = KnownHosts::new(false);
        hosts.parse(&content);

        assert!(!hosts.verify("example.com", 22, &different_key));
        Ok(())
    }

    #[test]
    fn test_entry_with_trailing_comment_still_matches() -> Result<(), CoreError> {
        // known_hosts 行尾带注释时（部分工具生成或手工粘贴），密钥一致仍应匹配成功
        let test_key = create_test_key()?;
        let content = format!(
            "example.com ssh-ed25519 {} user@example.com\n",
            test_key.public_key_base64()
        );

        let mut hosts = KnownHosts::new(false);
        hosts.parse(&content);

        assert!(hosts.verify("example.com", 22, &test_key));
        Ok(())
    }

    /// 测试用固定公钥 A（Ed25519，仅用于单测；只含公钥，无需 RNG 依赖）
    /// 注意：不带注释——`PublicKey` 的 PartialEq 会比对 comment 字段，
    /// 而 `public_key_base64()` 输出不含注释，带注释会导致往返比较不相等
    const TEST_KEY_A: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBkzxbG3si6k3jEXkAdanRHjo/3yA/x4NCJdxXDDfjxd";
    /// 测试用固定公钥 B（与 A 不同，用于“密钥不匹配”场景）
    const TEST_KEY_B: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIO7v3HWgTuQy1N/bcegB1zYs0KE0r9Y8F/ZNteVKKh5c";

    fn parse_test_key(openssh: &str) -> Result<PublicKey, CoreError> {
        PublicKey::from_openssh(openssh)
            .map_err(|e| CoreError::common(CommonError::General(e.to_string())))
    }

    fn create_test_key() -> Result<PublicKey, CoreError> {
        parse_test_key(TEST_KEY_A)
    }

    fn second_test_key() -> Result<PublicKey, CoreError> {
        parse_test_key(TEST_KEY_B)
    }

    #[test]
    fn test_prefixed_key_entry_uses_openssh_format() -> Result<(), CoreError> {
        // 完整 OpenSSH 格式条目（含 "ssh-ed25519 " 类型前缀）走「原样使用」分支；
        // 裸 base64（无前缀）分支由 test_parse_non_hashed_entry 覆盖（public_key_base64 即裸值）。
        let test_key = create_test_key()?;
        let content = format!("example.com {}\n", TEST_KEY_A);
        let mut hosts = KnownHosts::new(false);
        hosts.parse(&content);

        assert!(
            hosts.verify("example.com", 22, &test_key),
            "带类型前缀的条目应匹配"
        );
        assert!(!hosts.verify("other.com", 22, &test_key));
        Ok(())
    }

    #[test]
    fn test_comma_separated_hosts_and_case_insensitive_lookup() -> Result<(), CoreError> {
        let test_key = create_test_key()?;
        let key_b64 = test_key.public_key_base64();

        let content = format!("host-a,host-b,host-c ssh-ed25519 {}\n", key_b64);
        let mut hosts = KnownHosts::new(false);
        hosts.parse(&content);

        assert!(hosts.verify("host-a", 22, &test_key));
        assert!(hosts.verify("host-c", 22, &test_key));
        // 修复回归：查询侧与写入侧一样做小写归一化（此前混合大小写主机名查不到）
        assert!(hosts.verify("HOST-A", 22, &test_key), "主机名查询应大小写不敏感");
        assert!(hosts.verify("HOST-B", 22, &test_key), "主机名查询应大小写不敏感");
        assert!(!hosts.verify("host-d", 22, &test_key));
        Ok(())
    }

    #[test]
    fn test_mixed_case_entry_matches_lower_and_upper_queries() -> Result<(), CoreError> {
        let test_key = create_test_key()?;
        let content = format!(
            "Db.Example.Com ssh-ed25519 {}\n",
            test_key.public_key_base64()
        );
        let mut hosts = KnownHosts::new(false);
        hosts.parse(&content);

        assert!(hosts.verify("db.example.com", 22, &test_key), "小写查询应命中");
        assert!(hosts.verify("DB.EXAMPLE.COM", 22, &test_key), "大写查询应命中（修复回归）");
        Ok(())
    }

    #[test]
    fn test_load_missing_file_is_empty_table() {
        let path = PathBuf::from("C:/definitely/not/exist/known_hosts");
        let hosts = KnownHosts::load(&path, false).expect("缺失文件应返回空表而非错误");
        assert!(hosts.loaded());
        let key = create_test_key().expect("key");
        assert!(!hosts.verify("example.com", 22, &key), "空表 + deny → false");
    }

    // ===== #37 TOFU：三态校验 / 落盘 / 策略矩阵 =====

    #[test]
    fn test_verify_outcome_three_states() -> Result<(), CoreError> {
        let key = create_test_key()?;
        let other = second_test_key()?;
        let content = format!("example.com {}\n", TEST_KEY_A);
        let mut hosts = KnownHosts::new(false);
        hosts.parse(&content);

        assert_eq!(hosts.verify_outcome("example.com", 22, &key), HostKeyOutcome::Matched);
        assert_eq!(
            hosts.verify_outcome("example.com", 22, &other),
            HostKeyOutcome::Mismatched
        );
        assert_eq!(
            hosts.verify_outcome("unknown.com", 22, &key),
            HostKeyOutcome::Unknown
        );
        Ok(())
    }

    #[test]
    fn test_host_key_check_policy_matrix() {
        // Tofu：未知放行 + 落盘
        assert!(HostKeyCheck::Tofu.allows_unknown());
        assert!(HostKeyCheck::Tofu.should_append_unknown());
        // Strict：未知拒绝、不落盘
        assert!(!HostKeyCheck::Strict.allows_unknown());
        assert!(!HostKeyCheck::Strict.should_append_unknown());
        // AllowUnknown：放行但不落盘（内网退路）
        assert!(HostKeyCheck::AllowUnknown.allows_unknown());
        assert!(!HostKeyCheck::AllowUnknown.should_append_unknown());
    }

    #[test]
    fn test_append_writes_file_and_reload_matches() -> Result<(), CoreError> {
        let key = create_test_key()?;
        let dir = std::env::temp_dir().join(format!("rds-kh-append-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("known_hosts");

        // 首连：空表 → Unknown → append 落盘
        let mut hosts = KnownHosts::load(&path, true).expect("load");
        assert_eq!(hosts.verify_outcome("example.com", 22, &key), HostKeyOutcome::Unknown);
        hosts.append("example.com", 22, &key).expect("append");
        assert_eq!(hosts.verify_outcome("example.com", 22, &key), HostKeyOutcome::Matched);

        // 重新加载同一文件（新实例）→ 仍 Matched（落盘生效）
        let reloaded = KnownHosts::load(&path, true).expect("reload");
        assert_eq!(
            reloaded.verify_outcome("example.com", 22, &key),
            HostKeyOutcome::Matched,
            "TOFU 落盘后重载应匹配"
        );

        // 异密钥 → Mismatched（轮换 / MITM 可被识别）
        let other = second_test_key()?;
        assert_eq!(
            reloaded.verify_outcome("example.com", 22, &other),
            HostKeyOutcome::Mismatched
        );

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[test]
    fn test_append_non_default_port_uses_bracketed_host() -> Result<(), CoreError> {
        let key = create_test_key()?;
        let dir = std::env::temp_dir().join(format!("rds-kh-port-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("known_hosts");

        let mut hosts = KnownHosts::load(&path, true).expect("load");
        hosts.append("db.example.com", 2222, &key).expect("append");

        let content = std::fs::read_to_string(&path).expect("read");
        assert!(
            content.contains("[db.example.com]:2222 ssh-ed25519"),
            "非 22 端口应写 [host]:port 形式，实际: {content}"
        );
        assert_eq!(
            hosts.verify_outcome("db.example.com", 2222, &key),
            HostKeyOutcome::Matched
        );

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[test]
    fn test_append_is_idempotent() -> Result<(), CoreError> {
        let key = create_test_key()?;
        let dir = std::env::temp_dir().join(format!("rds-kh-idem-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("known_hosts");

        let mut hosts = KnownHosts::load(&path, true).expect("load");
        hosts.append("example.com", 22, &key).expect("append");
        hosts.append("example.com", 22, &key).expect("append (2nd)");

        let content = std::fs::read_to_string(&path).expect("read");
        assert_eq!(
            content.matches("ssh-ed25519").count(),
            1,
            "同主机同密钥不应重复落盘，实际: {content}"
        );

        std::fs::remove_dir_all(&dir).ok();
        Ok(())
    }

    #[test]
    fn test_append_without_file_path_updates_memory_only() -> Result<(), CoreError> {
        let key = create_test_key()?;
        let mut hosts = KnownHosts::new(true);
        hosts.append("example.com", 22, &key).expect("append");
        assert_eq!(
            hosts.verify_outcome("example.com", 22, &key),
            HostKeyOutcome::Matched,
            "未加载文件路径时仍应更新内存表"
        );
        Ok(())
    }
}
