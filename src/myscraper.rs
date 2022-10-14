use itertools::Itertools;
use scraper::Html;
use scraper::Selector;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::fs::OpenOptions;
use std::io::prelude::*;

#[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
pub struct Target {
    // The uri the scraper should scrape. Note that this serves as the ID of thes
    pub uri: String,
    // The text to search in the html content of `uri`.
    pub text: String,
    // Description of what the target is, only for humans.
    pub description: String,
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

// Writes <timestamp, target, ...> metrics.
// Metrics are appendded to scraper-metrics.csv
struct Metrics {
    // Path to log file. useful for overriding in tests.
    log_file: &'static str,
}

impl Metrics {
    const FILE_PATH: &str = "scraper-metrics.csv";
    fn new() -> Self {
        Metrics {
            log_file: Metrics::FILE_PATH,
        }
    }

    fn increment_num_requests(&self, target: &str, status: &str) {
        let now = std::time::SystemTime::now();
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_file)
        {
            Ok(mut f) => match writeln!(f, "{:?},inc_req,{},{}", now, target, status) {
                Ok(_) => (),
                Err(e) => eprintln!("failed to write to metrics file: {}", e),
            },
            Err(e) => eprintln!("failed to open metrics file: {}", e),
        }
    }
}

pub struct Scraper<'a, S> {
    targets: Vec<Target>,
    sender: &'a S,
    metrics: Metrics,
}

impl<'a, S> Scraper<'a, S>
where
    S: Sender,
{
    pub fn new(targets: Vec<Target>, sender: &'a S) -> Scraper<'a, S> {
        let metrics = Metrics::new();
        Scraper {
            targets,
            sender,
            metrics,
        }
    }

    // scrape runs a single scraping iteration, reporting any matches on targets to sender.
    pub fn scrape(&self) -> Result<(), Box<dyn std::error::Error>> {
        // make async
        for t in &self.targets {
            let resp = match reqwest::blocking::get(&t.uri).map(|x| x.text()) {
                Ok(Ok(x)) => {
                    self.metrics.increment_num_requests(&t.uri, "OK");
                    Html::parse_document(&x)
                }
                Ok(Err(e)) | Err(e) => {
                    let status = e.status().map_or("unknown".to_string(), |s| s.to_string());
                    self.metrics.increment_num_requests(&t.uri, &status);
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
        let mut file = match OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&file)
        {
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
    use httptest::cycle;
    use httptest::{matchers::request, responders::status_code, Expectation};
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
            ..Default::default()
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
            ..Default::default()
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

    #[test]
    fn test_real_http_server() -> Result<(), Box<dyn std::error::Error>> {
        let server = httptest::Server::run();

        server.expect(
            Expectation::matching(request::method_path("GET", "/target1"))
                .times(..) // any number.
                .respond_with(status_code(200).body("meow-meow")),
        );
        server.expect(
            Expectation::matching(request::method_path("GET", "/target2"))
                .times(..)
                .respond_with(status_code(200).body("cat-meow")),
        );
        server.expect(
            Expectation::matching(request::method_path("GET", "/i-don't-exist"))
                .times(..)
                .respond_with(status_code(404)),
        );

        let target1 = Target {
            uri: server.url_str("/target1"),
            text: "meow".to_string(),
            ..Default::default()
        };
        let target2 = Target {
            uri: server.url_str("/target2"),
            text: "cat".to_string(),
            ..Default::default()
        };
        let target3 = Target {
            uri: server.url_str("/i-don't-exist"),
            text: "cactus".to_string(),
            ..Default::default()
        };
        let sender = FakeSender::new();
        let scraper = Scraper::new(vec![target1, target2, target3], &sender);

        scraper.scrape()?;
        // We should have match for target1 and target2.
        assert_eq!(sender.msgs.borrow().len(), 2);
        // Expect one match for target1 and one match for target 2
        assert_eq!(
            sender
                .msgs
                .borrow()
                .iter()
                .filter(|x| x.contains("target1"))
                .count(),
            1
        );
        assert_eq!(
            sender
                .msgs
                .borrow()
                .iter()
                .filter(|x| x.contains("target2"))
                .count(),
            1
        );

        // Run another iteration and expect no messages.
        scraper.scrape()?;

        Ok(())
    }

    #[test]
    fn test_real_http_server_content_change() -> Result<(), Box<dyn std::error::Error>> {
        let server = httptest::Server::run();

        server.expect(
            Expectation::matching(request::method_path("GET", "/target"))
                .times(..) // any number.
                .respond_with(cycle![
                    status_code(200).body("meow-meow"),
                    status_code(200).body("new meow who dis")
                ]),
        );

        let target = Target {
            uri: server.url_str("/target"),
            text: "meow".to_string(),
            ..Default::default()
        };
        let sender = FakeSender::new();
        let scraper = Scraper::new(vec![target], &sender);

        scraper.scrape()?;
        // We should have match for target.
        assert_eq!(sender.msgs.borrow().len(), 1);
        // Expect one match for target1 and one match for target 2
        assert!(sender.msgs.borrow()[0].contains("meow-meow"));

        // Run another iteration and expect another match
        scraper.scrape()?;
        assert_eq!(sender.msgs.borrow().len(), 2);
        // Expect one match for target1 and one match for target 2
        assert!(sender.msgs.borrow()[1].contains("new meow who dis"));

        scraper.scrape()?;
        assert_eq!(sender.msgs.borrow().len(), 3);
        // Expect one match for target1 and one match for target 2
        assert!(
            sender.msgs.borrow()[2].contains("meow-meow"),
            "got {} want {}",
            sender.msgs.borrow()[2],
            "meow-meow"
        );

        Ok(())
    }

    #[test]
    fn test_serialize_deserialize_target() -> Result<(), Box<dyn std::error::Error>> {
        let t = Target {
            uri: "a".to_string(),
            text: "b".to_string(),
            ..Default::default()
        };
        let serialized = serde_yaml::to_string(&t).unwrap();

        assert!(serialized.contains("uri: a"));
        assert!(serialized.contains("text: b"));

        let deserialized: Target = serde_yaml::from_str(&serialized)?;
        assert_eq!(t, deserialized);

        Ok(())
    }

    #[test]
    fn test_serialize_deserialize_multiple_targets() -> Result<(), Box<dyn std::error::Error>> {
        let t = vec![
            Target {
                uri: "a".to_string(),
                text: "b".to_string(),
                ..Default::default()
            },
            Target {
                uri: "a".to_string(),
                text: "b".to_string(),
                ..Default::default()
            },
            Target {
                uri: "c".to_string(),
                text: "d".to_string(),
                ..Default::default()
            },
        ];
        let serialized = serde_yaml::to_string(&t).unwrap();

        assert!(serialized.contains("uri: a"));
        assert!(serialized.contains("text: b"));
        assert!(serialized.contains("uri: c"));
        assert!(serialized.contains("text: d"));

        let deserialized: Vec<Target> = serde_yaml::from_str(&serialized)?;
        assert_eq!(deserialized.len(), 3);

        Ok(())
    }
}
