mod config;
use std::{
    sync::Arc,
    time::{self, Duration},
};

use anyverr::{AnyError, AnyResult};
// Include this in wherever you need `AnyError`.
pub use config::*;
use reqwest::Client;
use tokio::{sync::Semaphore, task::JoinSet};

pub async fn run(config: Config) -> AnyResult<()> {
    let urls = config.urls;

    let concurancy = config.con.min(urls.len());
    let sem = Arc::new(Semaphore::new(concurancy));
    let client = reqwest::ClientBuilder::new()
        .timeout(Duration::from_millis(config.timeout))
        .no_proxy()
        .build()
        .map_err(|e| AnyError::wrap(e))?;

    let timer = time::Instant::now();
    let mut tasks = JoinSet::new();

    for url in urls {
        let premit = sem
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| AnyError::wrap(e))?;
        let client = client.clone();
        let permit_count = concurancy - sem.available_permits();
        tasks.spawn(async move {
            let _p = premit;
            tokio::time::sleep(Duration::from_millis(50)).await;
            println!("{}", permit_count);
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

    println!("elapsed: {}ms", timer.elapsed().as_millis());

    Ok(())
}

async fn req_text(client: Client, url: String) -> AnyResult<String> {
    let resp = client.get(url).send().await.map_err(|e| {
        AnyError::builder()
            .message(format!("get url: {}", e.to_string()))
            .build()
    })?;

    // 检查 HTTP 状态码，只有 200 状态码才会继续处理
    if !resp.status().is_success() {
        return Err(AnyError::builder()
            .message(format!("HTTP error: {} - {}", resp.status(), resp.url()))
            .build());
    }

    let text = resp.text().await.map_err(|e| {
        AnyError::builder()
            .message(format!("text: {}", e.to_string()))
            .build()
    })?;
    Ok(text)
}
