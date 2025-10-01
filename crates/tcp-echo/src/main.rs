use anyverr::AnyResult;
use tcp_echo::Config;

#[tokio::main]
async fn main() -> AnyResult<()> {
    let config = Config::default();
    tcp_echo::run(config).await?;
    Ok(())
}
