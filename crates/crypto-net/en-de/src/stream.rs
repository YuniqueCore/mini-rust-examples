use anyverr::AnyError;
use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};

use super::Result;

/// 流式加密器，支持大文件的分块加密
pub struct StreamEncryptor {
    cipher: XChaCha20Poly1305,
    nonce: XNonce,
    counter: u64,
}

impl StreamEncryptor {
    /// 创建新的流式加密器
    pub fn new(key: &[u8], nonce: &[u8]) -> Result<Self> {
        if key.len() < 32 {
            return Err(AnyError::quick(
                "The key len should be greater than or equals 32",
                anyverr::ErrKind::RuleViolation,
            ));
        }
        if nonce.len() != 24 {
            return Err(AnyError::quick(
                "Nonce must be 24 bytes for XChaCha20",
                anyverr::ErrKind::RuleViolation,
            ));
        }

        let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
        let nonce = XNonce::from_slice(nonce);

        Ok(Self {
            cipher,
            nonce: *nonce,
            counter: 0,
        })
    }

    /// 加密一个数据块
    pub fn encrypt_chunk(&mut self, chunk: &[u8]) -> Result<Vec<u8>> {
        if chunk.is_empty() {
            return Ok(Vec::new());
        }

        // 为每个块生成唯一的 nonce：基础 nonce + 计数器
        let mut chunk_nonce = self.nonce;
        let counter_bytes = self.counter.to_le_bytes();
        // 将计数器添加到 nonce 的最后 8 字节
        for (i, &byte) in counter_bytes.iter().enumerate() {
            if i < 24 {
                chunk_nonce[i] ^= byte;
            }
        }

        let ct = self
            .cipher
            .encrypt(&chunk_nonce.into(), chunk)
            .map_err(|e| {
                AnyError::quick(
                    format!("failed to encrypt chunk: {}", e),
                    anyverr::ErrKind::ValueValidation,
                )
            })?;

        self.counter += 1;
        Ok(ct)
    }

    /// 完成加密并返回最终的 nonce（用于解密）
    pub fn finalize(self) -> [u8; 24] {
        self.nonce.into()
    }
}

/// 流式解密器，支持大文件的分块解密
pub struct StreamDecryptor {
    cipher: XChaCha20Poly1305,
    nonce: XNonce,
    counter: u64,
}

impl StreamDecryptor {
    /// 创建新的流式解密器
    pub fn new(key: &[u8], nonce: &[u8]) -> Result<Self> {
        if key.len() < 32 {
            return Err(AnyError::quick(
                "The key len should be greater than or equals 32",
                anyverr::ErrKind::RuleViolation,
            ));
        }
        if nonce.len() != 24 {
            return Err(AnyError::quick(
                "Nonce must be 24 bytes for XChaCha20",
                anyverr::ErrKind::RuleViolation,
            ));
        }

        let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
        let nonce = XNonce::from_slice(nonce);

        Ok(Self {
            cipher,
            nonce: *nonce,
            counter: 0,
        })
    }

    /// 解密一个数据块
    pub fn decrypt_chunk(&mut self, chunk: &[u8]) -> Result<Vec<u8>> {
        if chunk.is_empty() {
            return Ok(Vec::new());
        }

        // 为每个块生成唯一的 nonce：基础 nonce + 计数器
        let mut chunk_nonce = self.nonce;
        let counter_bytes = self.counter.to_le_bytes();
        // 将计数器添加到 nonce 的最后 8 字节
        for (i, &byte) in counter_bytes.iter().enumerate() {
            if i < 24 {
                chunk_nonce[i] ^= byte;
            }
        }

        let pt = self
            .cipher
            .decrypt(&chunk_nonce.into(), chunk)
            .map_err(|e| {
                AnyError::quick(
                    format!("failed to decrypt chunk: {}", e),
                    anyverr::ErrKind::ValueValidation,
                )
            })?;

        self.counter += 1;
        Ok(pt)
    }
}
