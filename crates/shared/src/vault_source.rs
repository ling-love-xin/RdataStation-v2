//! 密钥来源三档（原型 ⑦ 密码管理 Tab2）：机器绑定 / 系统凭据 / 主密码。
//!
//! 现状：`crypto::derive_key` 只有一档——安装盐值（`data/encryption-salt`）+ 机器标识
//! （`data/machine-id`）派生的固定密钥。后果是「同一个 `RDS_HOME` 拷到同型号机器上就能解」，
//! 换机又要搬两个文件。本模块给出三档选择，**密文格式不变**（仍是 `AES:`/base64 那套），
//! 只换「用什么密钥加密」：
//!
//! | 档位 | 密钥 | 换机 | 忘记的后果 |
//! |---|---|---|---|
//! | `machine`（默认，向后兼容） | SHA-256(安装盐 ‖ 机器标识) | 搬 `data/` 目录 | 无（可重建） |
//! | `os` | 随机 32 字节，经 Windows DPAPI 加密后存 `data/vault-os-key` | DPAPI 绑定本机本用户 | 无（删档重建，旧密文解不开） |
//! | `master` | 随机 DEK，用 Argon2id(口令) 派生的 KEK 加密后存 `data/vault-master-key` | 拷 `data/` + 口令即可 | **不可恢复**（原型明示） |
//!
//! 边界（不假装）：
//! - 档位切换 = 旧 key 解密全量 → 新 key 重加密，由 store 层编排（`reencrypt_all_*` 已有），
//!   本模块只负责「当前档位 / 取 key / 主密码解锁」；
//! - 解锁交互（启动弹窗、错误次数）由壳层承载，本模块只暴露 `unlock_master` / `lock_master`；
//! - 非 Windows 平台没有 `os` 档（返回明确的 NotSupported 错误，不静默降级）。

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::TryRng;
use rand::rngs::SysRng;
use serde::{Deserialize, Serialize};

use crate::crypto::machine_key;
use crate::error::{CommonError, CoreError};

/// 密钥来源档位（落盘为 `vault-source.json`，值用短名，缺失 = `machine`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum VaultSource {
    /// 机器绑定：安装盐 + 机器标识（v1 起的现状，默认）
    #[default]
    Machine,
    /// 系统凭据管理器：DPAPI 保护的本机密钥
    Os,
    /// 主密码：Argon2id 派生 KEK 加密随机 DEK
    Master,
}

impl VaultSource {
    /// 三档的展示顺序（UI 与文档同序：机器绑定 → 系统凭据 → 主密码）。
    pub const ALL: [VaultSource; 3] = [Self::Machine, Self::Os, Self::Master];

    pub fn label(self) -> &'static str {
        match self {
            Self::Machine => "机器绑定",
            Self::Os => "系统凭据",
            Self::Master => "主密码",
        }
    }

    /// 稳定短名（与落盘值一致；UI 的按钮 id / 测试定位用它，不用展示名）。
    pub fn key(self) -> &'static str {
        match self {
            Self::Machine => "machine",
            Self::Os => "os",
            Self::Master => "master",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_ascii_lowercase().as_str() {
            "machine" => Some(Self::Machine),
            "os" => Some(Self::Os),
            "master" => Some(Self::Master),
            _ => None,
        }
    }

    /// 一句话说明（切档前要让用户看清代价，文案就是这里）。
    pub fn hint(self) -> &'static str {
        match self {
            Self::Machine => "盐值 + 机器标识派生。换机需搬 data 目录；忘记无代价（可重建）。",
            Self::Os => "随机密钥由 Windows DPAPI 保护，绑定本机本账户。删档即重建，旧密文将解不开。",
            Self::Master => "随机密钥由口令（Argon2id）派生的 KEK 封装。换机拷 data 目录 + 口令即可；忘记口令不可恢复。",
        }
    }
}

fn data_file(name: &str) -> PathBuf {
    paths::data_dir().join(name)
}

pub fn source_file() -> PathBuf {
    data_file("vault-source.json")
}

fn os_key_file() -> PathBuf {
    data_file("vault-os-key")
}

fn master_file() -> PathBuf {
    data_file("vault-master-key")
}

#[derive(Serialize, Deserialize)]
struct SourceFile {
    source: VaultSource,
}

/// 当前档位（文件缺失 / 坏 JSON → `machine`，不报错：默认档必须永远可用）。
pub fn current_source() -> VaultSource {
    fs::read_to_string(source_file())
        .ok()
        .and_then(|s| serde_json::from_str::<SourceFile>(&s).ok())
        .map(|f| f.source)
        .unwrap_or_default()
}

