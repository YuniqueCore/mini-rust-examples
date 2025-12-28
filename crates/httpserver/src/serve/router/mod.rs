mod path;
mod handler;

pub use path::*;
pub use handler::*;


#[derive(Debug)]
pub enum Route {
    GET(RoutePath),
    POST(RoutePath),
    UNKNOWN(RoutePath),
}


#[derive(Debug)]
pub struct Router{
    routes:Vec<Route>,
    methods:Vec<String>
}


