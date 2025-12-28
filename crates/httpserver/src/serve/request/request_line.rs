use std::str::FromStr;
use anyhow::Result;

use crate::serve::Method;

#[derive(Debug)]
pub struct RequestLine {
    method: Method,
    route_path: String,
    protocl: String,
}

impl RequestLine {
    pub fn new<S: Into<String>>(method: S, route_path: S, protocl: S) -> Self {
        Self {
            method: Method::from_str(&method.into()).unwrap_or(Method::GET),
            route_path: route_path.into(),
            protocl: protocl.into(),
        }
    }

    pub fn split(&self) ->(Method, String, String){
        (self.method.clone(),self.route_path.clone(),self.protocl.clone())
    }
}

impl FromStr for RequestLine {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let splits: Vec<&str> = s.trim().splitn(3, ' ').collect();
        if splits.len() != 3 {
            return Err(anyhow::anyhow!("failed to parse the status line: {}", s));
        }

        Ok(Self {
            method: splits[0].into(),
            route_path: splits[1].to_owned(),
            protocl: splits[2].to_owned(),
        })
    }
}

impl ToString for RequestLine {
    fn to_string(&self) -> String {
        format!("{} {} {}", self.method, self.route_path, self.protocl)
    }
}