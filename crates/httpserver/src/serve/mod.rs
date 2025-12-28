use std::path::PathBuf;

mod router;
mod response;

pub use router::*;
pub use response::*;

#[derive(Debug)]
pub struct StaticServeService{
    serve_path: PathBuf,
    router: Router,
}