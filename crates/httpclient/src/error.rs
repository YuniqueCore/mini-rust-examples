pub type Result<T> = core::result::Result<T, Box<dyn core::error::Error>>;

#[derive(Debug, Clone)]
pub enum Error {
    IO(String),
    Net(String),
}

impl Error {
    pub fn io<S: AsRef<str>>(msg: S) -> Error
    where
        String: From<S>,
    {
        Self::IO(String::from(msg))
    }

    pub fn addr<S: AsRef<str>>(msg: S) -> Self
    where
        String: From<S>,
    {
        Self::Net(String::from(msg))
    }

    pub fn from_io_error(e: std::io::Error) -> Self {
        Self::IO(e.to_string())
    }

    pub fn from_addr_parse_error(e: core::net::AddrParseError) -> Self {
        Self::Net(format!("parse error: {e}"))
    }
}

impl core::error::Error for Error {}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::IO(msg) => write!(f, "[IO Error] {msg}"),
            Error::Net(msg) => write!(f, "[Net Error] {msg}"),
        }
    }
}
