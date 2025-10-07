use anyverr::{AnyError, AnyResult};
use chacha20poly1305::{AeadCore, ChaCha20Poly1305, KeyInit, aead::OsRng};

type Result<T> = AnyResult<T>;
type Span = u16;

pub enum Cipher {
    Xor(Option<Span>),
    ChaCha20Poly1305,
    Rc6,
}

impl Cipher {
    pub fn encrypt(&self, data: &[u8], key: &[u8], nonce: Option<&[u8]>) -> Result<Vec<u8>> {
        match self {
            Cipher::Xor(span) => Self::encrypt_xor(data, span, key, nonce),
            Cipher::ChaCha20Poly1305 => Self::encrypt_chacha20(data, key, nonce),
            Cipher::Rc6 => Self::encrypt_rc6(data, key, nonce),
        }
    }

    pub fn decrypt(&self, data: &[u8], key: &[u8], nonce: Option<&[u8]>) -> Result<Vec<u8>> {
        Ok(vec![])
    }

    /// 对数据进行 XOR 加密，并可以指定跳过的字节间隔。
    ///
    /// # 参数
    /// * `data`: 待加密的字节切片。
    /// * `span`: 一个 `Option<Span>`，如果为 `Some(s)`，则每隔 `s` 个字节跳过一个字节不进行加密。
    ///           如果为 `None` 或 `Some(0)`，则对所有字节进行加密。
    /// * `key`: XOR 密钥。
    ///
    /// # 返回值
    /// * `Ok(Vec<u8>)`: 加密后的数据。
    /// * `Err(AnyError)`: 如果密钥为空。
    fn encrypt_xor(
        data: &[u8],
        span: &Option<Span>,
        key: &[u8],
        _nonce: Option<&[u8]>,
    ) -> Result<Vec<u8>> {
        if key.is_empty() {
            return Err(AnyError::quick(
                "Key is empty",
                anyverr::ErrKind::ValueValidation,
            ));
        }

        if let Some(span_val) = *span {
            // 复杂情况：每隔 'span_val' 个字节跳过一个
            // 这里的逻辑已经能正确处理 span > data.len() 的情况。
            // 例如，如果 data.len() = 10, span = 100，那么 i % 100 == 0
            // 只在 i == 0 时成立，因此只会跳过第一个字节。
            // 这正是“收缩至对应范围”的数学体现。
            Ok(data
                .iter()
                .enumerate()
                .map(|(i, &d)| {
                    // 检查当前索引是否是 span 的倍数
                    if i % span_val as usize == 0 {
                        d // 是，则保留原始字节
                    } else {
                        // 否，进行 XOR 运算
                        d ^ key[i % key.len()]
                    }
                })
                .collect())
        } else {
            // 对所有数据进行 XOR
            Ok(data
                .iter()
                .enumerate()
                .map(|(i, &d)| d ^ key[i % key.len()])
                .collect())
        }
    }

    fn encrypt_chacha20(data: &[u8], key: &[u8], nonce: Option<&[u8]>) -> Result<Vec<u8>> {
        Ok(vec![])
    }
    fn encrypt_rc6(data: &[u8], key: &[u8], nonce: Option<&[u8]>) -> Result<Vec<u8>> {
        Ok(vec![])
    }
}

struct CryptoSuite {
    key: Vec<u8>,
    nonce: Option<Vec<u8>>,
}

impl CryptoSuite {
    pub fn new() -> Self {
        let os_rng = OsRng::default();

        let key = ChaCha20Poly1305::generate_key(os_rng);
        let key = key.to_vec();

        let nonce = ChaCha20Poly1305::generate_nonce(os_rng);
        let nonce = Some(nonce.to_vec());

        CryptoSuite { key, nonce }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_xor() -> Result<()> {
        let data = String::from("Hello world");
        let CryptoSuite { key, nonce } = CryptoSuite::new();
        let key = &key.as_slice();
        // let mut os_rng = OsRng::default();
        // let random_span = os_rng.next_u32() as usize / data.len();
        let xor_cipher = Cipher::Xor(Some(2));
        println!("origin: {:?}", data.as_bytes());
        println!("origin msg: {}", data);
        let res = if let Some(n) = nonce.clone() {
            let nonce = Some(n.as_slice());
            xor_cipher.encrypt(data.as_bytes(), key, nonce)?
        } else {
            let nonce = None;
            xor_cipher.encrypt(data.as_bytes(), key, nonce)?
        };
        println!("xor: {:?}", res);
        let msg = String::from_utf8_lossy(&res);
        println!("xor msg: {}", msg);

        let xor_cipher = Cipher::Xor(Some(3));

        let res = if let Some(n) = nonce {
            let nonce = Some(n.as_slice());
            xor_cipher.encrypt(&res, key, nonce)?
        } else {
            let nonce = None;
            xor_cipher.encrypt(&res, key, nonce)?
        };
        println!("xor2: {:?}", res);
        let msg = String::from_utf8_lossy(&res);
        println!("xor2 msg: {}", msg);

        Ok(())
    }
}
