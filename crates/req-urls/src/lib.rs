mod config;
mod err;
use std::{sync::Arc, time::Duration};

// Include this in wherever you need `AnyError`.
pub use config::*;
pub use err::*;
use reqwest::Client;
use tokio::{sync::Semaphore, task::JoinSet};

pub async fn run(config: Config) -> AnyResult<()> {
    let urls = config.urls;

    let concurancy = config.con.min(urls.len());
    let sem = Arc::new(Semaphore::new(concurancy));
    let client = reqwest::ClientBuilder::new()
        .timeout(Duration::from_millis(config.timeout))
        .build()
        .map_err(|e| AnyError::wrap(e))?;

    let mut tasks = JoinSet::new();

    for url in urls {
        let premit = sem
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| AnyError::wrap(e))?;
        let client = client.clone();
        tasks.spawn(async move {
            let _p = premit;
            req_text(client, url).await
        });
    }

    while let Some(res) = tasks.join_next().await {
        match res {
            Ok(Ok(data)) => {
                println!("{}", data.len());
            }
            Ok(Err(e)) => println!("Ok::Err: {e}"),
            Err(e) => println!("Err::Err: {e}"),
        }
    }

    Ok(())
}

async fn req_text(client: Client, url: String) -> AnyResult<String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| AnyError::builder().message(e.to_string()).build())?;
    let text = resp
        .text()
        .await
        .map_err(|e| AnyError::builder().message(e.to_string()).build())?;
    Ok(text)
}
