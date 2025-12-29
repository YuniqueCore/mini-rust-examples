# Minimal HTTP Static File Server: TCP I/O Root Cause & Fix

This document explains why the previous implementation did not respond to HTTP clients and why it spammed `-> 0`, then documents the new (fixed) TCP read/write + HTTP parsing flow.

## 1. Symptoms

- Client connects but never receives a response (often times out).
- After the client times out / closes, the server prints a large amount of `0 ->` / `-> 0` repeatedly.

## 2. Root Causes (What Was Actually Broken)

### 2.1 Reading the request by waiting for EOF (deadlock with HTTP/1.1 clients)

In `src/serve/mod.rs`, the previous code used:

```rust
// (previous behavior) read the whole stream into a String
stream.read_to_string(&mut request_from_client).await;
```

`read_to_string` reads **until EOF**. In HTTP/1.1, the client typically **does not close** the TCP connection after sending the request (keep-alive is the default), because it expects the server to respond first.

So the dataflow became:

```
client: send request, keep connection open, wait response
server: wait client EOF before parsing, so never writes response
=> deadlock / timeout
```

Reference: Rust `read_to_string` reads all bytes from a reader into a string (i.e., until EOF).  
- https://doc.rust-lang.org/std/io/fn.read_to_string.html

### 2.2 Parsing headers with an endless `read_line` loop (CRLF + EOF not handled)

The previous parser relied on `find_empty_line_index` in `src/serve/common/mod.rs` to find the blank line between headers and body.

Two problems existed:

1) HTTP uses CRLF (`\r\n`) line endings, so the empty line is `"\r\n"` (length 2), not `"\n"` (length 1).  
2) `read_line` returns `Ok(0)` at EOF. If you don’t break on `len == 0`, you will loop forever.

The observable result was a tight loop repeatedly printing `0 -> ...` once the cursor reached EOF.

The fixed implementation now stops on both `"\n"` and `"\r\n"`, and also stops on EOF (`len == 0`):

```rust
// src/serve/common/mod.rs
while let Ok(len) = cursor.read_line(&mut buf) {
    if len == 0 {
        break;
    }
    if buf == "\n" || buf == "\r\n" {
        break;
    }
    // ...
}
```

## 3. Correct Mental Model: Minimal HTTP-over-TCP Server Loop

HTTP is application-level framing running on top of a byte stream (TCP). The server must *not* wait for EOF to know a request is complete.

### 3.1 “Good” per-connection state machine

```
            +------------------+
accept ---> | handle_connection| ------------------------------+
            +------------------+                              |
                    |                                          |
                    v                                          v
            +------------------+                      +------------------+
            | read_request     |                      | write_response   |
            | - read until     |                      | - status line    |
            |   \r\n\r\n       |                      | - headers        |
            | - parse head     |                      | - \r\n\r\n       |
            | - read body (CL) |                      | - body           |
            +------------------+                      +------------------+
                    |                                          |
                    v                                          v
            +------------------+                      +------------------+
            | serve_static     |                      | close/flush      |
            | - map path       |                      +------------------+
            | - read file      |
            +------------------+
```

### 3.2 Key point

- A request is “complete” once you have read enough bytes to satisfy the HTTP framing:
  - headers end at `\r\n\r\n`
  - body length is determined by `Content-Length` (for this minimal server)
- EOF is *not* part of HTTP framing for normal keep-alive requests.

## 4. What Changed in This Repo (Implementation Notes)

### 4.1 New connection handler and request reader

The new flow is implemented in `src/serve/mod.rs`:

- `handle_connection(...)`: read request → serve file → write response
- `read_request(...)`: read until `\r\n\r\n`, parse headers, then read body (optional)
- `find_subslice(...)`: locate the `\r\n\r\n` delimiter in the growing buffer

Key excerpt:

```rust
// src/serve/mod.rs
let header_end = loop {
    let n = stream.read(&mut tmp).await?;
    if n == 0 { /* EOF error */ }
    buf.extend_from_slice(&tmp[..n]);
    if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
        break pos + 4;
    }
};
```

