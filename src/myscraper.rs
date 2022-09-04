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
    // The text to search in the html content of `uri`.
    pub text: String,
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
        for t in &self.targets {
            let resp = match reqwest::blocking::get(&t.uri).map(|x| x.text()) {
                Ok(Ok(x)) => Html::parse_document(&x),
                Ok(Err(e)) | Err(e) => {
                    eprintln!("failed to scrape {:?}, err: {:?}", t.uri, e);
                    continue;
                }
            };
            handle_page_content(resp, &t)?;
        }
        Ok(())
    }
}

// Checks content for any matches. For each encountered match a notification event is generated.
// Note that if content has not changed since last handling, no notifcations are generated.
fn handle_page_content(page: Html, target: &Target) -> Result<(), Box<dyn std::error::Error>> {
    let selector = Selector::parse("*").unwrap();
    let content = page.select(&selector).flat_map(|x| x.text());
    // We create a file with the sname name as the uri, but with _ instead of //
    // this file serves as a cache of what the last time we ran this on this uri.
    let file = target.uri.replace("/", "_");
    let old_contents = match fs::read_to_string(&file) {
        Ok(x) => x,
        // if there isn't a file we just assume a clean slate of matches.
        _ => "".to_string(),
    };
    let old_matches: HashSet<_> = old_contents.lines().collect();

    // We open the file for writing so we can write the new state to the file.
    let mut file = match OpenOptions::new().append(true).create(true).open(&file) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("Failed to open cache file for {}, err: {}", &target.uri, e);
            None
        }
    };
    // Look up old content and compare
    content
        .filter_map(|x| {
            if x.contains(&target.text) {
                Some(x)
            } else {
                None
            }
        })
        .unique()
        .map(|x| {
            if let Some(ff) = &mut file {
                if let Err(e) = writeln!(ff, "{}", x) {
                    eprintln!(
                        "Failed to write match {} for target {}. err: {}",
                        x, &target.uri, e
                    );
                }
            }
            x
        })
        .filter(|x| !old_matches.contains(x))
        .for_each(|x| println!("found {}", x));

    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_handle_page_content() -> Result<(), Box<dyn std::error::Error>> {
        let target = Target {
            uri: "test_handle_page_content_uri".to_string(),
            text: "meow".to_string(),
        };
        let html = Html::parse_document(
            r#"
         <li> meow </li>
         <li> cactus </li>
        "#,
        );
        handle_page_content(html.clone(), &target)?;
        // TODO- don't write real files -- this cache should be an implementation detail
        fs::remove_file(&target.uri)?;
        // run again after deleting file should be okay
        handle_page_content(html.clone(), &target)?;
        fs::remove_file(&target.uri)?;
        Ok(())
    }

    #[test]
    fn test_handle_page_content_caches() -> Result<(), Box<dyn std::error::Error>> {
        let target = Target {
            uri: "test_handle_page_content_caches".to_string(),
            text: "meow".to_string(),
        };
        let html = Html::parse_document(
            r#"
         <li> meow </li>
         <li> cactus </li>
        "#,
        );
        handle_page_content(html.clone(), &target)?;
        // second call should have no matches.
        handle_page_content(html.clone(), &target)?;
        fs::remove_file(&target.uri)?;
        Ok(())
    }
}
