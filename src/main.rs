use crate::myscraper::Target;
use crate::telegramsender::TelegramSender;

use clap::Parser;
use myscraper::PrintSender;

mod myscraper;
mod telegramsender;

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let targets = vec![
        Target {
            uri: "https://www.brooklynmuseum.org/about/careers".to_string(),
            text: "Curator".to_string(),
        },
        Target {
            uri: "https://whitney.org/about/job-postings".to_string(),
            text: "Curator".to_string(),
        },
        // moma website isn't letting us scrape -- sad/
        //
        // Target {
        //    uri: "https://www.moma.org/about/careers/jobs".to_string(),
        //    text: "Moma".to_string(),
        //},
    ];
    match args.reporting.as_str() {
        "print" => {
            let sender = PrintSender {};
            let s = myscraper::Scraper::new(targets, &sender);
            s.scrape()
        }
        "telegram" => {
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
