use crate::myscraper::{PrintSender, Target};

mod myscraper;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sender = PrintSender {};
    let s = myscraper::Scraper::new(
        vec![
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
        ],
        &sender,
    );
    s.start()
}
