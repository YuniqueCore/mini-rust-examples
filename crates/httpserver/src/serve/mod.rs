use std::{path::{Path, PathBuf}};

use anyhow::Result;

mod common;
mod request;
mod response;
mod router;

pub use common::*;
pub use response::*;
pub use router::*;
use smol::{io::AsyncWriteExt, net::TcpListener as SmolTcpListener, stream::{self, StreamExt}};

#[derive(Debug)]
pub struct StaticServeService {
    serve_path: PathBuf,
    router: Router,
}

impl StaticServeService {
    pub fn new(serve_path: &Path) -> Self {
        Self {
            serve_path: serve_path.to_path_buf(),
            router: Router::new()
        }
    }
    
    pub async fn serve(&self, tcp_listener: SmolTcpListener) -> Result<()> {
        let mut incoming =  tcp_listener.incoming();
        while let Some(stream)  = incoming.next().await  {
            let mut stream = stream?;
            let _ = stream.write(b"hello").await.inspect_err(|e|eprintln!("should write data successfully: {e}"));
        }
        Ok(())
    }

}
