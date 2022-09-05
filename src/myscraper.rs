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

// Sender sends messages to the given addr.
// User can provide implementations that email, log or print matches.
pub trait Sender {
    fn send(&self, addr: &str, target: &Target, msg: String);
}

/// Sender implementation that just calls println with arguments.
pub struct PrintSender {}

impl Sender for PrintSender {
    fn send(&self, addr: &str, t: &Target, msg: String) {
        println!("[to {}] Target {}. msg: \n {}", addr, t.uri, msg);
    }
}

pub struct Scraper<'a, S> {
    targets: Vec<Target>,
    sender: &'a S,
}

impl<'a, S> Scraper<'a, S>
where
    S: Sender,
{
    pub fn new(targets: Vec<Target>, sender: &'a S) -> Scraper<'a, S> {
        Scraper { targets, sender }
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
            self.handle_page_content(resp, &t)?;
        }
        Ok(())
    }

    // Checks content for any matches. For each encountered match a notification event is generated.
    // Note that if content has not changed since last handling, no notifcations are generated.
    fn handle_page_content(
        &self,
        page: Html,
        target: &Target,
    ) -> Result<(), Box<dyn std::error::Error>> {
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
            .for_each(|x| {
                self.sender.send(
                    "everyone@everyone.com",
                    &target,
                    format!("Found match: {}", x),
                )
            });
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use std::cell::RefCell;

    use super::*;

    struct FakeSender {
        // messages sent to this fake sender
        msgs: RefCell<Vec<String>>,
    }
    impl FakeSender {
        fn new() -> Self {
            FakeSender {
                msgs: RefCell::new(vec![]),
            }
        }
    }
    impl Sender for FakeSender {
        fn send(&self, addr: &str, t: &Target, msg: String) {
            self.msgs
                .borrow_mut()
                .push(format!("[to {}] Target {}. msg: \n {}", addr, t.uri, msg));
        }
    }

    #[test]
    fn test_handle_page_content() -> Result<(), Box<dyn std::error::Error>> {
        let target = Target {
            uri: "test_handle_page_content_uri".to_string(),
            text: "meow".to_string(),
        };
        // TODO- don't write real files -- this cache should be an implementation detail
        let cache_file = target.uri.clone();
        // make sure we're running fresh without a leftover cached file
        let _ = fs::remove_file(&cache_file);
        // todo- figure out a better way to create the dummy scraper
        let sender = FakeSender::new();
        let scraper = Scraper::new(vec![], &sender);
        let html = Html::parse_document(
            r#"
            <html>
         <li> meow </li>
         <li> cactus </li>
         <li> meow mathew </li>
            </html>
        "#,
        );
        // The first scrape should give us one matching meow.
        scraper.handle_page_content(html.clone(), &target)?;
        assert_eq!(sender.msgs.borrow().len(), 2);

        fs::remove_file(&cache_file)?;
        // run again after deleting file, should have another match.
        scraper.handle_page_content(html.clone(), &target)?;
        assert_eq!(sender.msgs.borrow().len(), 4);
        fs::remove_file(&cache_file)?;
        Ok(())
    }

    #[test]
    fn test_handle_page_content_caches() -> Result<(), Box<dyn std::error::Error>> {
        let target = Target {
            uri: "test_handle_page_content_caches".to_string(),
            text: "meow".to_string(),
        };
        let sender = FakeSender::new();
        let scraper = Scraper::new(vec![], &sender);
        let html = Html::parse_document(
            r#"
         <li> meow </li>
         <li> cactus </li>
        "#,
        );
        scraper.handle_page_content(html.clone(), &target)?;
        // One message for the meow.
        assert_eq!(sender.msgs.borrow().len(), 1);
        // let's update the html to include a new element. A message should only be added for the
        // new one.
        let html = Html::parse_document(
            r#"
         <li> meow </li>
         <li> cactus </li>
         <li> another meow!!!! </li>
        "#,
        );
        scraper.handle_page_content(html.clone(), &target)?;
        // Only an additional message should be appended.
        assert_eq!(sender.msgs.borrow().len(), 2);
        // New message should be different than the first.
        assert_ne!(sender.msgs.borrow()[0], sender.msgs.borrow()[1]);
        fs::remove_file(&target.uri)?;
        Ok(())
    }
}
