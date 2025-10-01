use req_urls::{AnyResult, Config};

#[tokio::main]
async fn main() -> AnyResult<()> {
    let c = Config::load("./test.json")?;
    req_urls::run(c).await
}
