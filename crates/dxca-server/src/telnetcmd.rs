//! The join between a telnet session and the cluster nodes —
//! [`docs/TELNET-INTERACTIVE.md`] milestone 3.
//!
//! Everything hard already lives elsewhere: [`crate::commands`] decides what
//! a line is allowed to be, [`crate::cmdrouter`] decides whose reply is
//! whose, and [`crate::nodes`] does the writing. This module holds the
//! per-session state those three need between them — which node the operator
//! is talking to, and where to send the answers.

use crate::cmdrouter::{CommandRouter, RouterAction};
use crate::commands::{Classified, classify};
use crate::nodes::{NodeEventFilter, NodeManager};
use dxca_connect::dxcluster::ClientEvent;
use dxca_connect::telnet::{CommandSink, SessionId, TelnetIdentity};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

/// How often the router's timers are driven.
const TICK_MS: u64 = 250;

struct Session {
    tx: UnboundedSender<String>,
    identity: TelnetIdentity,
    /// The node commands go to. `None` until one is picked or defaulted.
    node: Option<String>,
}

pub struct TelnetCommands {
    nodes: Arc<NodeManager>,
    router: Mutex<CommandRouter>,
    sessions: Mutex<HashMap<SessionId, Session>>,
}

impl TelnetCommands {
    /// Wire it up and start the timer task.
    ///
    /// The caller must also hand the returned value to
    /// [`NodeManager::set_event_filter`] — without that the router never
    /// sees node replies, and `SHOW/DX` output would reach the spot feed.
    pub fn start(nodes: Arc<NodeManager>) -> Arc<Self> {
        let this = Arc::new(TelnetCommands {
            nodes,
            router: Mutex::new(CommandRouter::new()),
            sessions: Mutex::new(HashMap::new()),
        });
        let ticker = this.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(std::time::Duration::from_millis(TICK_MS));
            loop {
                iv.tick().await;
                let actions = {
                    let mut r = ticker.router.lock().unwrap();
                    r.on_tick(now_ms())
                };
                ticker.dispatch(actions);
            }
        });
        this
    }

    /// Perform what the router asked for. Never called with the router lock
    /// held: writing to a node and to a session can both take time, and
    /// holding the lock across them would serialize every other session.
    fn dispatch(&self, actions: Vec<RouterAction>) {
        for action in actions {
            match action {
                RouterAction::ToNode { node, line } => {
                    if !self.nodes.send_line(&node, &line) {
                        // Configured away mid-flight. Nobody is left to
                        // answer, so free the slot rather than let the
                        // command sit until it times out.
                        let actions = {
                            let mut r = self.router.lock().unwrap();
                            r.on_node_down(&node, "node no longer configured")
                        };
                        self.dispatch(actions);
                    }
                }
                RouterAction::ToSession { session, text } => self.send(session, text),
            }
        }
    }

    fn send(&self, session: SessionId, text: String) {
        if let Some(s) = self.sessions.lock().unwrap().get(&session) {
            self.send_to(&s.tx, text);
        }
    }

    /// Send when the session's channel is already to hand — avoids taking
    /// the sessions lock twice, and avoids re-entering it while held.
    fn send_to(&self, tx: &UnboundedSender<String>, text: String) {
        // The receiver is gone when the socket closed a moment ago; that is
        // ordinary, not an error.
        let _ = tx.send(text);
    }

    /// The node this session talks to, defaulting to the first live one so
    /// a `SH/DX` works without ceremony on a single-node setup.
    fn node_for(&self, session: SessionId) -> Result<String, String> {
        let mut sessions = self.sessions.lock().unwrap();
        let Some(s) = sessions.get_mut(&session) else {
            return Err("session is gone".into());
        };
        if let Some(node) = &s.node {
            return Ok(node.clone());
        }
        let statuses = self.nodes.statuses();
        let mut live: Vec<&String> = statuses
            .iter()
            .filter(|(_, st)| st.proven)
            .map(|(name, _)| name)
            .collect();
        live.sort();
        match live.first() {
            Some(name) => {
                s.node = Some((*name).clone());
                Ok((*name).clone())
            }
            None => Err("no cluster node is live; SH/NODES shows the states".into()),
        }
    }

    fn local(&self, session: SessionId, canonical: &str, args: &str) {
        match canonical {
            "HELP" => {
                for line in HELP_TEXT.lines() {
                    self.send(session, line.to_string());
                }
            }
            "SHOW/NODES" => {
                let statuses = self.nodes.statuses();
                let current = self
                    .sessions
                    .lock()
                    .unwrap()
                    .get(&session)
                    .and_then(|s| s.node.clone());
                let mut names: Vec<&String> = statuses.keys().collect();
                names.sort();
                if names.is_empty() {
                    self.send(session, "No cluster nodes are configured.".into());
                }
                for name in names {
                    let st = &statuses[name];
                    let mark = if current.as_deref() == Some(name.as_str()) {
                        "*"
                    } else {
                        " "
                    };
                    self.send(
                        session,
                        format!("{mark} {name:<12} {:<24} spots {}", st.state, st.spot_count),
                    );
                }
            }
            "SET/NODE" => {
                let want = args.trim().to_string();
                if want.is_empty() {
                    self.send(
                        session,
                        "Usage: SET/NODE <name>  (SH/NODES lists them)".into(),
                    );
                    return;
                }
                let statuses = self.nodes.statuses();
                // Case-insensitive, because node names are shouted in the
                // logs and typed in lower case by everyone.
                let found = statuses
                    .keys()
                    .find(|n| n.eq_ignore_ascii_case(&want))
                    .cloned();
                match found {
                    Some(name) => {
                        if let Some(s) = self.sessions.lock().unwrap().get_mut(&session) {
                            s.node = Some(name.clone());
                        }
                        let state = statuses[&name].state.clone();
                        self.send(session, format!("Commands now go to {name} ({state})."));
                    }
                    None => self.send(session, format!("{want}: no such node. Try SH/NODES.")),
                }
            }
            "SHOW/DXCA" => {
                let statuses = self.nodes.statuses();
                let live = statuses.values().filter(|s| s.proven).count();
                let spots: u64 = statuses.values().map(|s| s.spot_count).sum();
                self.send(
                    session,
                    format!(
                        "DXCA {} — {} of {} nodes live, {spots} cluster spots this run",
                        env!("CARGO_PKG_VERSION"),
                        live,
                        statuses.len()
                    ),
                );
                // Who the node sees is not who is typing: commands go out
                // under the shack's `login_call`, whoever issued them. Say
                // so, so nobody assumes their own callsign is on the wire.
                if let Some(s) = self.sessions.lock().unwrap().get(&session) {
                    self.send_to(
                        &s.tx,
                        format!(
                            "Logged in as {} ({}); commands reach nodes as this server's login.",
                            s.identity.callsign, s.identity.role
                        ),
                    );
                }
            }
            "BYE" | "QUIT" => {
                // The telnet layer intercepts a literal BYE and hangs up;
                // an abbreviation reaches here instead, so say so rather
                // than leaving the operator wondering why nothing happened.
                self.send(session, "Type BYE in full to disconnect.".into());
            }
            other => self.send(session, format!("{other}: not implemented here.")),
        }
    }
}

