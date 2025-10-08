use std::{io, path::PathBuf, str::FromStr};

use anyverr::{AnyError, AnyResult};
use en_de::Cipher;

enum CipherAction {
    Encrypt,
    Decrypt,
}

impl FromStr for CipherAction {
    type Err = io::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "decrypt" => Ok(Self::Decrypt),
            "encrypt" | _ => Ok(Self::Encrypt),
        }
    }
}

struct Args {
    action: CipherAction,
    input: PathBuf,
    output: PathBuf,
    cipher: Option<Cipher>,
}

fn parse_args() -> Result<Args, lexopt::Error> {
    use lexopt::prelude::*;
    let mut action = CipherAction::Encrypt;
    let mut cipher = None;
    let mut input = PathBuf::new();
    let mut output = PathBuf::new();
    let mut parser = lexopt::Parser::from_env();
    while let Some(arg) = parser.next()? {
        match arg {
            Short('a') | Long("action") => {
                let action_opt = parser.optional_value();
                if let Some(v) = action_opt {
                    action = CipherAction::from_str(&v.into_string()?)
                        .map_err(|e| lexopt::Error::Custom(Box::new(e)))?;
                }
            }

            Short('i') | Long("input") => {
                input = parser.value()?.parse()?;
            }
            Short('o') | Long("output") => {
                output = parser.value()?.parse()?;
            }
            Short('c') | Long("cipher") => {
                let cipher_opt = parser.optional_value();
                cipher = if let Some(v) = cipher_opt {
                    Some(
                        Cipher::from_str(&v.into_string()?)
                            .map_err(|e| lexopt::Error::Custom(Box::new(e)))?,
                    )
                } else {
                    None
                };
            }
            Long("help") => {
                println!(
                    "Usage: deal-file [-i|--input=file_path] [-o|--output=output_file|] [-c|--cipher=cipher]"
                );
                std::process::exit(0);
            }
            _ => return Err(arg.unexpected()),
        }
    }

    Ok(Args {
        action,
        input,
        output,
        cipher,
    })
}

fn main() -> AnyResult<()> {
    let args = parse_args().map_err(AnyError::wrap)?;

    Ok(())
}

fn encrypt(input: &[u8]) -> Vec<[u8]> {}

fn decrypt(input: &[u8]) -> Vec<[u8]> {}
