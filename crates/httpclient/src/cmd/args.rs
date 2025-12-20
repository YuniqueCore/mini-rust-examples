use std::ops::{Deref, DerefMut};

use sarge::{ArgParseError, ArgumentType};

#[derive(Debug, Clone)]
pub struct HeadersArg(pub Vec<String>);

impl ArgumentType for HeadersArg {
    type Error = ArgParseError;

    fn from_value(val: Option<&str>) -> sarge::ArgResult<Self> {
        Some(Ok(Self(val?.split(';').map(|s| s.to_string()).collect())))
    }
}

impl Deref for HeadersArg {
    type Target = Vec<String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HeadersArg {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
