use std::fmt::Display;


#[derive(Debug)]
pub enum ContentType {
    Html,
    Json,
    Plaintext,
    Other(String)
}

impl Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
      let v =  match self {
            ContentType::Json => "application/json",
            ContentType::Html | ContentType::Plaintext => "text/html; charset=utf-8",
            ContentType::Other(x) => x,
        };
        write!(f,"{}",v)
    }
}