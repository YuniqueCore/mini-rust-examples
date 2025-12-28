mod path;
mod handler;

pub use path::*;
pub use handler::*;

use crate::serve::Response;


#[derive(Debug)]
pub enum Route {
    GET(RoutePath),
    POST(RoutePath),
    UNKNOWN(RoutePath),
}

type Handler = fn(&Request) -> Response;

#[derive(Debug)]
pub struct Router {
    routes: std::collections::HashMap<String, std::collections::HashMap<Method, Handler>>,
}


