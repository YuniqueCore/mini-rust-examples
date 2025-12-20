use std::io;

use pastey::paste;

#[derive(Debug, Clone)]
pub enum RespStatusCode {
    OneXX(String),
    TwoXX(String),
    ThreeXX(String),
    FourXX(String),
    FiveXX(String),
    Unknown(String),
}

macro_rules! impl_response_status_code {
    (
        $enum_parent:ident :: $enum_ident:ident
    ) => {
        paste! {
            impl $enum_parent {
                #[allow(unused)]
                pub fn [< $enum_ident:replace("XX", ""):lower >](msg: &str) -> Self {
                    $enum_parent::$enum_ident(msg.into())
                }
            }
        }
    };
}

impl_response_status_code!(RespStatusCode::OneXX);
impl_response_status_code!(RespStatusCode::TwoXX);
impl_response_status_code!(RespStatusCode::ThreeXX);
impl_response_status_code!(RespStatusCode::FourXX);
impl_response_status_code!(RespStatusCode::FiveXX);
impl_response_status_code!(RespStatusCode::Unknown);

impl Default for RespStatusCode {
    fn default() -> Self {
        Self::unknown("Unknown")
    }
}

impl RespStatusCode {
    pub fn parse(code: &str, msg: &str) -> Result<Self, io::Error> {
        let num: u16 = code.parse().map_err(io::Error::other)?;

        let msg = &format!("{num} {msg}");
        Ok(match num {
            n if n >= 600 => Self::unknown(msg),
            n if n >= 500 => Self::five(msg),
            n if n >= 400 => Self::four(msg),
            n if n >= 300 => Self::three(msg),
            n if n >= 200 => Self::two(msg),
            n if n >= 100 => Self::one(msg),
            _ => Self::unknown(msg),
        })
    }
}
