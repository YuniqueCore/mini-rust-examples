fn main() {
    smol::block_on(async {
        let _ = httpserver::run().await.inspect_err(|e| eprintln!("{e}"));
    })
}