impl CommandSink for TelnetCommands {
    fn open(&self, session: SessionId, identity: &TelnetIdentity) -> UnboundedReceiver<String> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        // Insert replaces any previous registration for this id — a session
        // that logs in twice gets one entry, not two.
        self.sessions.lock().unwrap().insert(
            session,
            Session {
                tx,
                identity: identity.clone(),
                node: None,
            },
        );
        rx
    }

    fn submit(&self, session: SessionId, line: &str) {
        match classify(line) {
            Classified::Local { canonical, args } => self.local(session, &canonical, &args),
            Classified::Refused { canonical, why } => {
                // Name what the operator typed, not the canonical form:
                // being told `SET/PASSWORD` is refused when you typed
                // `s/pass` should still show you what it expanded to.
                let verb = line.split_whitespace().next().unwrap_or(line);
                let mut msg = why.message(verb);
                if let Some(c) = canonical
                    && !verb.eq_ignore_ascii_case(&c)
                {
                    msg.push_str(&format!(" (read as {c})"));
                }
                self.send(session, msg);
            }
            Classified::Forward { line, .. } => {
                let node = match self.node_for(session) {
                    Ok(n) => n,
                    Err(e) => return self.send(session, e),
                };
                // Refuse rather than queue against a node that cannot
                // answer — an honest "it is down" beats a silent wait.
                let status = self.nodes.statuses().get(&node).cloned();
                match status {
                    Some(st) if st.proven => {}
                    Some(st) => {
                        return self.send(
                            session,
                            format!(
                                "{node} is {}; pick another with SET/NODE or wait.",
                                st.state
                            ),
                        );
                    }
                    None => return self.send(session, format!("{node}: no such node.")),
                }
                let actions = {
                    let mut r = self.router.lock().unwrap();
                    r.submit(&node, session, line, now_ms())
                };
                self.dispatch(actions);
            }
        }
    }

    fn close(&self, session: SessionId) {
        self.sessions.lock().unwrap().remove(&session);
        self.router.lock().unwrap().on_session_gone(session);
    }
}

