use chacha20poly1305::{
    AeadCore, ChaCha20Poly1305, KeyInit,
    aead::{OsRng, rand_core::RngCore},
};

type Span = u16;

pub enum Cipher {
    Xor(Option<Span>),
    ChaCha20Poly1305,
    Rc6,
}

impl Cipher {
    pub fn encrypt(&self, data: &[u8], key: &[u8], nonce: Option<&[u8]>) {
        let CryptoSuite { key, nonce } = CryptoSuite::generate_key_nonce();
        let key = &key.as_slice();
        if let Some(n) = nonce {
            let nonce = Some(n.as_slice());
            match self {
                Cipher::Xor(span) => Self::encode_with_xor(data, span, key, nonce),
                Cipher::ChaCha20Poly1305 => Self::encode_with_chacha20(data, key, nonce),
                Cipher::Rc6 => Self::encode_with_rc6(data, key, nonce),
            }
        } else {
            let nonce = None;
            match self {
                Cipher::Xor(span) => Self::encode_with_xor(data, span, key, nonce),
                Cipher::ChaCha20Poly1305 => Self::encode_with_chacha20(data, key, nonce),
                Cipher::Rc6 => Self::encode_with_rc6(data, key, nonce),
            }
        }
    }

    pub fn decrypt(&self, data: &[u8], key: &[u8], nonce: Option<&[u8]>) {}

    fn encode_with_xor(data: &[u8], span: &Option<Span>, key: &[u8], nonce: Option<&[u8]>) {}
    fn encode_with_chacha20(data: &[u8], key: &[u8], nonce: Option<&[u8]>) {}
    fn encode_with_rc6(data: &[u8], key: &[u8], nonce: Option<&[u8]>) {}
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
