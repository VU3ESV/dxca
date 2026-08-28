//! Per-node cluster-command router — [`docs/TELNET-INTERACTIVE.md`] §3.3.
//!
//! The DX-cluster protocol has no request IDs: a reply to `sh/dx` arrives as
//! a burst of ordinary lines, indistinguishable from the node's ambient
//! chatter. With several telnet sessions connected there is nothing in the
//! bytes saying whose reply is whose. This router solves that by
//! **serializing**: each node has one command in flight at a time, and every
//! non-spot line from that node is routed to the session that asked, until
//! the response window closes. The next queued command then starts.
//!
//! Commands are issued at human pace, so serialization costs nothing real,
//! and it is the only scheme that cannot leak one operator's results to
//! another (see the alternatives table in the design doc).
//!
//! Pure state machine: no sockets, no clock, no I/O. Every entry point takes
//! `now_ms` and returns the [`RouterAction`]s the caller should perform, so
//! the whole thing is testable by feeding it events.
//!
//! **Milliseconds, not the seconds used elsewhere in this crate.** The quiet
//! window is ~2 s; at one-second granularity a line at t=10.9 recorded as 10
//! would let a tick at 12.0 close a window that had been quiet for only 1.1 s,
//! truncating slow replies. Sub-second precision is the point here.

use dxca_connect::dxcluster::ClientEvent;
use std::collections::{HashMap, VecDeque};

/// Identifies a telnet session. Opaque; the telnet layer assigns them.
pub type SessionId = u64;

/// Quiet period after the last response line before the window closes.
pub const QUIET_MS: u64 = 2_000;
/// Hard cap on a response window, however chatty the node is.
pub const TIMEOUT_MS: u64 = 15_000;

/// What the caller should do. The router never performs I/O itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouterAction {
    /// Write this command line to the node's cluster session.
    ToNode { node: String, line: String },
    /// Deliver this text to one telnet session.
    ToSession { session: SessionId, text: String },
}

/// One command waiting for its turn on a node.
#[derive(Clone, Debug)]
struct Pending {
    session: SessionId,
    command: String,
}

/// The command currently occupying a node's single slot.
#[derive(Clone, Debug)]
struct InFlight {
    session: SessionId,
    command: String,
    /// When the command was written — drives [`TIMEOUT_MS`].
    started_ms: u64,
    /// When the last response line arrived — drives [`QUIET_MS`]. Seeded
    /// with `started_ms` so a command that draws no reply at all still
    /// closes on the quiet path rather than waiting out the hard timeout.
    last_line_ms: u64,
    /// Lines routed so far, for the timeout message.
    lines: usize,
}

#[derive(Default)]
struct NodeSlot {
    inflight: Option<InFlight>,
    queue: VecDeque<Pending>,
}

/// Serializes commands per node and routes each reply to its requester.
#[derive(Default)]
pub struct CommandRouter {
    nodes: HashMap<String, NodeSlot>,
}

