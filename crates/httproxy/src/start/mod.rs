use anyhow::Result;
use mea::mpsc::{self, UnboundedReceiver, UnboundedSender};
use smol::net::TcpListener;
use std::{net::SocketAddr, str::FromStr};

#[derive(Debug)]
pub struct CommunicatePeer<'a> {
    pub reverse_tx: UnboundedSender<&'a [u8]>,
    pub reverse_rx: UnboundedReceiver<&'a [u8]>,
    pub local_tx: UnboundedSender<&'a [u8]>,
    pub local_rx: UnboundedReceiver<&'a [u8]>,
}

async fn double_tx_rx() -> Result<()> {
    let (mut reverse_tx,mut reverse_rx) = mpsc::unbounded::<&[u8]>(); 
    let (mut local_tx,mut local_rx) = mpsc::unbounded::<&[u8]>(); 

    let local_socket_addr = SocketAddr::from_str("192.168.5.5:4632").unwrap();
    let local_tcp = TcpListener::bind(local_socket_addr).await?;

    


    Ok(())
}

pub async fn handle_reverse_target(target_addr: SocketAddr) -> Result<()> {
    Ok(())
}

pub async fn handle_local_target(target_addr: SocketAddr) -> Result<()> {
    let tcp_listener = TcpListener::bind(target_addr).await?;
    loop {
        let (stream, peer) = tcp_listener.accept().await?;
    }
    Ok(())
}
