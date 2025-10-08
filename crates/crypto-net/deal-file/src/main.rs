use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::PathBuf,
    str::FromStr,
    sync::LazyLock,
    time::SystemTime,
};

use anyverr::{AnyError, AnyResult};
use en_de::Cipher;

#[derive(Debug)]
enum CipherAction {
    Encrypt,
    Decrypt,
}

impl std::fmt::Display for CipherAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            CipherAction::Encrypt => "encrypt",
            CipherAction::Decrypt => "decrypt",
        };
        write!(f, "{}", value)
    }
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

#[derive(Debug)]
struct Args {
    action: CipherAction,
    cipher: Option<Cipher>,
    input: PathBuf,
    output: PathBuf,
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
        cipher,
        input,
        output,
    })
}

fn main() -> AnyResult<()> {
    let args = parse_args().map_err(AnyError::wrap)?;
    println!("args: {:?}", args);
    let input_file = if args.input.is_file() {
        Ok(OpenOptions::new()
            .read(true)
            .open(&args.input)
            .map_err(AnyError::wrap)?)
    } else {
        Err(AnyError::quick(
            "input file not exists",
            anyverr::ErrKind::EntityAbsence,
        ))
    }?;
    let output_file = if !args.output.exists() {
        Ok(OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&args.output)
            .map_err(AnyError::wrap)?)
    } else {
        Err(AnyError::quick(
            "output file already exists",
            anyverr::ErrKind::ValueValidation,
        ))
    }?;

    let timer = SystemTime::now();

    if let Err(e) = handle(&args, input_file, output_file) {
        std::fs::remove_file(&args.output).map_err(AnyError::wrap)?;
        return Err(e);
    };

    let elapsed = timer.elapsed().map_err(AnyError::wrap)?;
    println!(
        "[{}ms] Successfully {} file: {} and save the content to file: {}",
        elapsed.as_millis(),
        args.action,
        args.input.display(),
        args.output.display()
    );

    Ok(())
}

fn handle(
    args: &Args,
    mut input_file: std::fs::File,
    mut output_file: std::fs::File,
) -> AnyResult<()> {
    let mut buf = [0u8; 1024];
    Ok(loop {
        match input_file.read(&mut buf) {
            Ok(0) => {
                // EOF
                break;
            }
            Ok(n) => {
                let handled_content = match &args.action {
                    CipherAction::Encrypt => encrypt(&buf[..n], &args.cipher),
                    CipherAction::Decrypt => decrypt(&buf[..n], &args.cipher),
                }?;
                output_file
                    .write_all(&handled_content)
                    .map_err(AnyError::wrap)?;
            }
            Err(e) => {
                return Err(AnyError::wrap(e));
            }
        }
    })
}

const KEY_STR: &str = "THE DEAL_FILE DEFAULT KEY FOR TESTING";
const NONCE_STR: &str = "THE DEAL_FILE DEFAULT NONCE FOR TESTING";

static KEY: LazyLock<&[u8]> = LazyLock::new(|| &KEY_STR.as_bytes()[..32]);
static NONCE: LazyLock<&[u8]> = LazyLock::new(|| &NONCE_STR.as_bytes()[..24]);

fn encrypt(input: &[u8], cipher: &Option<Cipher>) -> AnyResult<Vec<u8>> {
    let cipher = cipher.as_ref().unwrap_or(&Cipher::XChaCha20Poly1305);
    cipher.encrypt(input, &KEY, Some(&NONCE))
}

fn decrypt(input: &[u8], cipher: &Option<Cipher>) -> AnyResult<Vec<u8>> {
    let cipher = cipher.as_ref().unwrap_or(&Cipher::XChaCha20Poly1305);
    cipher.decrypt(input, &KEY, Some(&NONCE))
}
