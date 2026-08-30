//! Feed-health alerts: Telegram when DXCA is running and **nothing is
//! reaching it**.
//!
//! ## What this can and cannot do, because the limit is the design
//!
//! A node cannot report its own death. If the Pi is off, or DXCA has
//! crashed, or the site's internet is down, nothing here runs and nothing is
//! sent — and since Telegram itself needs the internet, a connectivity
//! failure silences the alert about the connectivity failure. Covering
//! *that* needs an observer somewhere else, which is a different design
//! (and one deliberately not built: see HANDOVER).
//!
//! What is left is the class this module owns, and it is the one that
//! actually goes unnoticed for weeks: **DXCA alive and well, web GUI
//! answering, and the feed quietly dead** because the decoders were closed,
//! the radio was off, or a node dropped and never came back.
//!
//! ## Two conditions, both opt-in per account
//!
//! * **Feed quiet** — no spots at all for N minutes.
//! * **Node down** — one cluster node *disconnected* for N minutes.
//!
//! Node health is keyed on the connection, never on traffic. "Connected but
//! no spots" is a perfectly normal state — Hamalert and KST2Mac sit `Live`
//! with `spot_count: 0` for hours — so alerting on silence would cry wolf on
//! a healthy feed every quiet afternoon. A dropped connection is unambiguous.
//!
//! ## Edge-triggered, and recoveries are sent too
//!
//! One message when a condition starts, one when it clears. Not a repeat
//! every tick, which is how a monitor teaches its operator to ignore it —
//! and not silence on recovery either, or the only way to learn the feed is
//! back is to go and look, which is the thing the alert was supposed to save.
//!
//! State lives in this task, not the database. A restart forgets what it had
//! already reported, so a still-quiet feed is announced once more a threshold
//! later. That is the right way round: after a restart the operator wants to
//! know the state, not be spared a repeat.

use crate::db::NotifyUserConfig;
use crate::nodes::NodeManager;
use crate::pipeline::PipelineState;
use crate::users::UserService;
use dxca_connect::telegram::escape_html;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

/// Fine enough that a 5-minute threshold means roughly five minutes, coarse
/// enough to cost nothing: the whole check is two mutex reads and a little
/// arithmetic.
const TICK: Duration = Duration::from_secs(60);

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock before 1970")
        .as_secs() as i64
}

/// What has already been reported, so each condition speaks once.
#[derive(Default)]
struct Reported {
    /// Users currently told the feed is quiet.
    feed: HashSet<i64>,
    /// (user, node) pairs currently told that node is down.
    node: HashSet<(i64, String)>,
}

/// The spot clock: total spots seen, and when that total last moved.
///
/// A counter diff rather than a timestamp on the hot path. `process_spot`
/// runs for every decode on every band; this needs no part of it, and
/// "the total has not moved since T" is exactly the question being asked.
struct FeedClock {
    total: u64,
    changed_unix: i64,
}

impl FeedClock {
    /// Starts the clock at boot, so a server that comes up to a dead feed
    /// reports it a threshold later rather than immediately.
    fn new(total: u64, now: i64) -> FeedClock {
        FeedClock {
            total,
            changed_unix: now,
        }
    }

    /// Seconds since the last spot arrived.
    fn quiet_for(&mut self, total: u64, now: i64) -> i64 {
        if total != self.total {
            self.total = total;
            self.changed_unix = now;
        }
        now - self.changed_unix
    }
}

/// Every spot DXCA has taken in, from both kinds of source.
fn total_spots(pipeline: &PipelineState, nodes: &NodeManager) -> u64 {
    let udp: u64 = pipeline.source_counts.lock().unwrap().values().sum();
    let cluster: u64 = nodes.statuses().values().map(|s| s.spot_count).sum();
    udp + cluster
}

/// Spawn the watch. Silent unless an account asks for it.
pub fn spawn(users: Arc<UserService>, pipeline: Arc<PipelineState>, nodes: Arc<NodeManager>) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        // The first tick fires immediately; skip it so the clock starts from
        // a settled process rather than from mid-boot.
        ticker.tick().await;
        let mut clock = FeedClock::new(total_spots(&pipeline, &nodes), now_unix());
        let mut reported = Reported::default();
        // When each node was first seen disconnected. Absent = connected.
        let mut down_since: HashMap<String, i64> = HashMap::new();

        loop {
            ticker.tick().await;
            let now = now_unix();
            let quiet_for = clock.quiet_for(total_spots(&pipeline, &nodes), now);
            let statuses = nodes.statuses();

            for (name, st) in &statuses {
                if st.connected {
                    down_since.remove(name);
                } else {
                    down_since.entry(name.clone()).or_insert(now);
                }
            }
            // A node removed from the config stops existing, rather than
            // staying down forever in a map nobody prunes.
            down_since.retain(|name, _| statuses.contains_key(name));

            let mut outbox: Vec<(String, String, String)> = Vec::new();
            let Ok(all) = users.db.users() else { continue };
            for u in all {
                let Ok(cfg) = users.db.notify_config(u.id) else {
                    continue;
                };
                if !cfg.telegram_enabled
                    || cfg.telegram_bot_token.is_empty()
                    || cfg.telegram_chat_id.is_empty()
                {
                    continue;
                }
                collect_feed(&cfg, u.id, quiet_for, &mut reported, &mut outbox);
                collect_nodes(
                    &cfg,
                    u.id,
                    now,
                    &down_since,
                    &statuses,
                    &mut reported,
                    &mut outbox,
                );
            }

            if outbox.is_empty() {
                continue;
            }
            // `Telegram::send` is blocking ureq, and this task shares the
            // runtime with the spot pipeline.
            let telegram = users.telegram();
            tokio::task::spawn_blocking(move || {
                for (token, chat, text) in outbox {
                    if let Err(e) = telegram.send(&token, &chat, &text) {
                        eprintln!("dxca: health alert: {e}");
                    }
                }
            });
        }
    });
}

