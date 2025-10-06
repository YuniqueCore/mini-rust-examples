use std::{io::ErrorKind, net::SocketAddr, str::FromStr};

use anyverr::{AnyError, AnyResult};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    ip: String,
    port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".into(),
            port: 59411,
        }
    }
}

pub async fn run(config: Config) -> AnyResult<()> {
    let fallback_addr = "127.0.0.1:0".parse().unwrap();
    let socket_addr_str = format!("{}:{}", config.ip, config.port);
    let socket_addr = SocketAddr::from_str(&socket_addr_str).unwrap_or(fallback_addr);
    let bind_res = TcpListener::bind(socket_addr).await;
    println!("First bind");
    let listener = match bind_res {
        Ok(l) => Ok(l),
        Err(e) => {
            if e.kind() == ErrorKind::AddrInUse {
                Ok(TcpListener::bind(fallback_addr).await.unwrap())
            } else {
                Err(AnyError::quick(
                    format!("Failed to bind to local: {}", e),
                    anyverr::ErrKind::ValueValidation,
                ))
            }
        }
    }?;

    println!("successfully bind to:  {}", listener.local_addr().unwrap());
    let mut conn_handles = vec![];
    loop {
        let (stream, addr) = listener.accept().await.map_err(|e| AnyError::wrap(e))?;
        conn_handles.push(tokio::spawn(process_conn(stream, addr)));
    }
}

async fn process_conn(mut stream: tokio::net::TcpStream, addr: SocketAddr) {
    println!("client {} connected", addr);
    let mut buf = vec![0u8; 2048];
    let (mut rx, mut tx) = stream.split();
    loop {
        match rx.read(&mut buf).await {
            Ok(0) => {
                println!("client: {} closed when reading", addr);
                break;
            }
            Ok(rn) => {
                let mut written_len = 0;
                while written_len < rn {
                    match tx.write(&mut buf[written_len..rn]).await {
                        Ok(0) => {
                            println!("client: {} closed when writing", addr);
                            break;
                        }
                        Ok(wn) => written_len += wn,
                        Err(e) => {
                            eprintln!("failed to write: {}", e);
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("failed to write: {}", e);
                break;
            }
        }

        println!("current data: {}", String::from_utf8_lossy(&buf[..]))
    }
}
