### Day 1

```rust
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
```

搭好 enum 和调用结构了，方向很清晰！

在我动手写代码或一步步带你实现之前，先给你几个**快速反馈与建议**（很重要，改动不多但会让实现更对路）：

1. **API 设计问题**

   * 目前 `encode(&self, data: &mut [u8])` 修改原地字节流，但 **现代 AEAD（比如 ChaCha20-Poly1305）会增加认证 tag（+16 bytes）**，不能总是就地覆盖原数据。建议改成 `fn encode(&self, plaintext: &[u8]) -> anyhow::Result<Vec<u8>>`（返回 ciphertext），`decode` 返回 `Result<Vec<u8>>`。
   * XOR 可以就地做（长度不变），但要和其他算法统一返回 `Vec<u8>` 或封装枚举结果更好。

2. **`async` 的使用**

   * 纯粹的加密/解密通常是 CPU-bound、同步操作——把它设为 `async` 并没有好处。两种选择更合理：

     * 保持同步函数（推荐）；如果必须在 tokio 上异步执行，可在调用处用 `tokio::task::spawn_blocking`。
     * 或者把 API 保持 `async` 但在实现里调用 `spawn_blocking`（会更复杂）。
   * 总结：**把这些函数改为同步** 更简单且正确。

3. **缺少密钥 / nonce /参数**

   * ChaCha20/AEAD 需要 `key` 与 `nonce`（XChaCha20-Poly1305 用 24 字节 nonce）。你的 enum 里没有携带 key/nonce 管理策略。需要设计：每个 `Cipher` 包含 key/nonce 管理，或在方法调用时传入 `key: &Key, nonce: &Nonce`。
   * 如果你想做「会话密钥协商」，密钥应来自上层（DH + HKDF），不要写死在加密模块里。

4. **RC6**

   * RC6 实现并不常见于 RustCrypto；若你不是为了研究历史算法，建议用 `ChaCha20-Poly1305` 与 `AES-GCM` 作为主力算法。RC6 可暂时跳过或仅做占位。

5. **错误处理**

   * 返回 `Result<_, anyhow::Error>` 更合理，解密失败应能报错而不是静默。

#### Q&A

**Q1:**

我这里使用 async 是为了实现高并发和并行处理？没问吧。然后不能原地处理数据的话，返回 Result<Vec<u8>> 会不会太重了？能够返回&[u8] 吗？但是生命周期处理是不是很存在问题？
然后 Rc6 我需要实现，因为我需要了解历史的算法。

**Answer:**

明白了，你的思路很清晰，我来逐条分析：

---

### 1️⃣ 关于 `async` 和加密函数

* 你说的没错：如果你的目标是**在 tokio runtime 上做并发处理很多数据块**，把函数设为 `async` 是有意义的，因为可以 `tokio::spawn` 多个加密任务并发执行。
* **关键点**：加密本身是 CPU-bound（非 IO），所以 async 并不会让单个加密块变快，只是让 tokio runtime 能调度并发任务。
* 实际做法：

  * `async fn encrypt(...) {}` 可以保留，但要记住：CPU 密集型加密用 spawn_blocking 更合适。
  * 你可以直接用 `async fn` 然后内部 `spawn_blocking(move || { ... })` 来执行阻塞加密操作。

---

### 2️⃣ 关于返回类型：`Vec<u8>` vs `&[u8]`

* **不能返回 `&[u8]` 原因**：

  * 加密后的数据长度可能比原始数据长（AEAD 会加 tag），或者你可能要在函数内部生成新的 buffer。
  * Rust 的借用规则决定了**你不能返回一个引用到函数内部临时 buffer**（生命周期问题）。
* **Vec<u8> 是最简单安全的方式**：

  * 在高性能场景下，这样分配是可接受的，尤其你还需要 async 并发。
  * 如果非常在意性能，可以考虑 **内存池/预分配 buffer** 或者让调用者提供可变 slice（但生命周期和长度控制会更复杂）。
* ✅ 总结：在 RustCrypto + tokio async 的场景下，返回 `Result<Vec<u8>>` 是最稳妥、可维护的。

---

### 3️⃣ RC6 历史算法

* 没问题，你想学习历史算法就实现它。
* 注意点：

  * RC6 是**块加密**，分组长度通常 128-bit，密钥长度可变（16/24/32 字节）。
  * 如果用在文件/消息加密，需要自己实现块分组模式（ECB/CBC/CTR 等）。
  * 可以先实现 **ECB 或 CTR** 模式测试理解，再和现代 AEAD 对比。
  * **不推荐在生产使用 RC6**，仅做学习和实验。

---

### 4️⃣ 推荐 API 设计（结合你想做 async + RC6 + AEAD）

```rust
pub enum Cipher {
    Xor(Option<Span>),
    ChaCha20,
    Rc6,
}

impl Cipher {
    // async 保留，用于高并发
    pub async fn encrypt(&self, data: &[u8], key: &[u8], nonce: Option<&[u8]>) -> anyhow::Result<Vec<u8>>;

    pub async fn decrypt(&self, data: &[u8], key: &[u8], nonce: Option<&[u8]>) -> anyhow::Result<Vec<u8>>;
}
```

* `key` 对于 RC6 和 ChaCha20/Poly1305 都是必须的。
* `nonce` 对于 AEAD/流密码必需；RC6 ECB 不需要，但 CTR 模式需要。
* 返回 `Vec<u8>` 是安全、生命周期清晰的做法。
* 可以在调用处用 tokio spawn 并发加密多个数据块。

