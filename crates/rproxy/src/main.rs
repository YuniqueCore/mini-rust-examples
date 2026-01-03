fn main() {
    smol::block_on(async {
        let _ = rpoxy::run().await.inspect_err(|e| eprintln!("{e}"));
    });
}
