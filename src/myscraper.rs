use itertools::Itertools;
use scraper::Html;
use scraper::Selector;

pub struct Target {
    // The uri the scraper should scrape. Note that this serves as the ID of thes
    pub uri: String,
    pub matcher: Matcher,
    // todo: something notifier thing.
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
            handle_page_content(resp.select(&selector).flat_map(|x| x.text()), &t.matcher)?;
        }
        Ok(())
    }
}

// Checks content for any matches. For each encountered match a notification event is generated.
// Note that if content has not changed since last handling, no notifcations are generated.
fn handle_page_content<'a, I>(
    content: I,
    matcher: &Matcher,
) -> Result<(), Box<dyn std::error::Error>>
where
    I: Iterator<Item = &'a str>,
{
    // Look up old content and compare
    content
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
        .unique()
        .for_each(|x| {
            if let Matcher::TextMatch(_, f) = &matcher {
                f(x);
            }
        });
    // Look over all text in content and look for matches. Generate match notifications for any
    // matches.
    Ok(())
}
