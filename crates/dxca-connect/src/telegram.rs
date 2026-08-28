//! Telegram Bot API notifier — port of the Swift `TelegramNotifier`.
//! Blocking (ureq); callers run it on a blocking task. Failures are
//! returned, not panicked — alerting must never take the pipeline down.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Telegram {
    /// API base, overridable for tests. Default `https://api.telegram.org`.
    pub base: String,
    /// Pause before the single transport-error retry; zeroed in tests.
    pub retry_delay: Duration,
}

impl Default for Telegram {
    fn default() -> Self {
        Telegram {
            base: "https://api.telegram.org".into(),
            retry_delay: Duration::from_secs(2),
        }
    }
}

impl Telegram {
    pub fn with_base(base: &str) -> Self {
        Telegram {
            base: base.trim_end_matches('/').into(),
            ..Telegram::default()
        }
    }

    /// Send an HTML-formatted message (1.x parse_mode) to one chat.
    ///
    /// A transport failure (connect/TLS trouble, response timeout) gets one
    /// retry after `retry_delay` — on a flaky uplink such failures are
    /// momentary and the second attempt usually lands. An HTTP rejection
    /// (bad token, unknown chat) returns at once: Telegram would only
    /// refuse it again. Callers run this on a blocking task, so the pause
    /// never stalls the pipeline.
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
        let attempt = || {
            ureq::post(&url)
                .timeout(Duration::from_secs(10))
                .send_json(&body)
        };
        match attempt() {
            Ok(_) => Ok(()),
            Err(e @ ureq::Error::Status(..)) => Err(format!("telegram send: {e}")),
            Err(first) => {
                std::thread::sleep(self.retry_delay);
                attempt()
                    .map(|_| ())
                    .map_err(|e| format!("telegram send: {e} (retried; first attempt: {first})"))
            }
        }
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn escaping() {
        assert_eq!(super::escape_html("a<b> & c"), "a&lt;b&gt; &amp; c");
    }

    /// One-shot HTTP stub: each accepted connection is answered by the
    /// per-request closure (given the 1-based request number). Returning
    /// `None` drops the connection unanswered — a transport error to ureq.
    fn spawn_stub(
        respond: impl Fn(usize) -> Option<&'static str> + Send + 'static,
    ) -> (u16, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_srv = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let n = hits_srv.fetch_add(1, Ordering::SeqCst) + 1;
                // Read the full request (headers, then Content-Length body)
                // before replying — answering early makes the client's body
                // write fail, which reads as a transport error, not HTTP.
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                let body_len = loop {
                    let Ok(k) = stream.read(&mut buf) else { break 0 };
                    if k == 0 {
                        break 0;
                    }
                    req.extend_from_slice(&buf[..k]);
                    if let Some(head_end) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&req[..head_end]).to_lowercase();
                        let want: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap_or(0);
                        break want.saturating_sub(req.len() - head_end - 4);
                    }
                };
                let mut remaining = body_len;
                while remaining > 0 {
                    let Ok(k) = stream.read(&mut buf) else { break };
                    if k == 0 {
                        break;
                    }
                    remaining = remaining.saturating_sub(k);
                }
                if let Some(reply) = respond(n) {
                    let _ = stream.write_all(reply.as_bytes());
                }
            }
        });
        (port, hits)
    }

    #[test]
    fn transport_error_is_retried_once() {
        let ok = "HTTP/1.1 200 OK\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}";
        let (port, hits) = spawn_stub(move |n| (n > 1).then_some(ok));
        let mut t = super::Telegram::with_base(&format!("http://127.0.0.1:{port}"));
        t.retry_delay = super::Duration::ZERO;
        t.send("tok", "chat", "hello").expect("second attempt lands");
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn http_rejection_is_not_retried() {
        let bad = "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (port, hits) = spawn_stub(move |_| Some(bad));
        let mut t = super::Telegram::with_base(&format!("http://127.0.0.1:{port}"));
        t.retry_delay = super::Duration::ZERO;
        let err = t.send("tok", "chat", "hello").unwrap_err();
        assert!(err.contains("telegram send:"), "got: {err}");
        assert_eq!(hits.load(Ordering::SeqCst), 1, "a 4xx must not retry");
    }
}
