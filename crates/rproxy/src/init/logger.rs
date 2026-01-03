use std::str::FromStr;

use anyhow::Result;
use fern::colors::{Color, ColoredLevelConfig};
pub fn init(log_level: &str, colored: bool) -> Result<()> {
    let colors = ColoredLevelConfig::new()
        .trace(Color::Magenta)
        .debug(Color::Blue)
        .info(Color::Green)
        .warn(Color::Yellow)
        .error(Color::Red);

    fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                chrono::Utc::now().to_rfc3339(),
                if colored {
                    colors.color(record.level()).to_string()
                } else {
                    record.level().to_string()
                },
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::from_str(log_level)?)
        .chain(std::io::stdout())
        .chain(fern::log_file("output.log")?)
        .apply()?;
    Ok(())
}
