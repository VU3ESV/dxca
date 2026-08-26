//! Telegram Bot API notifier — port of the Swift `TelegramNotifier`.
//! Blocking (ureq); callers run it on a blocking task. Failures are
//! returned, not panicked — alerting must never take the pipeline down.

#[derive(Debug, Clone)]
pub struct Telegram {
    /// API base, overridable for tests. Default `https://api.telegram.org`.
    pub base: String,
}

impl Default for Telegram {
    fn default() -> Self {
        Telegram {
            base: "https://api.telegram.org".into(),
        }
    }
}

impl Telegram {
    pub fn with_base(base: &str) -> Self {
        Telegram {
            base: base.trim_end_matches('/').into(),
        }
    }

    /// Send an HTML-formatted message (1.x parse_mode) to one chat.
    pub fn send(&self, bot_token: &str, chat_id: &str, html_text: &str) -> Result<(), String> {
        if bot_token.is_empty() || chat_id.is_empty() {
            return Err("bot token / chat id not configured".into());
        }
        let url = format!("{}/bot{}/sendMessage", self.base, bot_token);
        let body = serde_json::json!({
            "chat_id": chat_id,
            "text": html_text,
            "parse_mode": "HTML",
        });
        ureq::post(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send_json(body)
            .map_err(|e| format!("telegram send: {e}"))?;
        Ok(())
    }
}

/// The 1.x HTML escaper for message text.
pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    #[test]
    fn escaping() {
        assert_eq!(super::escape_html("a<b> & c"), "a&lt;b&gt; &amp; c");
    }
}