### 4.2 Parsing headers without waiting for EOF

Once the header block is read, we parse the request line + headers via:

```rust
// src/serve/request/mod.rs
pub fn parse_head(head: &str, peer: SocketAddr) -> Result<(Self, Option<usize>)> { ... }
```

This returns `(RequestWithoutBody, content_length)`, allowing the TCP reader to fetch exactly `Content-Length` bytes for the body.

Limitations (by design, for minimal server):

- `Transfer-Encoding: chunked` is rejected (not supported).
- Only `Content-Length` bodies are supported.

### 4.3 Fixing response formatting (CRLF + Content-Length)

The previous `Response::build()` did not produce valid HTTP framing (missing required CRLF separators).

Now `Response::to_bytes()` builds:

1) status line ending with `\r\n`
2) headers, each ending with `\r\n`
3) empty line `\r\n`
4) optional body bytes

And it auto-injects:

- `Content-Length` if missing
- `Connection: close` if missing (to keep the behavior simple and predictable)

Implementation: `src/serve/response/mod.rs`.

### 4.4 Fixing header parsing

`Header::from_str` previously assigned `value = key`, which breaks all header logic.

Now it correctly parses `key: value`:

```rust
// src/serve/common/header.rs
let (key, value) = trimmed.split_once(':')?;
```

## 5. Minimal Static File Serving Behavior

`serve_static(base, req)` is implemented in `src/serve/mod.rs`:

- Methods: `GET` and `HEAD` supported, other methods return `405 Method Not Allowed` and `Allow: GET, HEAD`.
- Path traversal protection: rejects `..` and platform prefixes.
- Directory: serves `index.html` if present, otherwise `404 Not Found`.
- MIME type: guessed by file extension (simple mapping).

### 5.1 Path mapping: `/<path>` -> `./public/<path>`

The server serves files from:

```
<serve_path>/public/...
```

So:

- `GET /index.html` -> `./public/index.html` (when `--serve .`)
- `GET /` -> `./public/index.html` (directory default)

## 6. Access logs + request counter + QPS

After writing the response, `handle_connection(...)` emits one access log line:

```
<time> peer=<ip:port> method=<...> path=<...> status=<...> count=<...> qps=<...>
```

Implementation: `src/serve/mod.rs` (`handle_connection`).

## 7. Pseudocode (Current Runtime Behavior)

```text
loop:
  (stream, peer) = listener.accept()
  spawn:
    buf = read until "\r\n\r\n"
    (req, content_length) = parse_head(buf[0..header_end])
    if content_length > 0:
      read exactly content_length bytes as body
      req.body = body
    resp = serve_static(serve_path, req)
    write resp bytes
    flush
    drop stream
```

## 8. Verification Checklist

Manual checks (examples):

- `cargo run -- -l 127.0.0.1:8080 -s .`
- Create `public/index.html`, then open `http://127.0.0.1:8080/` in a browser.
- `curl -v http://127.0.0.1:8080/index.html`
- `curl -v http://127.0.0.1:8080/hello.txt`
- `curl -v http://127.0.0.1:8080/nope` (expect 404)
- `curl -I http://127.0.0.1:8080/index.html` (HEAD)

Expected:

- Server responds immediately (no need for client to close the connection first).
- No more `0 ->` spam.
- Response includes `Content-Length` and a blank line between headers and body.

## 9. Known Limitations / Next Steps

- No HTTP keep-alive / pipelining support (server always replies with `Connection: close`).
- No `Transfer-Encoding: chunked`.
- Blocking file reads (`std::fs::read`) are used for simplicity; for higher concurrency, wrap filesystem work with a blocking executor (e.g., `smol` utilities) and/or stream large files in chunks.
- No URL percent-decoding for paths.

## 10. External References

- Rust `read_to_string` (reads until EOF): https://doc.rust-lang.org/std/io/fn.read_to_string.html
- `smol` runtime docs: https://docs.rs/smol/latest/smol/
