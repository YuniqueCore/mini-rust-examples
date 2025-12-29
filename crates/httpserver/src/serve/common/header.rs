use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct Header {
    pub key: String,
    pub value: String,
}

impl Header {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }

    pub fn key_eq_ignore_ascii_case(&self, other: &str) -> bool {
        self.key.eq_ignore_ascii_case(other)
    }
}

impl FromStr for Header {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        let (key, value) = trimmed
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("failed to parse the header line: {}", s))?;

        Ok(Self {
            key: key.trim().to_owned(),
            value: value.trim().to_owned(),
        })
    }
}

impl std::fmt::Display for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}\r\n", self.key, self.value)
    }
}
