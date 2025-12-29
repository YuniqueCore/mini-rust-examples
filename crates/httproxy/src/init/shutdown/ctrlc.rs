use anyhow::Result;

pub fn init() -> Result<ctrlc2::AsyncCtrlC> {
    let ctrlc = ctrlc2::AsyncCtrlC::new(move || {
        println!("Ctrl-C received! Ready to exiting...");
        true
    })
    .expect("should install async ctrl c signal but failed");
    Ok(ctrlc)
}
