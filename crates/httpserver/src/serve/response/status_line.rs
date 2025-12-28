use std::{str::FromStr};


#[derive(Debug)]
pub struct  StatusLine{
    method:String,
    route_path:String,
    protocl:String,
}

impl  FromStr for StatusLine {
    type Err= anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let splits:Vec<&str> = s.trim().splitn(3, ' ').collect();
        if splits.len() != 3 {
            return Err(anyhow::anyhow!("failed to parse the status line: {}",s))
        }

        Ok(Self { 
            method: splits[0].to_owned(),
            route_path: splits[1].to_owned(),
            protocl: splits[2].to_owned()
        })
    }
}