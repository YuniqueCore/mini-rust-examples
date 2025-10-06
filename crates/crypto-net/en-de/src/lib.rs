type Span = u16;

pub enum Cipher {
    Xor(Option<Span>),
    ChaCha20,
    Rc6,
}

impl Cipher {
    pub async fn encode(&self, data: &mut [u8]) {
        match self {
            Cipher::Xor(span) => Self::encode_with_xor(data, span).await,
            Cipher::ChaCha20 => Self::encode_with_chacha20(data).await,
            Cipher::Rc6 => Self::encode_with_rc6(data).await,
        }
    }

    pub async fn decode(&self, data: &mut [u8]) {}

    async fn encode_with_xor(data: &mut [u8], span: &Option<Span>) {}
    async fn encode_with_chacha20(data: &mut [u8]) {}
    async fn encode_with_rc6(data: &mut [u8]) {}
}
