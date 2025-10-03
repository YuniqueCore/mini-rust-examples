use anyverr::AnyResult;
use ct_room::Config;

#[tokio::main]
async fn main() -> AnyResult<()> {
    let config = Config::default();
    ct_room::run(config).await?;
    Ok(())
}
