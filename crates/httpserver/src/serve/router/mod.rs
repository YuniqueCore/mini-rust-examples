use anyhow::Result;

use std::collections::HashMap;

mod handler;
mod path;

pub use handler::*;
pub use path::*;

use crate::serve::{Method, Response, request::Request};

type Handler = fn(&Request) -> Response;

#[derive(Debug, Default)]
pub struct Router {
    routes: HashMap<RoutePath, HashMap<Method, Handler>>,
}

impl Router {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn get(&mut self, path: &str, func: Handler) -> &mut Self {
        self.routes
            .entry(path.into())
            .or_insert_with(HashMap::new)
            .insert(Method::GET, func);
        self
    }

    pub fn post(&mut self, path: &str, func: Handler) -> &mut Self {
        self.routes
            .entry(path.into())
            .or_insert_with(HashMap::new)
            .insert(Method::POST, func);
        self
    }

    pub async fn handle(&self, req: &Request) -> Response {
        if let Some(handles) = self.routes.get(&req.path.as_str().into())
            && let Some(handle) = handles.get(&req.method)
        {
            handle(req)
        } else {
            Response::default()
        }
    }
}
