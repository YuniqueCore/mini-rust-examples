use std::net::SocketAddr;
use anyhow::Result;
use mea::mpsc::{UnboundedReceiver, UnboundedSender};
use smol::net::TcpListener;


#[derive(Debug)]
pub struct CommunicatePeer{
    pub reverse_tx:UnboundedSender<Vec<u8>>,
    pub reverse_rx:UnboundedReceiver<Vec<u8>>,
    pub local_tx:UnboundedSender<Vec<u8>>,
    pub local_rx:UnboundedReceiver<Vec<u8>>,
}



pub async fn handle_reverse_target(target_addr:SocketAddr)->Result<()>{

    Ok(())
}


pub async fn handle_local_target(target_addr:SocketAddr)->Result<()>{
    let tcp_listener = TcpListener::bind(target_addr).await?;
    loop {
        let (stream,peer) = tcp_listener.accept().await?;
    }
    Ok(())
}