impl NodeEventFilter for TelnetCommands {
    fn intercept(&self, node: &str, event: &ClientEvent) -> bool {
        // A disconnect is never "consumed" — the node manager still has to
        // see it to update status — but the router must learn about it so
        // whoever was waiting is told instead of timing out.
        if let ClientEvent::Disconnected { reason } = event {
            let actions = {
                let mut r = self.router.lock().unwrap();
                r.on_node_down(node, reason)
            };
            self.dispatch(actions);
            return false;
        }
        let (actions, consumed) = {
            let mut r = self.router.lock().unwrap();
            r.on_event(node, event, now_ms())
        };
        self.dispatch(actions);
        consumed
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before 1970")
        .as_millis() as u64
}

const HELP_TEXT: &str = "\
DXCA telnet — cluster commands are passed to ONE node at a time.
  SH/NODES            list configured nodes; * marks yours
  SET/NODE <name>     send your commands to that node
  SH/DXCA             this server's status
  SH/DX, SH/WWV, ...  queries, forwarded to your node
  BYE                 log out and disconnect
Read-only queries only. Spotting and anything that changes a node account
is refused — DXCA shares one node session with every user.";

#[cfg(test)]
mod tests {
    use super::*;

    /// Every session state change goes through the sink, so the reply
    /// channel is the whole observable surface. These cover the parts that
    /// do not need a live node.
    async fn harness() -> (Arc<TelnetCommands>, UnboundedReceiver<String>) {
        let nodes = Arc::new(NodeManager::new());
        let cmds = TelnetCommands::start(nodes);
        let rx = cmds.open(
            1,
            &TelnetIdentity {
                user_id: 1,
                callsign: "VU2CPL".into(),
                role: "admin".into(),
            },
        );
        (cmds, rx)
    }

    fn drain(rx: &mut UnboundedReceiver<String>) -> Vec<String> {
        let mut out = Vec::new();
        while let Ok(line) = rx.try_recv() {
            out.push(line);
        }
        out
    }

    #[tokio::test]
    async fn a_refused_command_says_why_and_shows_what_it_expanded_to() {
        let (cmds, mut rx) = harness().await;
        cmds.submit(1, "s/pass hunter2");
        let out = drain(&mut rx).join(" ");
        assert!(out.contains("refused"), "got {out}");
        assert!(out.contains("SET/PASSWORD"), "expansion shown: {out}");
    }

    #[tokio::test]
    async fn an_unknown_command_is_refused_not_forwarded() {
        let (cmds, mut rx) = harness().await;
        cmds.submit(1, "frobnicate everything");
        let out = drain(&mut rx).join(" ");
        assert!(out.contains("not a command DXCA knows"), "got {out}");
    }

    #[tokio::test]
    async fn a_query_with_no_live_node_is_told_so_rather_than_queued() {
        let (cmds, mut rx) = harness().await;
        cmds.submit(1, "sh/dx 20");
        let out = drain(&mut rx).join(" ");
        assert!(out.contains("no cluster node is live"), "got {out}");
        assert!(
            !cmds.router.lock().unwrap().is_busy("anything"),
            "nothing should be queued"
        );
    }

    #[tokio::test]
    async fn set_node_rejects_a_name_that_does_not_exist() {
        let (cmds, mut rx) = harness().await;
        cmds.submit(1, "set/node NOSUCH");
        let out = drain(&mut rx).join(" ");
        assert!(out.contains("no such node"), "got {out}");
    }

    #[tokio::test]
    async fn help_is_answered_locally_and_mentions_the_refusal_policy() {
        let (cmds, mut rx) = harness().await;
        cmds.submit(1, "help");
        let out = drain(&mut rx).join("\n");
        assert!(out.contains("SET/NODE"), "got {out}");
        assert!(out.to_lowercase().contains("refused"), "got {out}");
    }

    #[tokio::test]
    async fn a_closed_session_stops_receiving() {
        let (cmds, mut rx) = harness().await;
        cmds.close(1);
        cmds.submit(1, "help");
        assert!(drain(&mut rx).is_empty(), "a gone session gets nothing");
    }
}
