use anyhow::{Ok, Result};
use smol::net::TcpListener as SmolTcpListener;

use crate::{cmd::Args, serve::StaticServeService};

mod cmd;
mod serve;
mod utils;


pub async fn run() ->Result<()>{
    let ctrlc2 = install_signal()?;
    let args = parse_cmd()?;

    serve(args).await?;

    ctrlc2.await.expect("should be shutdown gracefully");

    Ok(())

}


async fn serve(args:Args)->Result<()>{
    let serve_path = args.serve.expect("should have a valid path for serving");
    let bind_addr = args.bind.expect("should have a valid bind addr");
    let tcp_listener = SmolTcpListener::bind(*bind_addr).await?;
    StaticServeService::new(&serve_path).serve(tcp_listener).await
}

fn install_signal() -> Result<ctrlc2::AsyncCtrlC>{
    let ctrlc = ctrlc2::AsyncCtrlC::new(move ||{
          println!("Ctrl-C received! Ready to exiting...");
        true
    }).expect("should install async ctrl c signal but failed");
   Ok(ctrlc)
}

fn parse_cmd() ->Result<Args>{
    let (args,_reminder) = Args::parse()?;

    if args.help.is_some_and(|h| h) {
        let help = Args::help();
        println!("{}",help);
        std::process::exit(0);
        // exit
    }

    Ok(args)
}

