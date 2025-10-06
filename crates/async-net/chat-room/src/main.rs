use anyverr::AnyResult;
use chat_room::Config;

#[tokio::main]
async fn main() -> AnyResult<()> {
    let config = Config::default();
    chat_room::run(config).await?;
    Ok(())
}
