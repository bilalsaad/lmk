use crate::myscraper::Target;
use crate::telegramsender::TelegramSender;

use clap::Parser;
use myscraper::PrintSender;
use scoped_timer::ScopedTimer;

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

mod db;
mod myscraper;
mod telegramsender;
mod scoped_timer;

// TODO: this is unused because I couldn't figure out how to make the reporting flag turn into a nenum.
#[derive(PartialEq, Debug)]
pub enum Reporting {
    // Use a telegramsender::TelegramSender to report matches.
    // The telegram_chat_id defines which chat to use, note that this
    // requires that the telegram token is in scope.
    Telegram,
    // Just print matches to stdout
    Print,
}

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Type of reporting app should do:
    ///  "print" -> just print results
    ///  "telegram" -> use telegram chat (requires telegram_chat_id being set)
    #[arg(short, long)]
    reporting: String,

    /// Telegram Chat ID
    /// Defaults to bilal's bot.
    #[arg(short, long, default_value_t = -727046961)]
    telegram_chat_id: i64,
}

fn read_targets<P: AsRef<Path>>(path: P) -> Result<Vec<Target>, Box<dyn std::error::Error>> {
    // Open the file in read-only mode with buffer.
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let targets = serde_yaml::from_reader(reader)?;

    Ok(targets)
}

const TARGETS_PATH: &str = "targets.yaml";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();


    let targets = read_targets(TARGETS_PATH)?;
    match args.reporting.as_str() {
        "print" => {
            let _timer = ScopedTimer::new("print scrape time".into());
            let sender = PrintSender {};
            let s = myscraper::Scraper::new(targets, &sender);
            s.scrape()
        }
        "telegram" => {
            let _timer = ScopedTimer::new("telegram scrape time".into());
            let sender = TelegramSender::new(args.telegram_chat_id).unwrap();
            let s = myscraper::Scraper::new(targets, &sender);
            s.scrape()
        }
        // TODO(bilal): return an actual error here..
        _ => todo!(
            "Unsupported flag value for reporting {}, only 'print|telegram' supported.",
            args.reporting
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialze_targets() -> Result<(), Box<dyn std::error::Error>> {
        read_targets(TARGETS_PATH).map(|_| ())
    }
}
