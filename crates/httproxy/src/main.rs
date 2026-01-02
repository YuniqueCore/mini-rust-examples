fn main() {
    smol::block_on(async {
        let _ = httproxy::run().await.inspect_err(|e| eprintln!("{e}"));
    });
}
