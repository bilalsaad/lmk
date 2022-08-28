use itertools::Itertools;
use scraper::{Html, Selector};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resp = reqwest::blocking::get("https://www.brooklynmuseum.org/about/careers")?;
    let document = Html::parse_document(&resp.text().unwrap());
    let selector = Selector::parse("*").unwrap();
    let sections: Vec<_> = document.select(&selector).collect();
    for sec in sections {
        for t in sec
            .text()
            .filter_map(|x| {
                if x.contains("curator") || x.contains("Curator") {
                    return Some(x);
                }
                return None;
            })
            .unique()
        {
            println!("I see text: {:?}", t)
        }
    }
    Ok(())
}
