use scraper::{Html, Selector};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let resp = reqwest::blocking::get("https://www.brooklynmuseum.org/about/careers")?;
    let document = Html::parse_document(&resp.text().unwrap());
    let selector = Selector::parse(".job-postings li").unwrap();
    let sections: Vec<_> = document.select(&selector).collect();
    for sec in sections {
        for t in sec.text().filter(|x| x.contains("curator") || x.contains("Curator")) {
            println!("I see text: {:?}", t)
        }
    }
    Ok(())
}
