use std::path::PathBuf;

use anyverr::{AnyError, AnyResult};
use en_de::Cipher;

struct Args {
    input: PathBuf,
    output: PathBuf,
    cipher: Option<Cipher>,
}

fn parse_args() -> Result<Args, lexopt::Error> {
    use lexopt::prelude::*;

    let mut cipher = None;
    let mut input = PathBuf::new();
    let mut output = PathBuf::new();
    let mut parser = lexopt::Parser::from_env();
    while let Some(arg) = parser.next()? {
        match arg {
            Short('i') | Long("input") => {
                input = parser.value()?.parse()?;
            }
            Short('o') | Long("output") => {
                output = parser.value()?.parse()?;
            }
            Value(val) if cipher.is_none() => {
                cipher = Some(val.parse()?);
            }
            Long("help") => {
                println!(
                    "Usage: deal-file [-i|--input=file_path] [-o|--output=output_file|] CIPHER"
                );
                std::process::exit(0);
            }
            _ => return Err(arg.unexpected()),
        }
    }

    Ok(Args {
        input,
        output,
        cipher,
    })
}

fn main() -> AnyResult<()> {
    let args = parse_args().map_err(AnyError::wrap)?;

    Ok(())
}
