use std::{
    fs::OpenOptions,
    io::{self, Read, Write},
    path::PathBuf,
    str::FromStr,
    sync::LazyLock,
    time::SystemTime,
};

use anyverr::{AnyError, AnyResult};
use en_de::{Cipher, StreamDecryptor, StreamEncryptor};

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
    cipher: Cipher,
    input: PathBuf,
    output: PathBuf,
}

fn parse_args() -> Result<Args, lexopt::Error> {
    use lexopt::prelude::*;
    let mut action = CipherAction::Encrypt;
    let mut cipher = Cipher::XChaCha20Poly1305;
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
                cipher = parser.value()?.parse()?;
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

    if let Err(e) = handle_stream(&args, input_file, output_file) {
        std::fs::remove_file(&args.output).map_err(AnyError::wrap)?;
        return Err(e);
    };

    let elapsed = timer.elapsed().map_err(AnyError::wrap)?;
    println!(
        "[{}ms][{:?}] Successfully {} file: {} and save the content to file: {}",
        elapsed.as_millis(),
        args.cipher,
        args.action,
        args.input.display(),
        args.output.display()
    );

    Ok(())
}

fn handle_full_block(
    args: &Args,
    mut input_file: std::fs::File,
    mut output_file: std::fs::File,
) -> AnyResult<()> {
    let mut buf = Vec::new();
    input_file.read_to_end(&mut buf).map_err(AnyError::wrap)?;
    let handled_content = match &args.action {
        CipherAction::Encrypt => encrypt(&buf, &args.cipher),
        CipherAction::Decrypt => decrypt(&buf, &args.cipher),
    }?;

    output_file
        .write_all(&handled_content)
        .map_err(AnyError::wrap)
}

const KEY_STR: &str = "THE DEAL_FILE DEFAULT KEY FOR TESTING";
const NONCE_STR: &str = "THE DEAL_FILE DEFAULT NONCE FOR TESTING";

static KEY: LazyLock<&[u8]> = LazyLock::new(|| &KEY_STR.as_bytes()[..32]);
static NONCE: LazyLock<&[u8]> = LazyLock::new(|| &NONCE_STR.as_bytes()[..24]);

fn encrypt(input: &[u8], cipher: &Cipher) -> AnyResult<Vec<u8>> {
    cipher.encrypt(input, &KEY, Some(&NONCE))
}

fn decrypt(input: &[u8], cipher: &Cipher) -> AnyResult<Vec<u8>> {
    cipher.decrypt(input, &KEY, Some(&NONCE))
}

// #####################
// stream crypto
// #####################

/// 流式处理文件，支持大文件的分块加密解密
fn handle_stream(
    args: &Args,
    input_file: std::fs::File,
    output_file: std::fs::File,
) -> AnyResult<()> {
    match &args.cipher {
        Cipher::XChaCha20Poly1305 => handle_stream_xchacha20(args, input_file, output_file),
        Cipher::Xor(span) => handle_stream_xor(args, input_file, output_file, *span),
        Cipher::Rc6 => Err(AnyError::quick(
            "RC6 stream encryption not implemented",
            anyverr::ErrKind::InfrastructureFailure,
        )),
    }
}

/// XChaCha20Poly1305 流式处理
fn handle_stream_xchacha20(
    args: &Args,
    mut input_file: std::fs::File,
    mut output_file: std::fs::File,
) -> AnyResult<()> {
    match &args.action {
        CipherAction::Encrypt => {
            // 创建流式加密器
            let mut encryptor = StreamEncryptor::new(&KEY, &NONCE)?;
            let mut buf = [0u8; 1024];

            loop {
                match input_file.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let encrypted_chunk = encryptor.encrypt_chunk(&buf[..n])?;
                        output_file
                            .write_all(&encrypted_chunk)
                            .map_err(AnyError::wrap)?;
                    }
                    Err(e) => return Err(AnyError::wrap(e)),
                }
            }

            // 将 nonce 写入文件开头，以便解密时使用
            let _final_nonce = encryptor.finalize();
            // 注意：这里简化处理，实际应用中可能需要更复杂的格式
        }
        CipherAction::Decrypt => {
            // 创建流式解密器
            let mut decryptor = StreamDecryptor::new(&KEY, &NONCE)?;
            let mut buf = [0u8; 1040]; // 稍大一些，因为加密后数据会变大

            loop {
                match input_file.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let decrypted_chunk = decryptor.decrypt_chunk(&buf[..n])?;
                        output_file
                            .write_all(&decrypted_chunk)
                            .map_err(AnyError::wrap)?;
                    }
                    Err(e) => return Err(AnyError::wrap(e)),
                }
            }
        }
    }

    Ok(())
}

/// XOR 流式处理
fn handle_stream_xor(
    args: &Args,
    mut input_file: std::fs::File,
    mut output_file: std::fs::File,
    span: Option<u16>,
) -> AnyResult<()> {
    let mut buf = [0u8; 1024];
    let mut byte_counter = 0u64;

    loop {
        match input_file.read(&mut buf) {
            Ok(0) => break, // EOF
            Ok(n) => {
                let chunk = &buf[..n];
                let handled_content = match &args.action {
                    CipherAction::Encrypt => {
                        // XOR 流式加密，保持跨块的状态
                        let mut result = Vec::with_capacity(chunk.len());
                        for &byte in chunk {
                            let should_skip = if let Some(span_val) = span {
                                byte_counter % span_val as u64 == 0
                            } else {
                                false
                            };

                            let processed_byte = if should_skip {
                                byte
                            } else {
                                byte ^ KEY[byte_counter as usize % KEY.len()]
                            };

                            result.push(processed_byte);
                            byte_counter += 1;
                        }
                        result
                    }
                    CipherAction::Decrypt => {
                        // XOR 解密与加密相同
                        let mut result = Vec::with_capacity(chunk.len());
                        for &byte in chunk {
                            let should_skip = if let Some(span_val) = span {
                                byte_counter % span_val as u64 == 0
                            } else {
                                false
                            };

                            let processed_byte = if should_skip {
                                byte
                            } else {
                                byte ^ KEY[byte_counter as usize % KEY.len()]
                            };

                            result.push(processed_byte);
                            byte_counter += 1;
                        }
                        result
                    }
                };

                output_file
                    .write_all(&handled_content)
                    .map_err(AnyError::wrap)?;
            }
            Err(e) => return Err(AnyError::wrap(e)),
        }
    }

    Ok(())
}