fn collect_feed(
    cfg: &NotifyUserConfig,
    user_id: i64,
    quiet_for: i64,
    reported: &mut Reported,
    outbox: &mut Vec<(String, String, String)>,
) {
    if cfg.notify_feed_quiet_minutes == 0 {
        // Switched off. Clear any standing report so turning it back on does
        // not immediately fire a stale recovery.
        reported.feed.remove(&user_id);
        return;
    }
    let threshold = cfg.notify_feed_quiet_minutes as i64 * 60;
    let already = reported.feed.contains(&user_id);
    let mins = quiet_for / 60;

    if quiet_for >= threshold && !already {
        reported.feed.insert(user_id);
        outbox.push((
            cfg.telegram_bot_token.clone(),
            cfg.telegram_chat_id.clone(),
            format!("<b>\u{26a0}\u{fe0f} DXCA: no spots for {mins} min</b>\nNothing has reached the aggregator from any source. Check the decoders and the cluster nodes."),
        ));
    } else if quiet_for < threshold && already {
        reported.feed.remove(&user_id);
        outbox.push((
            cfg.telegram_bot_token.clone(),
            cfg.telegram_chat_id.clone(),
            "<b>\u{2705} DXCA: spots are flowing again</b>".to_string(),
        ));
    }
}

fn collect_nodes(
    cfg: &NotifyUserConfig,
    user_id: i64,
    now: i64,
    down_since: &HashMap<String, i64>,
    statuses: &HashMap<String, crate::nodes::NodeStatus>,
    reported: &mut Reported,
    outbox: &mut Vec<(String, String, String)>,
) {
    if cfg.notify_node_down_minutes == 0 {
        reported.node.retain(|(uid, _)| *uid != user_id);
        return;
    }
    let threshold = cfg.notify_node_down_minutes as i64 * 60;

    for (name, since) in down_since {
        let key = (user_id, name.clone());
        let down_for = now - since;
        if down_for >= threshold && !reported.node.contains(&key) {
            reported.node.insert(key);
            let state = statuses
                .get(name)
                .map(|s| s.state.clone())
                .unwrap_or_default();
            outbox.push((
                cfg.telegram_bot_token.clone(),
                cfg.telegram_chat_id.clone(),
                format!(
                    "<b>\u{26a0}\u{fe0f} DXCA: node {} is down</b>\nDisconnected for {} min\u{2002}\u{b7}\u{2002}{}",
                    escape_html(name),
                    down_for / 60,
                    escape_html(&state),
                ),
            ));
        }
    }

    // Recovery: anything this user was told about that is no longer down.
    let recovered: Vec<String> = reported
        .node
        .iter()
        .filter(|(uid, name)| *uid == user_id && !down_since.contains_key(name))
        .map(|(_, name)| name.clone())
        .collect();
    for name in recovered {
        reported.node.remove(&(user_id, name.clone()));
        outbox.push((
            cfg.telegram_bot_token.clone(),
            cfg.telegram_chat_id.clone(),
            format!("<b>\u{2705} DXCA: node {} is back</b>", escape_html(&name)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(feed: u64, node: u64) -> NotifyUserConfig {
        NotifyUserConfig {
            telegram_enabled: true,
            telegram_bot_token: "t".into(),
            telegram_chat_id: "c".into(),
            notify_feed_quiet_minutes: feed,
            notify_node_down_minutes: node,
            ..Default::default()
        }
    }

    fn status(connected: bool, spots: u64) -> crate::nodes::NodeStatus {
        crate::nodes::NodeStatus {
            state: if connected { "Live" } else { "Reconnecting" }.into(),
            connected,
            proven: connected,
            spot_count: spots,
            last_spot_unix: None,
            attempt: 0,
        }
    }

    /// The clock measures "since the total last moved", which is what makes
    /// a counter diff a legitimate stand-in for a timestamp on the hot path.
    #[test]
    fn the_feed_clock_resets_when_any_spot_arrives() {
        let mut c = FeedClock::new(10, 1_000);
        assert_eq!(c.quiet_for(10, 1_300), 300, "nothing arrived");
        assert_eq!(c.quiet_for(11, 1_400), 0, "one spot resets it");
        assert_eq!(c.quiet_for(11, 1_700), 300, "and it runs again");
    }

    /// One message on the way in, one on the way out, nothing in between —
    /// a monitor that repeats itself every tick trains its reader to ignore
    /// it.
    #[test]
    fn feed_alerts_are_edge_triggered_with_a_recovery() {
        let c = cfg(30, 0);
        let mut r = Reported::default();
        let mut out = Vec::new();

        collect_feed(&c, 1, 29 * 60, &mut r, &mut out);
        assert!(out.is_empty(), "under threshold");

        collect_feed(&c, 1, 30 * 60, &mut r, &mut out);
        assert_eq!(out.len(), 1, "fires on crossing");
        assert!(out[0].2.contains("no spots for 30 min"), "{}", out[0].2);

        collect_feed(&c, 1, 45 * 60, &mut r, &mut out);
        assert_eq!(out.len(), 1, "still quiet — says nothing more");

        collect_feed(&c, 1, 0, &mut r, &mut out);
        assert_eq!(out.len(), 2, "recovery");
        assert!(out[1].2.contains("flowing again"), "{}", out[1].2);

        collect_feed(&c, 1, 0, &mut r, &mut out);
        assert_eq!(out.len(), 2, "and stays quiet once recovered");
    }

    #[test]
    fn zero_minutes_is_off() {
        let mut r = Reported::default();
        let mut out = Vec::new();
        collect_feed(&cfg(0, 0), 1, 10 * 3600, &mut r, &mut out);
        assert!(out.is_empty(), "10 hours quiet, alert switched off");
    }

    /// Switching the alert off while it is standing must not leave a report
    /// behind that fires a bogus recovery when it is switched back on.
    #[test]
    fn turning_it_off_clears_a_standing_report() {
        let mut r = Reported::default();
        let mut out = Vec::new();
        collect_feed(&cfg(30, 0), 1, 40 * 60, &mut r, &mut out);
        assert_eq!(out.len(), 1);
        collect_feed(&cfg(0, 0), 1, 40 * 60, &mut r, &mut out);
        assert_eq!(out.len(), 1, "no recovery message");
        assert!(!r.feed.contains(&1));
        // Back on, still quiet: it reports the state afresh.
        collect_feed(&cfg(30, 0), 1, 40 * 60, &mut r, &mut out);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn node_alerts_fire_once_and_recover() {
        let c = cfg(0, 10);
        let mut r = Reported::default();
        let mut out = Vec::new();
        let statuses = HashMap::from([("VU2OY".to_string(), status(false, 0))]);
        let down = HashMap::from([("VU2OY".to_string(), 1_000_i64)]);

        collect_nodes(&c, 1, 1_500, &down, &statuses, &mut r, &mut out);
        assert!(out.is_empty(), "down 8 min, threshold 10");

        collect_nodes(&c, 1, 1_600, &down, &statuses, &mut r, &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].2.contains("node VU2OY is down"), "{}", out[0].2);
        assert!(out[0].2.contains("Reconnecting"), "carries the state");

        collect_nodes(&c, 1, 2_000, &down, &statuses, &mut r, &mut out);
        assert_eq!(out.len(), 1, "no repeat while it stays down");

        // Reconnected: gone from down_since.
        let up = HashMap::from([("VU2OY".to_string(), status(true, 5))]);
        collect_nodes(&c, 1, 2_100, &HashMap::new(), &up, &mut r, &mut out);
        assert_eq!(out.len(), 2);
        assert!(out[1].2.contains("node VU2OY is back"), "{}", out[1].2);
    }

    /// Each account has its own threshold, so one being alerted must not
    /// suppress the other.
    #[test]
    fn thresholds_are_per_account() {
        let mut r = Reported::default();
        let mut out = Vec::new();
        collect_feed(&cfg(10, 0), 1, 15 * 60, &mut r, &mut out);
        collect_feed(&cfg(30, 0), 2, 15 * 60, &mut r, &mut out);
        assert_eq!(out.len(), 1, "only the 10-minute account");
        collect_feed(&cfg(30, 0), 2, 31 * 60, &mut r, &mut out);
        assert_eq!(out.len(), 2, "then the 30-minute one");
    }

    /// A node dropped from the config must not sit in the map as
    /// permanently down.
    #[test]
    fn a_removed_node_is_not_reported_forever() {
        let c = cfg(0, 1);
        let mut r = Reported::default();
        let mut out = Vec::new();
        let statuses = HashMap::from([("GONE".to_string(), status(false, 0))]);
        collect_nodes(
            &c,
            1,
            9_999,
            &HashMap::from([("GONE".to_string(), 0_i64)]),
            &statuses,
            &mut r,
            &mut out,
        );
        assert_eq!(out.len(), 1, "reported while it exists");
        // Node deleted from config: down_since is pruned by the caller, and
        // the user is told it is back rather than left wondering.
        collect_nodes(
            &c,
            1,
            10_000,
            &HashMap::new(),
            &HashMap::new(),
            &mut r,
            &mut out,
        );
        assert_eq!(out.len(), 2);
        assert!(out[1].2.contains("is back"));
    }
}
