use crate::myscraper::Target;
use crate::telegramsender::TelegramSender;

use clap::Parser;
use myscraper::PrintSender;
use opentelemetry::sdk::export::trace::stdout;
use scoped_timer::ScopedTimer;

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

mod db;
mod myscraper;
mod scoped_timer;
mod telegramsender;

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

    /// Scraper Build ID -- git short commit ID of the version that this scraper ran as.
    /// useful for figuring out what version ran etc...
    #[arg(long)]
    build_id: Option<String>,

    /// If true, we use the default jaeger tracing, if false the otel traces are pretty printed to
    /// the stdout
    #[arg(long, default_value_t = false)]
    jaeger_tracing: bool,
}

fn read_targets<P: AsRef<Path>>(path: P) -> Result<Vec<Target>, Box<dyn std::error::Error>> {
    // Open the file in read-only mode with buffer.
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let targets = serde_yaml::from_reader(reader)?;

    Ok(targets)
}

const TARGETS_PATH: &str = "targets.yaml";

use opentelemetry::trace::{TraceContextExt, Tracer};
use opentelemetry::{global, KeyValue};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pretty_env_logger::init();
    let args = Args::parse();
    let build_id = args.build_id.unwrap_or("none".into());
    log::info!("starting build_id {}...", build_id);
    // jaeger tracing
    if args.jaeger_tracing {
        global::set_text_map_propagator(opentelemetry_jaeger::Propagator::new());
        let _tracer = opentelemetry_jaeger::new_agent_pipeline()
            .with_service_name("JobScraper")
            .install_simple()?;
    } else {
        let _tracer = stdout::new_pipeline()
            .with_pretty_print(true)
            .install_simple();
    }

    let tracer = global::tracer("scraper");

    tracer.in_span("scrape-main", |cx| {
        let targets = read_targets(TARGETS_PATH)?;
        cx.span().set_attribute(KeyValue::new("build_id", build_id));
        cx.span()
            .set_attribute(KeyValue::new("targets_path", TARGETS_PATH));
        cx.span()
            .set_attribute(KeyValue::new("scrape-type", args.reporting.clone()));
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
    })?;
    // Shutdown trace pipeline
    global::shutdown_tracer_provider();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialze_targets() -> Result<(), Box<dyn std::error::Error>> {
        read_targets(TARGETS_PATH).map(|_| ())
    }
}
