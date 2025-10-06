沿着 **RustCrypto → AEAD → 密钥协商 → TLS** 这条线来构建学习路径。

---

## 🧩 阶段 1：文件加密工具（对称 AEAD）

**目标**：用现代 AEAD 加密一个文件，再解密回来。
**重点概念**：对称加密、nonce、tag（认证标签）、AEAD。

### ✅ 推荐库

| 功能      | Crate                                                  | 用途                         | 说明                                  |
| --------- | ------------------------------------------------------ | ---------------------------- | ------------------------------------- |
| AEAD 算法 | [`chacha20poly1305`](https://docs.rs/chacha20poly1305) | XChaCha20-Poly1305 加密/解密 | RustCrypto 提供的高质量 AEAD 算法实现 |
| 随机数    | [`rand_core`](https://docs.rs/rand_core)               | 生成随机 nonce               | 可搭配 `OsRng` 使用                   |
| 工具      | [`anyhow`](https://docs.rs/anyhow)                     | 错误处理                     | 方便调试错误                          |
| 文件 IO   | 标准库 (`std::fs`)                                     | 读写文件                     | 不需额外库                            |

> 📘 额外选项：想对比性能/格式，可试试 `aes-gcm` crate。
> 两者都实现了 `aead` trait，API 类似。

---

## 🔑 阶段 2：建立会话密钥（密钥协商 + HKDF）

**目标**：理解公钥交换 + 从共享密钥派生出对称密钥。
**重点概念**：Diffie-Hellman（DH）、HKDF、密钥派生。

### ✅ 推荐库

| 功能             | Crate                                                  | 用途                                 | 说明                        |
| ---------------- | ------------------------------------------------------ | ------------------------------------ | --------------------------- |
| 椭圆曲线密钥交换 | [`x25519-dalek`](https://docs.rs/x25519-dalek)         | 生成公私钥 + 计算共享密钥            | 基于 Curve25519，安全且现代 |
| 密钥派生         | [`hkdf`](https://docs.rs/hkdf)                         | 用 HKDF 从共享密钥派生出固定长度密钥 | RustCrypto 实现             |
| AEAD 加密        | [`chacha20poly1305`](https://docs.rs/chacha20poly1305) | 用派生密钥加密消息                   | 和上阶段复用                |
| 随机数           | [`rand_core`]                                          | 生成临时私钥                         | 同前                        |

> 💡 这一步让你亲手体验“TLS 密钥协商的内核”。

---

## 🔒 阶段 3：TLS 实战（Rust Async 网络层）

**目标**：掌握 TLS 通道加密的结构。
**重点概念**：握手、证书、ECDHE、会话密钥、ALPN。

### ✅ 推荐库

| 功能                | Crate                                          | 用途                             | 说明                       |
| ------------------- | ---------------------------------------------- | -------------------------------- | -------------------------- |
| TLS 协议            | [`rustls`](https://docs.rs/rustls)             | TLS 1.3 协议栈                   | RustCrypto 生态核心 TLS 库 |
| Async 集成          | [`tokio-rustls`](https://docs.rs/tokio-rustls) | 在 tokio runtime 里跑 TLS stream | 用于 async TCP             |
| 证书生成            | [`rcgen`](https://docs.rs/rcgen)               | 生成自签证书                     | 实验阶段方便使用           |
| 客户端 HTTP（可选） | [`reqwest`](https://docs.rs/reqwest)           | 做 TLS 客户端测试                | 基于 rustls                |

---

## 🧱 附加推荐：基础算法层（可选打底库）

这些库你暂时不用手写调用，但建议了解：

| 类别              | Crate                                                                                  | 示例算法         |
| ----------------- | -------------------------------------------------------------------------------------- | ---------------- |
| 哈希函数          | [`sha2`](https://docs.rs/sha2), [`blake2`](https://docs.rs/blake2)                     | SHA-256, BLAKE2b |
| 密码学 trait 接口 | [`aead`](https://docs.rs/aead), [`cipher`](https://docs.rs/cipher)                     | 通用接口定义     |
| 数字签名          | [`ed25519-dalek`](https://docs.rs/ed25519-dalek)                                       | 之后用来签名消息 |
| PEM/DER 处理      | [`pem-rfc7468`](https://docs.rs/pem-rfc7468), [`x509-cert`](https://docs.rs/x509-cert) | 证书解析         |

---

## 🧩 总结：路线依赖清单

|    阶段    | 必备 Crate                                              | 可选辅助                       |
| :--------: | :------------------------------------------------------ | :----------------------------- |
| 1️⃣ 文件加密 | `chacha20poly1305`, `rand_core`, `anyhow`               | `aes-gcm`                      |
| 2️⃣ 会话密钥 | `x25519-dalek`, `hkdf`, `chacha20poly1305`, `rand_core` | `serde_json`（传输结构化数据） |
|   3️⃣ TLS    | `rustls`, `tokio-rustls`, `rcgen`, `tokio`              | `reqwest`                      |