impl CommandRouter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Queue `command` for `node` on behalf of `session`.
    ///
    /// The caller is responsible for checking the node is live and the
    /// command is permitted — by the time it reaches here it is going to be
    /// sent. Returns the write, or a queued-position notice when the node's
    /// slot is busy (an honest wait beats silence).
    pub fn submit(
        &mut self,
        node: &str,
        session: SessionId,
        command: String,
        now_ms: u64,
    ) -> Vec<RouterAction> {
        let slot = self.nodes.entry(node.to_string()).or_default();
        if slot.inflight.is_some() {
            slot.queue.push_back(Pending {
                session,
                command,
            });
            let ahead = slot.queue.len();
            return vec![RouterAction::ToSession {
                session,
                text: format!(
                    "Queued behind {ahead} command{} on {node}…",
                    if ahead == 1 { "" } else { "s" }
                ),
            }];
        }
        vec![dispatch(
            slot,
            node,
            Pending { session, command },
            now_ms,
        )]
    }

    /// Offer one event from `node`'s cluster session to the router.
    ///
    /// Returns the actions to perform and whether the router **consumed**
    /// the event. A consumed line belongs to somebody's reply and must not
    /// also flow onward — that is what keeps `sh/dx` output, which parses as
    /// perfectly good spots, out of the live spot pipeline.
    pub fn on_event(&mut self, node: &str, event: &ClientEvent, now_ms: u64) -> (Vec<RouterAction>, bool) {
        let Some(slot) = self.nodes.get_mut(node) else {
            return (Vec::new(), false);
        };
        let Some(f) = slot.inflight.as_mut() else {
            return (Vec::new(), false); // ambient chatter — not ours
        };
        match event {
            // The completion marker. Close the window, start the next one.
            ClientEvent::Prompt(_) => {
                let mut actions = Vec::new();
                slot.inflight = None;
                if let Some(next) = slot.queue.pop_front() {
                    actions.push(dispatch(slot, node, next, now_ms));
                }
                (actions, true)
            }
            ClientEvent::Line(text)
            | ClientEvent::Announce(text)
            | ClientEvent::Wwv(text) => {
                f.last_line_ms = now_ms;
                f.lines += 1;
                (
                    vec![RouterAction::ToSession {
                        session: f.session,
                        text: text.clone(),
                    }],
                    true,
                )
            }
            // A `sh/dx` reply arrives as parsed spots. They are historical —
            // often hours old — so they go to the requester and NOT into the
            // pipeline, which would otherwise re-announce them as fresh and
            // fire Telegram alerts for last Tuesday's QSOs.
            ClientEvent::Spot { raw, .. } => {
                f.last_line_ms = now_ms;
                f.lines += 1;
                (
                    vec![RouterAction::ToSession {
                        session: f.session,
                        text: raw.clone(),
                    }],
                    true,
                )
            }
            // Session-level events are never part of a command reply.
            ClientEvent::Connected
            | ClientEvent::LoggedIn
            | ClientEvent::Proven
            | ClientEvent::Disconnected { .. } => (Vec::new(), false),
        }
    }

    /// Drive the timers. Call periodically; cheap when nothing is in flight.
    pub fn on_tick(&mut self, now_ms: u64) -> Vec<RouterAction> {
        let mut actions = Vec::new();
        for (node, slot) in self.nodes.iter_mut() {
            let Some(f) = slot.inflight.as_ref() else {
                continue;
            };
            let timed_out = now_ms.saturating_sub(f.started_ms) >= TIMEOUT_MS;
            let quiet = now_ms.saturating_sub(f.last_line_ms) >= QUIET_MS;
            if !timed_out && !quiet {
                continue;
            }
            if timed_out {
                // Honest-status rule: say the reply may be short, never let
                // the operator assume they saw all of it.
                actions.push(RouterAction::ToSession {
                    session: f.session,
                    text: format!(
                        "— `{}` on {} timed out after {} line{}; the reply may be incomplete",
                        f.command,
                        node,
                        f.lines,
                        if f.lines == 1 { "" } else { "s" }
                    ),
                });
            }
            slot.inflight = None;
            if let Some(next) = slot.queue.pop_front() {
                actions.push(dispatch(slot, node, next, now_ms));
            }
        }
        actions
    }

    /// The node's session dropped. Anything outstanding for it is lost —
    /// tell the waiting operators rather than leaving them hanging.
    pub fn on_node_down(&mut self, node: &str, reason: &str) -> Vec<RouterAction> {
        let Some(slot) = self.nodes.get_mut(node) else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        let waiting: Vec<SessionId> = slot
            .inflight
            .take()
            .map(|f| f.session)
            .into_iter()
            .chain(slot.queue.drain(..).map(|p| p.session))
            .collect();
        for session in waiting {
            actions.push(RouterAction::ToSession {
                session,
                text: format!("— {node} disconnected ({reason}); command abandoned"),
            });
        }
        actions
    }

    /// A telnet session went away. Drop its queued commands; a reply already
    /// in flight keeps its slot (the node is going to send it regardless)
    /// but is routed nowhere.
    pub fn on_session_gone(&mut self, session: SessionId) {
        for slot in self.nodes.values_mut() {
            slot.queue.retain(|p| p.session != session);
        }
    }

    /// Commands queued for `node`, excluding the one in flight (tests/status).
    pub fn queue_len(&self, node: &str) -> usize {
        self.nodes.get(node).map_or(0, |s| s.queue.len())
    }

    /// Is a command currently occupying `node`'s slot?
    pub fn is_busy(&self, node: &str) -> bool {
        self.nodes
            .get(node)
            .is_some_and(|s| s.inflight.is_some())
    }
}

