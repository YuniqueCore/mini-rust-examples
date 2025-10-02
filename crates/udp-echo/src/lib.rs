use std::{net::SocketAddr, str::FromStr};

use anyverr::{AnyError, AnyResult};
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    ip: String,
    port_start: u16,
    port_end: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".into(),
            port_start: 59412,
            port_end: 59415,
        }
    }
}

pub async fn run(config: Config) -> AnyResult<()> {
    let mut addrs = vec![];
    for port in config.port_start..=config.port_end {
        let socket_addr_str = format!("{}:{}", config.ip, port);
        let socket_addr =
            SocketAddr::from_str(&socket_addr_str).expect("should be valid socketAddr");
        addrs.push(socket_addr);
    }
    let udp_socket = UdpSocket::bind(&addrs[..])
        .await
        .map_err(|e| AnyError::wrap(e))?;
    println!("Udp bind on: {}", udp_socket.local_addr().unwrap());

    let echo_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 2048];
        loop {
            match udp_socket.recv_from(&mut buf).await {
                Ok((0, t)) => {
                    println!("target send zero data {}", t);
                    continue;
                }
                Ok((n, t)) => {
                    let _ = udp_socket.send_to(&buf[..n], t).await; // Dont care the result
                    println!("send data {} to {}", String::from_utf8_lossy(&buf[..n]), t);
                }
                Err(e) => {
                    eprintln!("failed to send data: {}", e);
                    break;
                }
            }
        }
    });

    echo_task.await.map_err(|e| AnyError::wrap(e))?;

    Ok(())
}
