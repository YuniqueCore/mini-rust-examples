mod args;

use core::error;
use std::process;

pub use args::*;
use sarge::sarge;

sarge! {
    #[derive(Debug,Clone)]
    pub Args,

    /// Legacy doc identifier was `>`; prefer Rust doc comments (`/// ...`).
    /// socket addr
    #ok 's' pub socket_addr: String = "127.0.0.1:9912" ,
    #ok 't' pub target_addr: String= "127.0.0.1:8000",
    #ok 'H' pub headers: HeadersArg,
    #ok 'd' pub data:Vec<String> = vec!["{'name': 'hello', 'data': 'world', 'age': 18 }"],
    #err 'h' pub help:bool = false,
}

pub(crate) fn parse() -> Result<(Args, Vec<String>), Box<dyn error::Error + 'static>> {
    let (args, mut remainder) = Args::parse()?;
    if args.help.ok().is_some_and(|b| b) {
        let help = Args::help();
        println!("{help}");
        process::exit(0);
    }
    remainder.remove(0);
    println!("{args:#?}\n{remainder:?}\n\n");
    Ok((args, remainder))
}
