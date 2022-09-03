use itertools::Itertools;
use scraper::Html;
use scraper::Selector;
use std::collections::HashSet;
use std::fs;
use std::fs::OpenOptions;
use std::io::prelude::*;

pub struct Target {
    // The uri the scraper should scrape. Note that this serves as the ID of thes
    pub uri: String,
    pub matcher: Matcher,
}

pub enum Matcher {
    AnyChange,
    TextMatch(String, Box<dyn Fn(&str) -> ()>),
}

pub struct Scraper {
    targets: Vec<Target>,
}

impl Scraper {
    pub fn new(targets: Vec<Target>) -> Scraper {
        Scraper { targets }
    }

    pub fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        // make async
        let selector = Selector::parse("*").unwrap();
        for t in &self.targets {
            let resp = match reqwest::blocking::get(&t.uri).map(|x| x.text()) {
                Ok(Ok(x)) => Html::parse_document(&x),
                Ok(Err(e)) | Err(e) => {
                    eprintln!("failed to scrape {:?}, err: {:?}", t.uri, e);
                    continue;
                }
            };
            handle_page_content(resp.select(&selector).flat_map(|x| x.text()), &t)?;
        }
        Ok(())
    }
}

// Checks content for any matches. For each encountered match a notification event is generated.
// Note that if content has not changed since last handling, no notifcations are generated.
fn handle_page_content<'a, I>(content: I, target: &Target) -> Result<(), Box<dyn std::error::Error>>
where
    I: Iterator<Item = &'a str>,
{
    let file = target.uri.replace("/", "_");
    let matcher = &target.matcher;
    let old_contents = match fs::read_to_string(&file) {
        Ok(x) => x,
        _ => "".to_string(),
    };
    eprintln!("old contents: {}", old_contents);
    let old_matches: HashSet<_> = old_contents.lines().collect();

    // Look up old content and compare
    let matches = content
        .filter_map(|x| {
            // custom matcher(s) for document id
            match &matcher {
                Matcher::TextMatch(match_text, _) => {
                    if x.contains(match_text) {
                        Some(x)
                    } else {
                        None
                    }
                }
                Matcher::AnyChange => {
                    // TODO: look up old version and compare
                    None
                }
            }
        })
        .unique();

    // Writes matches to the file
    let mut file = match OpenOptions::new().append(true).create(true).open(&file) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("Failed to open cache file for {}, err: {}", &target.uri, e);
            None
        }
    };

    // Invoke callback on new matches only.
    matches.filter(|x| !old_matches.contains(x)).for_each(|x| {
        if let Matcher::TextMatch(_, f) = &matcher {
            f(x);
            if let Some(ff) = &mut file {
                // todo reduce nesting
                if let Err(e) = writeln!(ff, "{}", x) {
                    eprintln!(
                        "Failed to write match {} for target {}. err: {}",
                        x, &target.uri, e
                    );
                }
            }
        }
    });
    // Look over all text in content and look for matches. Generate match notifications for any
    // matches.
    Ok(())
}
