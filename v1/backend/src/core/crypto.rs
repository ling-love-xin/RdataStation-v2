use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;

use crate::core::error::{CommonError, CoreError};

/// 旧版固定盐值（用于向后兼容解密）
const LEGACY_FIXED_SALT: &[u8] = b"RdataStation_Connection_Vault_2026";

fn salt_path() -> PathBuf {
    let mut path = dirs::data_local_dir().unwrap_or_else(|| {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        tracing::warn!("无法获取系统数据目录，回退到用户主目录: {}", home.display());
        home
    });
    path.push("RdataStation");
    path.push("encryption-salt");
    path
}

/// 获取或生成安装级随机盐值，存储到文件
fn get_or_create_salt() -> Vec<u8> {
    let sp = salt_path();
    if let Ok(data) = fs::read(&sp) {
        if data.len() >= 32 {
            return data;
        }
    }

    // 生成 32 字节随机盐值
    let mut salt = vec![0u8; 32];
    OsRng.fill_bytes(&mut salt);

    if let Some(parent) = sp.parent() {
        let _ = fs::create_dir_all(parent);
        let _ = fs::write(&sp, &salt);
    }
    salt
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

fn machine_id_path() -> PathBuf {
    let mut path = dirs::data_local_dir().unwrap_or_else(|| {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        tracing::warn!("无法获取系统数据目录，回退到用户主目录: {}", home.display());
        home
    });
    path.push("RdataStation");
    path.push("machine-id");
    path
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
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher.encrypt(nonce, password.as_bytes()).map_err(|e| {
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
    let nonce = Nonce::from_slice(nonce_bytes);

    // 先用新密钥（随机盐值）解密
    let keys = [derive_key(), derive_legacy_key()];
    for key in &keys {
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| {
            CoreError::common(CommonError::Internal(format!("AES init error: {}", e)))
        })?;
        if let Ok(plaintext) = cipher.decrypt(nonce, ciphertext) {
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
