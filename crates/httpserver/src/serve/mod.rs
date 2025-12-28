use std::path::PathBuf;

mod router;
mod response;
mod common;

pub use router::*;
pub use common::*;
pub use response::*;

#[derive(Debug)]
pub struct StaticServeService{
    serve_path: PathBuf,
    router: Router,
}