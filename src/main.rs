use crate::myscraper::{Matcher, Target};

mod myscraper;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let s = myscraper::Scraper::new(vec![
        Target {
            uri: "https://www.brooklynmuseum.org/about/careers".to_string(),
            matcher: Matcher::TextMatch("urator".to_string()),
        },
        Target {
            uri: "poop".to_string(),
            matcher: Matcher::AnyChange,
        },
    ]);
    s.start()
}
