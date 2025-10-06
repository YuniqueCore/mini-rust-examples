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

        let mut res = Vec::with_capacity(data.len());
        if let Some(s) = *span {
            for (i, d) in data.iter().enumerate() {
                if i % s as usize == 0 {
                    // skip this bit which is n*span
                    res.push(*d);
                    continue;
                }

                let k = key[i % key.len()];
                let xor = d ^ k;
                res.push(xor);
            }
        }

        Ok(res)
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
        Self::generate_key_nonce()
    }

    fn generate_key_nonce() -> CryptoSuite {
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
