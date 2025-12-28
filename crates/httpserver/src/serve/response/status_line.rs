use anyhow::Result;
use std::str::FromStr;

#[derive(Debug)]
pub struct StatusLine {
    version: String,
    status: u16,
    reason: String,
}

impl StatusLine {
    pub fn new<S: Into<String>>(version: S, status: u16, reason: S) -> Self {
        Self {
            version: version.into(),
            status,
            reason: reason.into(),
        }
    }

    pub fn split(&self) -> (String, u16, String) {
        (self.version.clone(), self.status, self.reason.clone())
    }
}

impl FromStr for StatusLine {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let splits: Vec<&str> = s.trim().splitn(3, ' ').collect();
        if splits.len() != 3 {
            return Err(anyhow::anyhow!("failed to parse the status line: {}", s));
        }

        Ok(Self {
            version: splits[0].into(),
            status: splits[1].parse()?,
            reason: splits[2].to_owned(),
        })
    }
}

impl ToString for StatusLine {
    fn to_string(&self) -> String {
        format!("{} {} {}", self.version, self.status, self.reason)
    }
}
