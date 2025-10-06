type Span = u16;

pub enum Cipher {
    Xor(Option<Span>),
    ChaCha20Poly1305,
    Rc6,
}

impl Cipher {
    pub fn encrypt(&self, data: &[u8], key: &[u8], nonce: Option<&[u8]>) {
        match self {
            Cipher::Xor(span) => Self::encode_with_xor(data, key, nonce, span),
            Cipher::ChaCha20Poly1305 => Self::encode_with_chacha20(data, key, nonce),
            Cipher::Rc6 => Self::encode_with_rc6(data, key, nonce),
        }
    }

    pub fn decrypt(&self, data: &[u8], key: &[u8], nonce: Option<&[u8]>) {}

    fn encode_with_xor(data: &[u8], key: &[u8], nonce: Option<&[u8]>, span: &Option<Span>) {}
    fn encode_with_chacha20(data: &[u8], key: &[u8], nonce: Option<&[u8]>) {}
    fn encode_with_rc6(data: &[u8], key: &[u8], nonce: Option<&[u8]>) {}
}
