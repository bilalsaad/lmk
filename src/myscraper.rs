use itertools::Itertools;
use opentelemetry::global;
use opentelemetry::trace::Span;
use opentelemetry::trace::Tracer;
use opentelemetry::Context;
use opentelemetry::KeyValue;
use scraper::Html;
use scraper::Selector;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::Write as OtherWrite;
use std::fs::OpenOptions;
use std::io::Write;
use std::sync::mpsc;
use std::thread;
use std::time::UNIX_EPOCH;

use crate::db::Db;
use crate::scoped_timer::ScopedTimer;

#[derive(Serialize, Deserialize, PartialEq, Debug, Default)]
pub struct Target {
    // The uri the scraper should scrape.
    pub uri: String,
    // The text to search in the html content of `uri`.
    pub text: String,
    // Description of what the target is, only for humans.
    #[serde(default)]
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
    // Strings written to this channel will get written to log_file.
    log_writer: Option<mpsc::Sender<String>>,
    // thread that listens on the receiving and writes to the log_file.
    writer_thread: Option<thread::JoinHandle<()>>,
}

impl Metrics {
    // TODO(bilal): See if you can make this configurable.
    const FILE_PATH: &str = "scraper-metrics.csv";
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Metrics {
            log_writer: Some(sender),
            writer_thread: Some(thread::spawn(move || {
                log::info!(
                    "Starting metrics writing thread, writing to {}...",
                    Metrics::FILE_PATH
                );
                let mut buffer: Vec<u8> = vec![];
                let write_buffer = |buffer: &mut Vec<u8>| {
                    log::info!("flushing buffer to file.. writing {} bytes", buffer.len());
                    match OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(Metrics::FILE_PATH)
                    {
                        Ok(mut f) => match f.write_all(buffer) {
                            Ok(_) => (),
                            Err(e) => log::warn!("failed to write to metrics file: {}", e),
                        },
                        Err(e) => log::warn!("failed to open metrics file: {}", e),
                    };
                    buffer.clear()
                };
                for entry in receiver {
                    log::info!("metrics-writer: Writing entry {}", entry);
                    _ = writeln!(&mut buffer, "{}", entry);
                    if buffer.len() > 256 {
                        write_buffer(&mut buffer);
                    }
                }
                if buffer.len() > 0 {
                    write_buffer(&mut buffer);
                }
                log::info!("finished metrics writer thread...");
            })),
        }
    }

    #[cfg(test)]
    fn new_in_memory() -> Self {
        let (sender, receiver) = mpsc::channel();
        Metrics {
            log_writer: Some(sender),
            writer_thread: Some(thread::spawn(move || {
                eprintln!("Starting in memory metrics writing thread, writing to memory");
                for entry in receiver {
                    eprintln!("metrics-writer-in-memory: Writing entry {}", entry);
                }
                eprintln!("finished metrics writer thread...");
            })),
        }
    }

    // Writes <timestamp>,inc_req,<target>,<status> to the log file.
    //
    // -timestmap is seconds since unix epoch
    fn increment_num_requests(&self, target: &str, status: &str) {
        let _timer = ScopedTimer::new("increment_num_requests".into());
        let now = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if let Err(e) = self
            .log_writer
            .as_ref()
            .unwrap()
            .send(format!("{:?},inc_req,{},{}", now, target, status))
        {
            log::warn!("failed to write to log sink... {}", e);
        }
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        log::info!("droping metrics...");
        // Drop the sending channel, this will cause the log sink thread to stop.
        drop(self.log_writer.take());
        self.writer_thread.take().map(thread::JoinHandle::join);
    }
}

pub struct Scraper<'a, S> {
    // The targets to scrape.
    targets: Vec<Target>,
    // Used to send notifications.
    sender: &'a S,
    // Metrics related to scraping.
    metrics: Metrics,
    // Cache of Scraper::target_id(target) -> matching results.
    target_cache: std::cell::RefCell<Db>,
}

// ThreadMessage is an enum sent from the threads we spawn to do the requests.
enum ThreadMessage {
    Ok(String),
    Err(String),
}

