mod path;
mod handler;

use std::collections::HashMap;

pub use path::*;
pub use handler::*;

use crate::serve::{Method, Response, request::Request};

type Handler = fn(&Request) -> Response;

#[derive(Debug,Default)]
pub struct Router {
    routes: HashMap<RoutePath, HashMap<Method, Handler>>,
}

impl Router  {
    pub fn new() ->Self{
        Default::default()
    }

    pub fn get(&mut self, path:&str, func:Handler)->&mut Self {
        self.routes.entry(path.into()).or_insert_with(HashMap::new).insert(Method::GET,func);
        self 
    }    

    pub fn post(&mut self, path:&str, func:Handler)->&mut Self {
        self.routes.entry(path.into()).or_insert_with(HashMap::new).insert(Method::POST,func);
        self 
    }
}
