mod args;

pub use args::*;
use sarge::sarge;

sarge! {
    #[derive(Debug)]
    pub Args,

    /// Legacy doc identifier was `>`; prefer Rust doc comments (`/// ...`).
    /// socket addr
    #ok 's' pub socket_addr: String = "127.0.0.1:9912" ,
    #ok 't' pub target_addr: String= "127.0.0.1:8000",
    #ok 'H' pub headers: HeadersArg,
    #ok 'd' pub data:Vec<String> = vec!["{'name': 'hello', 'data': 'world', 'age': 18 }"],
    #err 'h' pub help:bool = false,
}