impl<'a, S> Scraper<'a, S>
where
    S: Sender,
{
    pub fn new(targets: Vec<Target>, sender: &'a S) -> Scraper<'a, S> {
        let metrics = Metrics::new();
        let db_path = "./.scraper_target_cache.db";
        let target_cache = std::cell::RefCell::new(Db::new(&db_path).unwrap());
        Scraper {
            targets,
            sender,
            metrics,
            target_cache,
        }
    }

    #[cfg(test)]
    fn new_in_memory(targets: Vec<Target>, sender: &'a S) -> Scraper<'a, S> {
        let metrics = Metrics::new_in_memory();
        let target_cache = std::cell::RefCell::new(Db::new_in_memory().unwrap());
        Scraper {
            targets,
            sender,
            metrics,
            target_cache,
        }
    }

    // scrape runs a single scraping iteration, reporting any matches on targets to sender.
    pub fn scrape(&self) -> Result<(), Box<dyn std::error::Error>> {
        let tracer = global::tracer("scraper");
        let _child_span = tracer.start("scraper.scrape");
        let _scrape_timer = ScopedTimer::new("scrape timer".into());
        let (sender, receiver) = mpsc::channel();

        // Spawn a scoped thread per target and do the http request in the thread.
        // the html pages are returned via a `ThreadMessage, target` pair over a channel.
        // Notes:
        // - A scoped thread was needed due to lifetime constraints (otherwise the lifetime of self
        // would need to be 'static'.
        // - A threadpool would be better here, a thread per target could be costly. so a future
        // improvement would be to do this in a thread pool or via async things.
        thread::scope(|s| {
            let mut handles = vec![];
            for t in &self.targets {
                let sender = sender.clone();
                let current_context = Context::current();
                handles.push(s.spawn(move || {
                    let tracer = global::tracer("scraper");
                    let mut child_span = tracer
                        .start_with_context(format!("scrape_thread: {}", t.uri), &current_context);
                    child_span.set_attribute(KeyValue::new("target", t.uri.clone()));
                    let _timer = ScopedTimer::new(format!("scrape for {}", t.uri));
                    match reqwest::blocking::get(&t.uri).map(|x| x.text()) {
                        Ok(Ok(x)) => {
                            // TODO(bilal): Instead of converting from a string,
                            // get the response and add more intereseintg things to the span
                            let resp_size = i64::try_from(x.len()).unwrap_or(i64::max_value());
                            child_span.add_event(
                                "http-response",
                                vec![
                                    KeyValue::new("resp_size", resp_size),
                                    KeyValue::new("uri", t.uri.clone()),
                                    KeyValue::new("status", "ok"),
                                ],
                            );
                            let _ = sender.send((t, ThreadMessage::Ok(x)));
                        }
                        Ok(Err(e)) | Err(e) => {
                            let status =
                                e.status().map_or("unknown".to_string(), |s| s.to_string());
                            child_span.add_event(
                                "http-response",
                                vec![
                                    KeyValue::new("err text", e.to_string()),
                                    KeyValue::new("uri", t.uri.clone()),
                                    KeyValue::new("status", status.clone()),
                                ],
                            );
                            let _ = sender.send((t, ThreadMessage::Err(status)));
                            log::warn!("failed to scrape {:?}, err: {:?}", t.uri, e);
                        }
                    };
                }));
            }
            // We need to drop the sender before waiting on the receiver because after
            // all of the threads join the original sender is still alive and the receiver
            // won't stop until all senders are dropped. So we explicitly drop the sender
            // I imagine there's a more idomatic way to do this.
            drop(sender);
            for (t, resp) in receiver {
                match resp {
                    ThreadMessage::Ok(resp) => {
                        let page = {
                            let _timer = ScopedTimer::new(format!("parse_docucment({})", t.uri));
                            Html::parse_document(&resp)
                        };
                        self.handle_page_content(page, t)?;
                        self.metrics.increment_num_requests(&t.uri, "OK");
                    }
                    ThreadMessage::Err(e) => {
                        self.metrics.increment_num_requests(&t.uri, &e);
                    }
                };
            }

            for handle in handles {
                handle.join().unwrap();
            }
            Ok(())
        })
    }

    fn target_id(target: &Target) -> String {
        std::format!("{}:{}", target.uri, target.text)
    }

    // Checks content for any matches. For each encountered match a notification event is generated.
    // Note that if content has not changed since last handling, no notifcations are generated.
    fn handle_page_content(
        &self,
        page: Html,
        target: &Target,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _content_timer = ScopedTimer::new(format!("handle_page_content({})", target.uri));
        let selector = Selector::parse("*").unwrap();
        let content = page.select(&selector).flat_map(|x| x.text());
        let cache_id = Self::target_id(target);
        let old_contents = self
            .target_cache
            .borrow()
            .get(&cache_id)
            .unwrap_or("".into());
        let old_matches: HashSet<_> = old_contents.lines().collect();

        // cache_value will hold the up to date matching content for target.uri.
        let mut cache_value = String::new();
        {
            let _timer = ScopedTimer::new(format!("lookup and compare for {}", target.uri));
            // Look up old content and compare
            content
                .filter_map(|x| {
                    // Get the elements that match `target.text`
                    if x.contains(&target.text) {
                        Some(x)
                    } else {
                        None
                    }
                })
                // Dedup them
                .unique()
                .map(|x| {
                    // Write the matches into target_caches
                    // writing into a string can't fail.
                    writeln!(cache_value, "{}", x).unwrap();
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
        }
        if let Err(e) = self.target_cache.borrow_mut().put(&cache_id, &cache_value) {
            log::warn!("failed to write into target_cache: {}", e);
        }
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
        // todo- figure out a better way to create the dummy scraper
        let sender = FakeSender::new();
        let scraper = Scraper::new_in_memory(vec![], &sender);
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

        // run again after deleting the cache , should have another match.
        let target_id = Scraper::<FakeSender>::target_id(&target);
        scraper.target_cache.borrow_mut().put(&target_id, "")?;
        scraper.handle_page_content(html.clone(), &target)?;
        assert_eq!(sender.msgs.borrow().len(), 4);
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
        let scraper = Scraper::new_in_memory(vec![], &sender);
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
        let scraper = Scraper::new_in_memory(vec![target1, target2, target3], &sender);

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
        let scraper = Scraper::new_in_memory(vec![target], &sender);

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
