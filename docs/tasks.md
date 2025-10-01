# 1. 学习准备

### 工具和前置知识

* Rust 基础语法（所有权/借用、`Result`、`Option`、`?` 操作符）。
* `cargo` 熟悉度（能创建项目、添加依赖）。
* 简单的同步编程：先写过**文件 IO**、**TCP/UDP 同步 socket**，哪怕是 blocking 的。

### 必备库

* **Tokio**（最常用 runtime）：`tokio::net`, `tokio::time`, `tokio::sync`。
* **futures**（可选）：理解 trait / combinator 基础。
* **serde + serde_json**：做简单协议时序列化/反序列化。
* **reqwest** 或 **hyper**：HTTP 客户端/服务端。

---

# 2. 阶段性小项目练习

### ✅ 阶段 1：熟悉 async/await 基础

目标：能写一个并发程序，理解 `async fn` 和 `await`。

* 小项目：写一个**异步下载器**。

  * 给定几个 URL，用 `reqwest::get` 并发抓取。
  * 用 `tokio::join!` 或 `tokio::select!` 等待结果。
  * 输出下载的字节大小。
    👉 收获：体会 async 函数是“声明异步”，而 runtime 帮你调度。

---

### ✅ 阶段 2：网络编程最小实践

目标：理解 socket 通信，能同时处理多个客户端。

* 小项目 1：**TCP Echo Server**

  * 用 `tokio::net::TcpListener` 接收连接。
  * 每个连接用 `tokio::spawn` 新建任务。
  * 客户端发啥，就原样返回。
* 小项目 2：**UDP Echo Server**

  * 用 `tokio::net::UdpSocket` 收消息，然后发回去。
    👉 收获：理解 `spawn` 就是“轻量级线程”，异步 IO 是如何并发处理的。

---

### ✅ 阶段 3：构建简单协议

目标：不仅传字节，还要有结构化数据。

* 小项目：**简易聊天室**

  * 客户端：输入文字，发给服务端。
  * 服务端：收到后广播给所有客户端。
  * 协议：用 `serde_json` 包装 `{username, message}`。
    👉 收获：练习 `tokio::sync::broadcast` 或 `mpsc` 通道，理解消息驱动。

---

### ✅ 阶段 4：综合练习

目标：把 async 用在“真实小工具”里。

* 小项目：**异步任务调度器**

  * 有一堆任务（比如 URL 检测、定时打印、数据库查询 mock）。
  * 用 `tokio::select!` 同时等待多个任务。
  * 输出执行日志。
    👉 收获：体会“反应式”写法，事件驱动编程。

---

# 3. 要求与方向

1. **代码量要求**
   每个小项目控制在 **100~300 行**，能跑就行，不要过度设计。

2. **实验性要求**

   * 尝试用 **tokio::time::sleep** 来模拟“耗时任务”。
   * 学会在循环里用 `tokio::select!` 同时等待多个事件。
   * 学会 `tokio::spawn` 来并发执行。

3. **学习方向**

   * **入门目标**：理解 runtime 的作用，知道 `async fn` 并不会开线程。
   * **进阶目标**：理解取消安全、错误处理（`?` + `Result`）。
   * **最终目标**：能独立写一个**支持多客户端的网络应用**（如聊天室/HTTP 服务）。

---

# 4. 进一步挑战

* 用 **Hyper** 写一个简单 HTTP Server（返回 JSON）。
* 学习 **WebSocket**（`tokio-tungstenite`），做个实时聊天室。
* 阅读 Tokio 的 **Mini-Redis** 示例项目（Tokio 官方教学）。

---

👉 总结：

* 路线：**下载器 → Echo Server → 聊天室 → 调度器 → HTTP/WebSocket**
* 要求：每个项目尽量小，100~300 行，先跑通，再思考优化。
* 心态：别急着造大系统，先写几个有趣的 demo，熟悉 async 的手感。
