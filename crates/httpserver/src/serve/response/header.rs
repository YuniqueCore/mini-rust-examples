use std::str::FromStr;

#[derive(Debug)]
pub struct Header{
    key:String,
    value:String
}

impl FromStr for Header {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let splits:Vec<&str> = s.trim().splitn(2, ':').collect();
        if splits.len() != 2{
            return Err(anyhow::anyhow!("failed to parse the header line: {}",s))
        }

        Ok(Self { 
            key:splits[0].to_owned(),
            value:splits[0].to_owned(),
        })
    }
}