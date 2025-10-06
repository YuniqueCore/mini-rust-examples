use anyverr::AnyResult;
use udp_echo::Config;

#[tokio::main]
async fn main() -> AnyResult<()> {
    let config = Config::default();
    udp_echo::run(config).await?;
    Ok(())
}
