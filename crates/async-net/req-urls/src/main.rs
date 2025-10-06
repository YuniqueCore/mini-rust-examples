use anyverr::AnyResult;
use req_urls::Config;

#[tokio::main]
async fn main() -> AnyResult<()> {
    let c = Config::load("./test/req_urls.json")?;
    req_urls::run(c).await
}
