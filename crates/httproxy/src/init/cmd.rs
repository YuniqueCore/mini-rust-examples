use std::str::FromStr;

use anyhow::Result;

use crate::{cmd::{Args, LogLevel}, init::logger};

pub fn init() -> Result<Args> {
    let (mut args, _reminder) = Args::parse()?;
    logger::init(
        &args.log_level.take().unwrap_or(LogLevel::from_str("info")?),
        args.colored.unwrap_or(false),
    )?;
    log::debug!("{:?}, {:?}", args, _reminder);

    if args.help.is_some_and(|h| h) {
        let help = Args::help();
        println!("{}", help);
        std::process::exit(0);
        // exit
    }

    Ok(args)
}