pub fn set_source(source: VaultSource) -> Result<(), CoreError> {
    let path = source_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| CoreError::common(CommonError::Internal(format!("创建目录失败: {e}"))))?;
    }
    let json = serde_json::to_string_pretty(&SourceFile { source })
        .map_err(|e| CoreError::common(CommonError::Internal(format!("序列化失败: {e}"))))?;
    fs::write(&path, json)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("写入档位失败: {e}"))))
}

/// 当前档位下用于**加密**的密钥。`master` 档未解锁时返回明确错误（不悄悄用别的 key）。
pub fn active_key() -> Result<[u8; 32], CoreError> {
    match current_source() {
        VaultSource::Machine => Ok(machine_key()),
        VaultSource::Os => os_key(),
        VaultSource::Master => master_key(),
    }
}

/// 解密可用钥集：当前档位 + 机器档 + 主密码已解锁档 + 系统档。
///
/// 解密必须「多钥尝试」——切档时若有密文没被重加密（迁移中断 / 手工拷库），
/// 至少还能用旧档解开，而不是让用户面对一堆读不出来的密码。
pub fn candidates() -> Vec<[u8; 32]> {
    let mut keys = Vec::with_capacity(4);
    keys.push(machine_key());
    if let Ok(k) = os_key() {
        keys.push(k);
    }
    if let Ok(k) = master_key() {
        keys.push(k);
    }
    if let Ok(k) = active_key() {
        keys.push(k);
    }
    keys
}

/// 切档时取目标档密钥：`machine` 直接可用；`os` 首次会生成 DPAPI 密钥；
/// `master` 需要口令——库不存在则**当场建立**（这是切档的语义，不是普通解锁）。
pub fn prepare_source_key(
    source: VaultSource,
    master_password: Option<&str>,
) -> Result<[u8; 32], CoreError> {
    match source {
        VaultSource::Machine => Ok(machine_key()),
        VaultSource::Os => os_key(),
        VaultSource::Master => {
            let password = master_password.ok_or_else(|| {
                CoreError::common(CommonError::invalid_argument(
                    "master_password",
                    "切换到主密码档必须提供主密码",
                ))
            })?;
            if master_initialized() {
                unlock_master(password)?;
            } else {
                init_master(password)?;
            }
            master_key()
        }
    }
}

// ===== 系统凭据档（Windows DPAPI）=====

fn os_key() -> Result<[u8; 32], CoreError> {
    let path = os_key_file();
    if let Ok(protected) = fs::read(&path) {
        let raw = dpapi_unprotect(&protected)?;
        return key_from_slice(&raw);
    }
    // 首次使用：随机生成，DPAPI 保护后落盘
    let key = random32()?;
    let protected = dpapi_protect(&key)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| CoreError::common(CommonError::Internal(format!("创建目录失败: {e}"))))?;
    }
    fs::write(&path, protected)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("写入系统密钥失败: {e}"))))?;
    Ok(key)
}

fn key_from_slice(raw: &[u8]) -> Result<[u8; 32], CoreError> {
    if raw.len() != 32 {
        return Err(CoreError::common(CommonError::Internal(
            "系统密钥长度不是 32 字节，可能已被破坏".to_string(),
        )));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(raw);
    Ok(key)
}

fn random32() -> Result<[u8; 32], CoreError> {
    let mut buf = [0u8; 32];
    SysRng
        .try_fill_bytes(&mut buf)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("随机数失败: {e}"))))?;
    Ok(buf)
}

#[cfg(windows)]
mod dpapi {
    use windows_sys::Win32::Foundation::{LocalFree, HLOCAL};
    use windows_sys::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };

    pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
        unsafe {
            let mut input = CRYPT_INTEGER_BLOB {
                cbData: plain.len() as u32,
                pbData: plain.as_ptr() as *mut u8,
            };
            let mut output = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            };
            let ok = CryptProtectData(
                &mut input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            );
            if ok == 0 || output.pbData.is_null() {
                return None;
            }
            let bytes =
                std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            LocalFree(output.pbData as HLOCAL);
            Some(bytes)
        }
    }

    pub fn unprotect(cipher: &[u8]) -> Option<Vec<u8>> {
        unsafe {
            let mut input = CRYPT_INTEGER_BLOB {
                cbData: cipher.len() as u32,
                pbData: cipher.as_ptr() as *mut u8,
            };
            let mut output = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            };
            let ok = CryptUnprotectData(
                &mut input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            );
            if ok == 0 || output.pbData.is_null() {
                return None;
            }
            let bytes =
                std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            LocalFree(output.pbData as HLOCAL);
            Some(bytes)
        }
    }
}

fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>, CoreError> {
    #[cfg(windows)]
    {
        dpapi::protect(plain).ok_or_else(|| {
            CoreError::common(CommonError::Internal(
                "DPAPI 加密失败（当前账户不可用？）".to_string(),
            ))
        })
    }
    #[cfg(not(windows))]
    {
        let _ = plain;
        Err(CoreError::common(CommonError::NotSupported(
            "系统凭据档（DPAPI）仅 Windows 支持".to_string(),
        )))
    }
}

fn dpapi_unprotect(cipher: &[u8]) -> Result<Vec<u8>, CoreError> {
    #[cfg(windows)]
    {
        dpapi::unprotect(cipher).ok_or_else(|| {
            CoreError::common(CommonError::Internal(
                "DPAPI 解密失败：密钥不是本机本账户创建的".to_string(),
            ))
        })
    }
    #[cfg(not(windows))]
    {
        let _ = cipher;
        Err(CoreError::common(CommonError::NotSupported(
            "系统凭据档（DPAPI）仅 Windows 支持".to_string(),
        )))
    }
}

// ===== 主密码档（Argon2id + DEK）=====

/// Argon2id 参数（19 MiB / 2 轮 / 1 线程，RFC 9106 的低内存档）。
const M_COST: u32 = 19 * 1024;
const T_COST: u32 = 2;
const P_COST: u32 = 1;

#[derive(Serialize, Deserialize)]
struct MasterFile {
    version: u32,
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
    /// base64(随机 16 字节 salt)
    salt: String,
    /// base64(nonce12 ‖ ciphertext)：被 KEK 加密的 DEK
    sealed_dek: String,
}

static MASTER_CACHE: OnceLock<Mutex<Option<[u8; 32]>>> = OnceLock::new();

fn master_cache() -> &'static Mutex<Option<[u8; 32]>> {
    MASTER_CACHE.get_or_init(|| Mutex::new(None))
}

pub fn master_initialized() -> bool {
    master_file().exists()
}

pub fn master_unlocked() -> bool {
    master_cache()
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
}

/// 首次设置主密码：生成随机 DEK，用口令派生的 KEK 封装后落盘，并留在内存缓存里。
pub fn init_master(password: &str) -> Result<(), CoreError> {
    if password.trim().is_empty() {
        return Err(CoreError::common(CommonError::invalid_argument("master_password", "主密码不能为空")));
    }
    let dek = random32()?;
    let salt = random16()?;
    let kek = argon2_kek(password, &salt, M_COST, T_COST, P_COST)?;
    let sealed = seal(&kek, &dek)?;
    let file = MasterFile {
        version: 1,
        m_cost: M_COST,
        t_cost: T_COST,
        p_cost: P_COST,
        salt: b64(&salt),
        sealed_dek: b64(&sealed),
    };
    let path = master_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| CoreError::common(CommonError::Internal(format!("创建目录失败: {e}"))))?;
    }
    let json = serde_json::to_string_pretty(&file)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("序列化失败: {e}"))))?;
    fs::write(&path, json)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("写入主密码库失败: {e}"))))?;
    *master_cache().lock().map_err(|_| {
        CoreError::common(CommonError::Internal("主密码缓存锁失败".to_string()))
    })? = Some(dek);
    Ok(())
}

/// 解锁主密码：读档 → Argon2id 派生 KEK → 解封 DEK → 缓存。
pub fn unlock_master(password: &str) -> Result<(), CoreError> {
    let path = master_file();
    let json = fs::read_to_string(&path).map_err(|e| {
        CoreError::common(CommonError::general(format!("主密码库不存在: {e}")))
    })?;
    let file: MasterFile = serde_json::from_str(&json)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("主密码库损坏: {e}"))))?;
    let salt = unb64(&file.salt)?;
    let sealed = unb64(&file.sealed_dek)?;
    let kek = argon2_kek(password, &salt, file.m_cost, file.t_cost, file.p_cost)?;
    let dek = open(&kek, &sealed).map_err(|_| {
        CoreError::common(CommonError::invalid_argument("master_password", "主密码错误"))
    })?;
    let key = key_from_slice(&dek)?;
    *master_cache().lock().map_err(|_| {
        CoreError::common(CommonError::Internal("主密码缓存锁失败".to_string()))
    })? = Some(key);
    Ok(())
}

