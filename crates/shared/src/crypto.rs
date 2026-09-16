use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::rngs::SysRng;
use rand::TryRng;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::error::{CommonError, CoreError};

/// 旧版固定盐值（用于向后兼容解密）
const LEGACY_FIXED_SALT: &[u8] = b"RdataStation_Connection_Vault_2026";

/// 安装级随机盐值的落点：`<RDS_HOME>/data/encryption-salt`。
///
/// ⚠ 迁移敏感：这个文件是密钥派生的输入之一，位置变了又不跟着搬，
/// 存量密文（连接密码）会全部解不开——见 `paths::migrate`。
fn salt_path() -> PathBuf {
    paths::data_dir().join("encryption-salt")
}

/// 进程内盐缓存：避免并行测试/并发调用下多个实例各自生成盐值
/// 互相覆盖加密文件，导致一方加密后另一方解不开
static SALT_CACHE: OnceLock<Vec<u8>> = OnceLock::new();

/// 获取或生成安装级随机盐值，存储到文件
fn get_or_create_salt() -> Vec<u8> {
    SALT_CACHE
        .get_or_init(|| {
            let sp = salt_path();
            if let Ok(data) = fs::read(&sp) {
                if data.len() >= 32 {
                    return data;
                }
            }

            // 生成 32 字节随机盐值
            // rand 0.10：OsRng 已移除，SysRng 直连 getrandom；失败处理与原实现一致（终止）
            let mut salt = vec![0u8; 32];
            SysRng
                .try_fill_bytes(&mut salt)
                .expect("无法从系统获取随机数（生成加密盐值失败）");

            if let Some(parent) = sp.parent() {
                let _ = fs::create_dir_all(parent);
                let _ = fs::write(&sp, &salt);
            }
            salt
        })
        .clone()
}

/// 主密钥派生：使用随机安装盐值 + 机器ID
fn derive_key() -> [u8; 32] {
    let salt = get_or_create_salt();
    let machine_id = get_machine_id();

    let mut hasher = Sha256::new();
    hasher.update(&salt);
    hasher.update(machine_id.as_bytes());
    let result = hasher.finalize();

    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// 旧版密钥派生（固定盐值 + 机器ID），用于向后兼容解密
fn derive_legacy_key() -> [u8; 32] {
    let machine_id = get_machine_id();

    let mut hasher = Sha256::new();
    hasher.update(LEGACY_FIXED_SALT);
    hasher.update(machine_id.as_bytes());
    let result = hasher.finalize();

    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// 机器标识落点：`<RDS_HOME>/data/machine-id`（与盐值同迁移敏感，见 [`salt_path`]）。
fn machine_id_path() -> PathBuf {
    paths::data_dir().join("machine-id")
}

fn get_machine_id() -> String {
    let id_path = machine_id_path();
    if let Ok(id) = fs::read_to_string(&id_path) {
        let trimmed = id.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    let fallback = build_fallback_id();
    if let Some(parent) = id_path.parent() {
        let _ = fs::create_dir_all(parent);
        let _ = fs::write(&id_path, &fallback);
    }
    fallback
}

fn build_fallback_id() -> String {
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown-host".to_string());

    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown-user".to_string());

    let home = dirs::home_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown-home".to_string());

    format!("{}:{}:{}", hostname, user, home)
}

pub fn encrypt_password(password: &str) -> Result<String, CoreError> {
    let key = derive_key();
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("AES init error: {}", e))))?;

    let mut nonce_bytes = [0u8; 12];
    // rand 0.10：OsRng 已移除，改用 SysRng（getrandom 直连）
    SysRng
        .try_fill_bytes(&mut nonce_bytes)
        .map_err(|e| CoreError::common(CommonError::Internal(format!("Nonce 生成失败: {}", e))))?;
    // aes-gcm 0.11 起 Array::from_slice 已废弃，改用 TryFrom（长度固定 12 字节）
    let nonce = Nonce::try_from(&nonce_bytes[..]).map_err(|e| {
        CoreError::common(CommonError::Internal(format!("Nonce init error: {}", e)))
    })?;

    let ciphertext = cipher.encrypt(&nonce, password.as_bytes()).map_err(|e| {
        CoreError::common(CommonError::Internal(format!("Encryption error: {}", e)))
    })?;

    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &combined,
    ))
}

pub fn decrypt_password(encrypted: &str) -> Result<String, CoreError> {
    let combined = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, encrypted)
        .map_err(|e| {
            CoreError::common(CommonError::Internal(format!("Base64 decode error: {}", e)))
        })?;

    if combined.len() < 12 {
        return Err(CoreError::common(CommonError::Internal(
            "Invalid encrypted data: too short".to_string(),
        )));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let nonce = Nonce::try_from(nonce_bytes).map_err(|e| {
        CoreError::common(CommonError::Internal(format!("Nonce init error: {}", e)))
    })?;

    // 先用新密钥（随机盐值）解密
    let keys = [derive_key(), derive_legacy_key()];
    for key in &keys {
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| {
            CoreError::common(CommonError::Internal(format!("AES init error: {}", e)))
        })?;
        if let Ok(plaintext) = cipher.decrypt(&nonce, ciphertext) {
            return String::from_utf8(plaintext).map_err(|e| {
                CoreError::common(CommonError::Internal(format!("UTF-8 decode error: {}", e)))
            });
        }
    }

    Err(CoreError::common(CommonError::Internal(
        "Decryption failed with both new and legacy keys".to_string(),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() -> Result<(), CoreError> {
        let original = "MySecretPassword123!";
        let encrypted = encrypt_password(original)?;
        let decrypted = decrypt_password(&encrypted)?;
        assert_eq!(original, decrypted);
        Ok(())
    }

    #[test]
    fn test_encrypt_empty_password() {
        let original = "";
        let encrypted = encrypt_password(original).expect("encrypt failed");
        let decrypted = decrypt_password(&encrypted).expect("decrypt failed");
        assert_eq!(original, decrypted);
    }

    #[test]
    fn test_encrypt_unicode_password() -> Result<(), CoreError> {
        let original = "密码测试🔐";
        let encrypted = encrypt_password(original)?;
        let decrypted = decrypt_password(&encrypted)?;
        assert_eq!(original, decrypted);
        Ok(())
    }
}
