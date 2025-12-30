use anyhow::Result;
use httparse::{Request, Status};
use mea::mpsc::{self, UnboundedReceiver, UnboundedSender};
use smol::{io::AsyncReadExt, net::TcpListener};
use std::{net::SocketAddr, str::FromStr};

#[derive(Debug)]
pub struct CommunicatePeer<'a> {
    pub reverse_tx: UnboundedSender<&'a [u8]>,
    pub reverse_rx: UnboundedReceiver<&'a [u8]>,
    pub local_tx: UnboundedSender<&'a [u8]>,
    pub local_rx: UnboundedReceiver<&'a [u8]>,
}

async fn double_tx_rx() -> Result<()> {
    let (mut reverse_tx, mut reverse_rx) = mpsc::unbounded::<&[u8]>();
    let (mut local_tx, mut local_rx) = mpsc::unbounded::<&[u8]>();

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
        let (mut stream, peer) = tcp_listener.accept().await?;
        log::info!("get request from {peer}");

        let headers_bytes = handle_conn(&mut stream, &peer).await?;
    }
    Ok(())
}

async fn handle_conn<'a>(
    stream: &mut smol::net::TcpStream,
    peer: &SocketAddr,
) -> Result<Option<Vec<u8>>> {
    const INIT_SIZE: usize = 4096;
    let mut tmp: Vec<u8> = Vec::with_capacity(INIT_SIZE);
    let mut buf = [0u8; INIT_SIZE];

    let header_end = loop {
        let len = stream.read(&mut buf).await?;
        if len == 0 {
            return Err(anyhow::anyhow!("Peer:{peer} connection closed"));
        };
        tmp.extend_from_slice(&buf[..len]); // TODO: need to use unsafe { } to extend tmp size more efficiently.

        let mut headers = [httparse::EMPTY_HEADER; 64];
        let mut req = httparse::Request::new(&mut headers);

        let http_parse = req.parse(&tmp)?;
        match http_parse {
            Status::Complete(n) => break n,
            Status::Partial => {
                if tmp.len() > 64 * 1024 {
                    return Err(anyhow::anyhow!("Peer:{peer} header too large"));
                }
            }
        }
    };
    // header bytes
    let header_bytes = &tmp[..header_end];
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = Request::new(&mut headers);
    let _ = req.parse(header_bytes)?;

    if req.method.is_none() || req.path.is_none() || req.version.is_none() {
        return Err(anyhow::anyhow!("Wrong request content: {req:?}"));
    }

    let mut content_length: usize = 0;
    if let Some(length_header) = req
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-length"))
    {
        let len_str = core::str::from_utf8(length_header.value)?;
        content_length = len_str.trim().parse()?;
    }

    let body_bytes = if content_length > 0 {
        const MAX_SUPPORT_SIZE: usize = 10 * 1024 * 1024;
        if content_length > MAX_SUPPORT_SIZE {
            return Err(anyhow::anyhow!(
                "The content size too large!!! {content_length} bytes"
            ));
        }

        let mut body = Vec::with_capacity(2048);
        body.extend_from_slice(&tmp[header_end..]);

        loop {
            let len = stream.read(&mut buf).await?;
            if len == 0 {
                return Err(anyhow::anyhow!("Peer:{peer} connection closed"));
            };

            if body.len() < content_length {
                body.extend_from_slice(&buf[..len]); // TODO: need to use unsafe { } to extend tmp size more efficiently.
                continue;
            }

            break;
        }

        if body.len() > content_length {
            body.truncate(content_length);
        }

        Some(body)
    } else {
        None
    };

    Ok(body_bytes)
}