pub fn lock_master() {
    if let Ok(mut guard) = master_cache().lock() {
        *guard = None;
    }
}

fn master_key() -> Result<[u8; 32], CoreError> {
    if let Ok(guard) = master_cache().lock() {
        if let Some(key) = *guard {
            return Ok(key);
        }
    }
    Err(CoreError::common(CommonError::Internal(
        "主密码库尚未解锁：请先输入主密码".to_string(),
    )))
}

pub(crate) fn argon2_kek(
    password: &str,
    salt: &[u8],
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
) -> Result<[u8; 32], CoreError> {
    let params = Params::new(m_cost, t_cost, p_cost, Some(32)).map_err(|e| {
        CoreError::common(CommonError::Internal(format!("Argon2 参数非法: {e}")))
    })?;
    let hasher = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = [0u8; 32];
    hasher.hash_password_into(password.as_bytes(), salt, &mut out).map_err(|e| {
        CoreError::common(CommonError::Internal(format!("Argon2 派生失败: {e}")))
    })?;
    Ok(out)
}

pub(crate) fn seal(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, CoreError> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("AES init: {e}"))))?;
    let mut nonce_bytes = [0u8; 12];
    SysRng
        .try_fill_bytes(&mut nonce_bytes)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("Nonce: {e}"))))?;
    let nonce = Nonce::try_from(&nonce_bytes[..])
        .map_err(|e| CoreError::common(CommonError::Internal(format!("Nonce init: {e}"))))?;
    let ct = cipher
        .encrypt(&nonce, plain)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("加密失败: {e}"))))?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub(crate) fn open(key: &[u8; 32], sealed: &[u8]) -> Result<Vec<u8>, CoreError> {
    if sealed.len() < 12 {
        return Err(CoreError::common(CommonError::Internal(
            "密文太短".to_string(),
        )));
    }
    let (nonce_bytes, ct) = sealed.split_at(12);
    let nonce = Nonce::try_from(nonce_bytes)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("Nonce init: {e}"))))?;
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("AES init: {e}"))))?;
    cipher.decrypt(&nonce, ct).map_err(|_| {
        CoreError::common(CommonError::Internal("解密失败（密钥不匹配）".to_string()))
    })
}

fn random16() -> Result<[u8; 16], CoreError> {
    let mut buf = [0u8; 16];
    SysRng
        .try_fill_bytes(&mut buf)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("随机数失败: {e}"))))?;
    Ok(buf)
}

fn b64(bytes: &[u8]) -> String {
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes)
}

fn unb64(s: &str) -> Result<Vec<u8>, CoreError> {
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, s)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("Base64 解码失败: {e}"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_serde_is_lowercase_and_roundtrips() {
        for s in [VaultSource::Machine, VaultSource::Os, VaultSource::Master] {
            let json = serde_json::to_string(&SourceFile { source: s }).unwrap();
            assert!(json.contains("\"source\""));
            let back: SourceFile = serde_json::from_str(&json).unwrap();
            assert_eq!(back.source, s);
        }
        let m: SourceFile = serde_json::from_str(r#"{"source":"machine"}"#).unwrap();
        assert_eq!(m.source, VaultSource::Machine);
    }

    #[test]
    fn argon2_kek_is_deterministic_and_password_sensitive() {
        let salt = [7u8; 16];
        let a = argon2_kek("hunter2", &salt, 8 * 1024, 1, 1).unwrap();
        let b = argon2_kek("hunter2", &salt, 8 * 1024, 1, 1).unwrap();
        assert_eq!(a, b, "同口令同盐必须得到同一 KEK");
        let c = argon2_kek("hunter3", &salt, 8 * 1024, 1, 1).unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn sealed_dek_roundtrip_and_wrong_kek_fails() {
        let kek = [1u8; 32];
        let dek = [42u8; 32];
        let sealed = seal(&kek, &dek).unwrap();
        assert_eq!(open(&kek, &sealed).unwrap(), dek.to_vec());
        assert!(open(&[2u8; 32], &sealed).is_err(), "错误 KEK 必须解不开");
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_protects_and_unprotects_32_bytes() {
        let plain = [9u8; 32];
        let protected = dpapi::protect(&plain).expect("DPAPI 加密应可用（Windows 本机账户）");
        assert!(!protected.is_empty());
        let back = dpapi::unprotect(&protected).expect("DPAPI 解密应可用");
        assert_eq!(back, plain.to_vec());
    }
}
