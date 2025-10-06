use std::{net::SocketAddr, str::FromStr};

use anyverr::{AnyError, AnyResult};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::broadcast,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    ip: String,
    port: u16,
}

#[derive(Debug, Clone)]
pub struct Msg {
    pub user: String,
    pub data: String,
}

impl Msg {
    pub fn to_string(user: String, data: String) -> String {
        format!("[{}]: {}", user, data)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".into(),
            port: 59413,
        }
    }
}

pub async fn run(config: Config) -> AnyResult<()> {
    let socket_addr_str = format!("{}:{}", config.ip, config.port);
    let socket_addr = SocketAddr::from_str(&socket_addr_str).unwrap();
    let tcp_listener = TcpListener::bind(socket_addr).await.unwrap_or_else(|_| {
        TcpListener::from_std(
            std::net::TcpListener::bind("127.0.0.1:0".parse::<SocketAddr>().unwrap()).unwrap(),
        )
        .unwrap()
    });

    println!("Tcp listen on: {}", tcp_listener.local_addr().unwrap());

    let (tx, mut rx) = broadcast::channel::<String>(2);

    tokio::spawn(async move {
        loop {
            if let Ok(data) = rx.recv().await {
                println!("recv rx data: {data}");
            }
        }
    });

    loop {
        let (mut stream, target) = tcp_listener.accept().await.map_err(|e| AnyError::wrap(e))?;
        let tx = tx.clone();
        let mut rx = tx.subscribe();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];

            loop {
                tokio::select! {
                  n = stream.read(&mut buf) => {
                     match n {
                         Ok(0) =>{
                             println!("{} closed", target);
                             break;
                         },
                         Ok(n) => {
                             let _ = tx.send(Msg::to_string(
                                 target.to_string(),
                                 String::from_utf8_lossy(&buf[..n]).into_owned(),
                             ));
                         },
                         Err(e)=>{
                            eprintln!("Err: failed to send {}",e);
                         }
                     }
                 }
                 d= rx.recv() => {
                    if let Ok(data) = d {
                       if let Err(e) =  stream.write(data.as_bytes()).await{
                            eprintln!("failed to send recv data: {e}");
                       };
                    }
                 }
                }
            }
        });
    }
}
