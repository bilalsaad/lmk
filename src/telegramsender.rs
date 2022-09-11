use teloxide::prelude::*;
use tokio::runtime::Runtime;

use crate::myscraper::{Sender, Target};

// A Teloxide telegram bot sender. Requires that env variable of TELOXIDE_TOKEN 
// being set e.g, $ export TELOXIDE_TOKEN=<Your token here>
pub struct TelegramSender {
    // Telegram chat id that all messages are sent to, provided in `new` method.
    chat_id: ChatId,
    // A teloxide bot. Requires bot token being in environment.
    // $ export TELOXIDE_TOKEN=<Your token here>
    bot: Bot,
    // Used to wait on the futures returned by bot.send_message.
    rt: Runtime,
}

impl Sender for TelegramSender {
    fn send(&self, addr: &str, target: &Target, msg: String) {
        eprintln!("[to {}] Target {}. msg: \n {}", addr, target.uri, msg);
        if let Err(e) = self.rt.block_on(
            self.bot
                .send_message(self.chat_id, format!("{}: {}", target.uri, msg))
                .send(),
        ) {
            eprintln!("failed to send for target {:?}, err: {} ", target, e);
        }
    }
}

impl TelegramSender {
    // Creates a new Sender, chat_id is a telegram chat id, e.g., -727046961
    pub fn new(chat_id: i64) -> Result<Self, Box<dyn std::error::Error>> {
        let bot = Bot::from_env();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let chat_id = ChatId(chat_id);

        Ok(TelegramSender { chat_id, bot, rt })
    }
}