/// Put `pending` in the node's slot and produce the write.
fn dispatch(slot: &mut NodeSlot, node: &str, pending: Pending, now_ms: u64) -> RouterAction {
    let line = pending.command.clone();
    slot.inflight = Some(InFlight {
        session: pending.session,
        command: pending.command,
        started_ms: now_ms,
        last_line_ms: now_ms,
        lines: 0,
    });
    RouterAction::ToNode {
        node: node.to_string(),
        line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(s: &str) -> ClientEvent {
        ClientEvent::Line(s.to_string())
    }

    fn prompt() -> ClientEvent {
        ClientEvent::Prompt("DB0SUE de VU2CPL >".into())
    }

    const A: SessionId = 1;
    const B: SessionId = 2;

    #[test]
    fn command_is_written_and_its_reply_reaches_the_requester() {
        let mut r = CommandRouter::new();
        let out = r.submit("DB0SUE", A, "sh/dx".into(), 0);
        assert_eq!(
            out,
            vec![RouterAction::ToNode {
                node: "DB0SUE".into(),
                line: "sh/dx".into()
            }]
        );
        assert!(r.is_busy("DB0SUE"));

        let (out, consumed) = r.on_event("DB0SUE", &line("first result"), 100);
        assert!(consumed, "a reply line belongs to the command, not the feed");
        assert_eq!(
            out,
            vec![RouterAction::ToSession {
                session: A,
                text: "first result".into()
            }]
        );

        let (out, consumed) = r.on_event("DB0SUE", &prompt(), 200);
        assert!(consumed);
        assert!(out.is_empty(), "nothing queued behind it");
        assert!(!r.is_busy("DB0SUE"), "prompt closes the window");
    }

    #[test]
    fn second_command_waits_then_runs_when_the_first_finishes() {
        let mut r = CommandRouter::new();
        r.submit("DB0SUE", A, "sh/dx".into(), 0);

        let out = r.submit("DB0SUE", B, "sh/wwv".into(), 10);
        assert_eq!(
            out,
            vec![RouterAction::ToSession {
                session: B,
                text: "Queued behind 1 command on DB0SUE…".into()
            }],
            "B is told it is waiting, not left in silence"
        );
        assert_eq!(r.queue_len("DB0SUE"), 1);

        // A's reply must not leak to B.
        let (out, _) = r.on_event("DB0SUE", &line("A's data"), 20);
        assert_eq!(
            out,
            vec![RouterAction::ToSession {
                session: A,
                text: "A's data".into()
            }]
        );

        let (out, _) = r.on_event("DB0SUE", &prompt(), 30);
        assert_eq!(
            out,
            vec![RouterAction::ToNode {
                node: "DB0SUE".into(),
                line: "sh/wwv".into()
            }],
            "B's command starts as soon as the slot frees"
        );
        assert_eq!(r.queue_len("DB0SUE"), 0);

        let (out, _) = r.on_event("DB0SUE", &line("B's data"), 40);
        assert_eq!(
            out,
            vec![RouterAction::ToSession {
                session: B,
                text: "B's data".into()
            }],
            "and now the lines belong to B"
        );
    }

    #[test]
    fn a_long_reply_is_not_truncated_by_the_quiet_timer() {
        let mut r = CommandRouter::new();
        r.submit("DB0SUE", A, "sh/dx 50".into(), 0);
        // Lines every 1.5 s for 9 s — never 2 s quiet, so the window holds
        // even though it far outlives QUIET_MS.
        let mut t = 0;
        for _ in 0..6 {
            t += 1_500;
            let (out, _) = r.on_event("DB0SUE", &line("row"), t);
            assert_eq!(out.len(), 1);
            assert!(r.on_tick(t).is_empty(), "still streaming at {t}ms");
            assert!(r.is_busy("DB0SUE"));
        }
        // Then it goes quiet.
        assert!(r.on_tick(t + QUIET_MS).is_empty());
        assert!(!r.is_busy("DB0SUE"), "quiet period closes it");
    }

    #[test]
    fn silent_command_closes_on_quiet_without_waiting_out_the_hard_timeout() {
        let mut r = CommandRouter::new();
        r.submit("DB0SUE", A, "sh/nothing".into(), 0);
        assert!(r.on_tick(QUIET_MS - 1).is_empty());
        let out = r.on_tick(QUIET_MS);
        assert!(out.is_empty(), "no reply, nothing to say, nothing queued");
        assert!(!r.is_busy("DB0SUE"));
    }

    #[test]
    fn endless_output_hits_the_hard_timeout_and_the_session_is_told() {
        let mut r = CommandRouter::new();
        r.submit("DB0SUE", A, "sh/dx 9999".into(), 0);
        // A line every second forever: the quiet timer never fires.
        let mut t = 0;
        while t < TIMEOUT_MS {
            t += 1_000;
            r.on_event("DB0SUE", &line("row"), t);
        }
        let out = r.on_tick(t);
        assert_eq!(out.len(), 1);
        match &out[0] {
            RouterAction::ToSession { session, text } => {
                assert_eq!(*session, A);
                assert!(text.contains("timed out"), "got: {text}");
                assert!(text.contains("may be incomplete"), "got: {text}");
            }
            other => panic!("expected a warning to the session, got {other:?}"),
        }
        assert!(!r.is_busy("DB0SUE"));
    }

    #[test]
    fn sh_dx_results_are_captured_and_never_reach_the_pipeline() {
        // The trap from the design doc: sh/dx output parses as spots, but
        // they are historical. Consumed => the caller must not forward them.
        let mut r = CommandRouter::new();
        r.submit("DB0SUE", A, "sh/dx".into(), 0);
        // A real historical spot line, through the real parser.
        let raw = "DX de W3LPL:     14074.0  K1JT           FT8 -10 dB                  1428Z";
        let spot = ClientEvent::Spot {
            spot: dxca_connect::dxcluster::wire::parse_spot_line(raw).expect("fixture parses"),
            raw: raw.to_string(),
        };
        let (out, consumed) = r.on_event("DB0SUE", &spot, 50);
        assert!(consumed, "MUST be consumed — else stale spots hit the feed");
        assert_eq!(
            out,
            vec![RouterAction::ToSession {
                session: A,
                text: raw.to_string()
            }]
        );
    }

    #[test]
    fn ambient_lines_pass_through_untouched_when_nothing_is_in_flight() {
        let mut r = CommandRouter::new();
        // Never submitted anything to this node.
        let (out, consumed) = r.on_event("N2WQ", &line("To ALL de N2WQ: hi"), 0);
        assert!(out.is_empty());
        assert!(!consumed, "the normal feed must keep flowing");

        // Even on a known node, once its window has closed.
        r.submit("DB0SUE", A, "sh/dx".into(), 0);
        r.on_event("DB0SUE", &prompt(), 10);
        let (out, consumed) = r.on_event("DB0SUE", &line("ambient"), 20);
        assert!(out.is_empty());
        assert!(!consumed);
    }

    #[test]
    fn nodes_do_not_block_each_other() {
        let mut r = CommandRouter::new();
        r.submit("DB0SUE", A, "sh/dx".into(), 0);
        let out = r.submit("N2WQ", B, "sh/wwv".into(), 0);
        assert_eq!(
            out,
            vec![RouterAction::ToNode {
                node: "N2WQ".into(),
                line: "sh/wwv".into()
            }],
            "a busy node must not stall a different one"
        );
        assert!(r.is_busy("DB0SUE") && r.is_busy("N2WQ"));

        // And their replies do not cross.
        let (out, _) = r.on_event("N2WQ", &line("wwv data"), 10);
        assert_eq!(
            out,
            vec![RouterAction::ToSession {
                session: B,
                text: "wwv data".into()
            }]
        );
    }

    #[test]
    fn node_disconnect_abandons_in_flight_and_queued_commands() {
        let mut r = CommandRouter::new();
        r.submit("DB0SUE", A, "sh/dx".into(), 0);
        r.submit("DB0SUE", B, "sh/wwv".into(), 10);

        let out = r.on_node_down("DB0SUE", "connection reset");
        assert_eq!(out.len(), 2, "both the runner and the waiter are told");
        for action in &out {
            match action {
                RouterAction::ToSession { text, .. } => {
                    assert!(text.contains("abandoned"), "got: {text}");
                    assert!(text.contains("connection reset"), "got: {text}");
                }
                other => panic!("expected session notices, got {other:?}"),
            }
        }
        assert!(!r.is_busy("DB0SUE"));
        assert_eq!(r.queue_len("DB0SUE"), 0);
    }

    #[test]
    fn a_departed_session_drops_its_queue_without_disturbing_others() {
        let mut r = CommandRouter::new();
        r.submit("DB0SUE", A, "sh/dx".into(), 0);
        r.submit("DB0SUE", B, "sh/wwv".into(), 10);
        r.submit("DB0SUE", A, "sh/muf".into(), 20);
        assert_eq!(r.queue_len("DB0SUE"), 2);

        r.on_session_gone(A);
        assert_eq!(r.queue_len("DB0SUE"), 1, "only A's queued command goes");

        // B's still runs when the slot frees.
        let (out, _) = r.on_event("DB0SUE", &prompt(), 30);
        assert_eq!(
            out,
            vec![RouterAction::ToNode {
                node: "DB0SUE".into(),
                line: "sh/wwv".into()
            }]
        );
    }
}
