use std::path::PathBuf;


mod router;

pub use router::*;


#[derive(Debug)]
pub struct StaticServeService{
    serve_path: PathBuf,
    router: Router,
}