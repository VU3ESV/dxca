//! Per-account feeds: the sources, nodes and outputs a station owns.
//!
//! Step one of `docs/MULTI-STATION.md` — **storage and namespacing only**.
//! Nothing reads these to build a pipeline yet; `config/dxca.toml` is still
//! what runs the server. This adds the place the config will live, the rule
//! that keeps two stations' names apart, and the migration that moves an
//! existing single-operator install across.
//!
//! ## Why the name needs a namespace
//!
//! The name is not a label, it is the **key**, in more places than is
//! obvious:
//!
//! * `PipelineState::apply_sources` keys its listener map on `(name, port)`
//! * `NodeManager::apply` keys `clients` and `statuses` on `name`
//! * every spot carries `source_name` — a decoder name *or* a node name, one
//!   namespace
//! * `/api/status` reports `spots_per_source` and `cluster_nodes` by name
//! * destinations filter on `sources`, by name
//!
//! So the moment two accounts both call a node `N2WQ-2`, they collide in the
//! client map, in the status map, and in every spot either produces. Asking
//! operators at different sites to coordinate names is not a plan.
//!
//! [`qualify`] prefixes the owner's callsign — `VU2CPL:N2WQ-2` — and
//! [`split`] takes it apart again for display. Two consequences, both
//! wanted: collisions become impossible, and **ownership is derivable from
//! the spot itself**, because `source_name` already travels the whole
//! pipeline. A user's filter becomes a prefix test with no second lookup and
//! no new field on `Spot`.

use crate::config::{BroadcastDestination, ClusterNode, UdpSource};
use serde::{Deserialize, Serialize};

/// Separates the owning callsign from the operator's own name for the thing.
///
/// A colon because callsigns cannot contain one, so `split` is unambiguous
/// even when the operator's own name has odd punctuation in it.
pub const SEP: char = ':';

/// One account's feeds. Field names match `config/dxca.toml`'s sections, so
/// the migration is a move rather than a translation.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FeedsUserConfig {
    #[serde(default)]
    pub udp_sources: Vec<UdpSource>,
    #[serde(default)]
    pub cluster_nodes: Vec<ClusterNode>,
    /// This account's outputs. **Passthrough destinations are not here** —
    /// they relay a decoder's datagram verbatim before parsing, are keyed to
    /// a source rather than an operator, and stay server-wide by decision
    /// (see the design note).
    #[serde(default)]
    pub destinations: Vec<BroadcastDestination>,
}

impl FeedsUserConfig {
    pub fn is_empty(&self) -> bool {
        self.udp_sources.is_empty() && self.cluster_nodes.is_empty() && self.destinations.is_empty()
    }
}

/// `VU2CPL` + `N2WQ-2` → `VU2CPL:N2WQ-2`.
///
/// The callsign is upper-cased so one account cannot produce two spellings
/// of its own prefix; the operator's own name is left exactly as typed,
/// because it is theirs and it is what they will see.
pub fn qualify(callsign: &str, name: &str) -> String {
    format!("{}{SEP}{name}", callsign.to_uppercase())
}

/// `VU2CPL:N2WQ-2` → `("VU2CPL", "N2WQ-2")`.
///
/// Splits on the **first** separator only, so a name containing a colon
/// survives intact. An unqualified string — anything from before this
/// existed, or from the TOML — comes back with an empty owner rather than
/// being rejected, because the server must keep running while both shapes
/// are in play.
pub fn split(qualified: &str) -> (&str, &str) {
    match qualified.split_once(SEP) {
        Some((owner, name)) => (owner, name),
        None => ("", qualified),
    }
}

/// Does this qualified name belong to `callsign`?
///
/// The ownership test a per-user spot filter will use. Case-insensitive on
/// the callsign; an unqualified name belongs to nobody.
pub fn owned_by(qualified: &str, callsign: &str) -> bool {
    let (owner, _) = split(qualified);
    !owner.is_empty() && owner.eq_ignore_ascii_case(callsign)
}

/// Every port this account's enabled sources want to bind.
///
/// One socket per port and DXCA is the sole listener, so two stations
/// choosing 2333 is a bind failure. `apply_sources` binds additions before
/// retiring anything so a clash surfaces as an error rather than dying in a
/// task — but under per-account ownership that error would arrive on
/// *someone else's* save, naming no culprit. Hence checking at save time.
pub fn wanted_ports(cfg: &FeedsUserConfig) -> Vec<u16> {
    cfg.udp_sources
        .iter()
        .filter(|s| s.enabled)
        .map(|s| s.port)
        .collect()
}

/// The first port `cfg` wants that another account already owns, with the
/// owner named. `None` when the account can be saved.
pub fn port_clash(
    cfg: &FeedsUserConfig,
    others: &[(String, FeedsUserConfig)],
) -> Option<(u16, String)> {
    let wanted = wanted_ports(cfg);
    for (callsign, other) in others {
        for port in wanted_ports(other) {
            if wanted.contains(&port) {
                return Some((port, callsign.clone()));
            }
        }
    }
    None
}

/// What [`migrate_from_toml`] did, for the caller to log.
#[derive(Debug, PartialEq)]
pub enum Migration {
    /// Nothing in the TOML worth moving.
    NothingToMove,
    /// Some account already owns feeds; the TOML is no longer the source.
    AlreadyMoved,
    /// Moved into the named account.
    Moved {
        callsign: String,
        sources: usize,
        nodes: usize,
        destinations: usize,
    },
    /// More than one account and no way to know whose the TOML's feeds are.
    /// Refused rather than guessed — an admin has to assign them.
    Ambiguous { accounts: usize },
}

/// Move a single-operator install's TOML feeds into its one account.
///
/// Every install in the field has exactly one account, which is the whole
/// reason this is tractable. With more than one there is no way to know
/// whose the sources and nodes are, and guessing would hand one operator
/// another's cluster logins — so it refuses and says so.
///
/// **Passthrough destinations are left behind on purpose.** They relay a
/// decoder's datagram verbatim and belong to the server's own machine, not
/// to a station.
///
/// Names are qualified on the way in, so the account owns `VU2CPL:MSHV`
/// rather than a bare `MSHV` that a second station could collide with.
/// `destinations[].sources` is rewritten to match, or an allowlist naming
/// `MSHV` would stop matching the moment the source became `VU2CPL:MSHV`.
///
/// Pure: takes what it needs and returns what to store, so the decision is
/// testable without a database or a config file.
pub fn migrate_from_toml(
    accounts: &[(i64, String, FeedsUserConfig)],
    toml_sources: &[UdpSource],
    toml_nodes: &[ClusterNode],
    toml_destinations: &[BroadcastDestination],
) -> (Migration, Option<(i64, FeedsUserConfig)>) {
    let movable: Vec<&BroadcastDestination> = toml_destinations
        .iter()
        .filter(|d| d.format != "passthrough")
        .collect();
    if toml_sources.is_empty() && toml_nodes.is_empty() && movable.is_empty() {
        return (Migration::NothingToMove, None);
    }
    if accounts.iter().any(|(_, _, f)| !f.is_empty()) {
        return (Migration::AlreadyMoved, None);
    }
    let [(user_id, callsign, _)] = accounts else {
        return (
            Migration::Ambiguous {
                accounts: accounts.len(),
            },
            None,
        );
    };

    let qualified: Vec<String> = toml_sources
        .iter()
        .map(|s| qualify(callsign, &s.name))
        .chain(toml_nodes.iter().map(|n| qualify(callsign, &n.name)))
        .collect();

    let cfg = FeedsUserConfig {
        udp_sources: toml_sources
            .iter()
            .map(|s| UdpSource {
                name: qualify(callsign, &s.name),
                ..s.clone()
            })
            .collect(),
        cluster_nodes: toml_nodes
            .iter()
            .map(|n| ClusterNode {
                name: qualify(callsign, &n.name),
                ..n.clone()
            })
            .collect(),
        destinations: movable
            .iter()
            .map(|d| BroadcastDestination {
                name: qualify(callsign, &d.name),
                // An empty allowlist means "all" and must stay empty; a
                // populated one is rewritten so it keeps matching.
                sources: d
                    .sources
                    .iter()
                    .map(|s| {
                        let q = qualify(callsign, s);
                        if qualified.contains(&q) { q } else { s.clone() }
                    })
                    .collect(),
                ..(*d).clone()
            })
            .collect(),
    };
    let done = Migration::Moved {
        callsign: callsign.clone(),
        sources: cfg.udp_sources.len(),
        nodes: cfg.cluster_nodes.len(),
        destinations: cfg.destinations.len(),
    };
    (done, Some((*user_id, cfg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(name: &str, port: u16, enabled: bool) -> UdpSource {
        UdpSource {
            name: name.into(),
            port,
            enabled,
        }
    }

    #[test]
    fn qualify_and_split_round_trip() {
        let q = qualify("vu2cpl", "N2WQ-2");
        assert_eq!(q, "VU2CPL:N2WQ-2", "callsign is normalised, name is not");
        assert_eq!(split(&q), ("VU2CPL", "N2WQ-2"));
    }

    /// Two stations naming a node the same thing is the case this exists
    /// for, and it must not need them to coordinate.
    #[test]
    fn the_same_name_under_two_callsigns_does_not_collide() {
        let a = qualify("VU2CPL", "N2WQ-2");
        let b = qualify("VU2WJ", "N2WQ-2");
        assert_ne!(a, b);
        assert_eq!(split(&a).1, split(&b).1, "both still display as N2WQ-2");
    }

    /// Split on the FIRST separator: an operator who puts a colon in their
    /// own name gets it back, rather than a truncated one.
    #[test]
    fn a_name_containing_the_separator_survives() {
        let q = qualify("VU2CPL", "node:backup");
        assert_eq!(split(&q), ("VU2CPL", "node:backup"));
    }

    /// Unqualified names exist — everything written before this, and
    /// everything still in the TOML. They must not panic or be swallowed.
    #[test]
    fn an_unqualified_name_has_no_owner() {
        assert_eq!(split("MSHV"), ("", "MSHV"));
        assert!(!owned_by("MSHV", "VU2CPL"), "belongs to nobody, not to all");
    }

    #[test]
    fn ownership_is_case_insensitive_on_the_callsign() {
        let q = qualify("VU2CPL", "MSHV");
        assert!(owned_by(&q, "vu2cpl"));
        assert!(owned_by(&q, "VU2CPL"));
        assert!(!owned_by(&q, "VU2WJ"));
    }

    /// A disabled source is not going to bind, so it cannot clash.
    #[test]
    fn only_enabled_sources_claim_a_port() {
        let cfg = FeedsUserConfig {
            udp_sources: vec![source("MSHV", 2333, true), source("OFF", 2334, false)],
            ..Default::default()
        };
        assert_eq!(wanted_ports(&cfg), vec![2333]);
    }

    /// The error has to name the other operator, or the person who cannot
    /// save has no idea who to ask.
    #[test]
    fn a_port_clash_names_the_other_account() {
        let mine = FeedsUserConfig {
            udp_sources: vec![source("MSHV", 2333, true)],
            ..Default::default()
        };
        let theirs = FeedsUserConfig {
            udp_sources: vec![source("JTDX", 2333, true)],
            ..Default::default()
        };
        let others = vec![("VU2WJ".to_string(), theirs)];
        assert_eq!(port_clash(&mine, &others), Some((2333, "VU2WJ".into())));

        // Different ports: no clash, even with the same source name.
        let free = FeedsUserConfig {
            udp_sources: vec![source("MSHV", 2336, true)],
            ..Default::default()
        };
        assert_eq!(port_clash(&free, &others), None);
    }

    fn node(name: &str) -> ClusterNode {
        ClusterNode {
            name: name.into(),
            host: "cluster.example".into(),
            port: 7300,
            login_call: "VU2CPL".into(),
            password: String::new(),
            enabled: true,
        }
    }

    fn dest(name: &str, format: &str, sources: &[&str]) -> BroadcastDestination {
        BroadcastDestination {
            name: name.into(),
            ip: "127.0.0.1".parse().unwrap(),
            port: 2237,
            format: format.into(),
            sources: sources.iter().map(|s| s.to_string()).collect(),
            unfiltered: false,
            enabled: true,
        }
    }

    /// The field case: one account, feeds in the TOML, everything qualified
    /// on the way in.
    #[test]
    fn a_single_account_install_migrates_and_gets_qualified_names() {
        let accounts = vec![(1_i64, "VU2CPL".to_string(), FeedsUserConfig::default())];
        let (what, stored) = migrate_from_toml(
            &accounts,
            &[source("MSHV", 2333, true)],
            &[node("N2WQ-2")],
            &[dest("RUMlog", "cluster", &[])],
        );
        assert_eq!(
            what,
            Migration::Moved {
                callsign: "VU2CPL".into(),
                sources: 1,
                nodes: 1,
                destinations: 1
            }
        );
        let (uid, cfg) = stored.expect("something to store");
        assert_eq!(uid, 1);
        assert_eq!(cfg.udp_sources[0].name, "VU2CPL:MSHV");
        assert_eq!(cfg.cluster_nodes[0].name, "VU2CPL:N2WQ-2");
        assert_eq!(cfg.destinations[0].name, "VU2CPL:RUMlog");
        // Everything else is carried across untouched.
        assert_eq!(cfg.udp_sources[0].port, 2333);
        assert_eq!(cfg.cluster_nodes[0].login_call, "VU2CPL");
    }

    /// Passthrough relays a decoder's datagram verbatim and belongs to the
    /// server's own machine. It stays in the TOML.
    #[test]
    fn passthrough_destinations_are_left_behind() {
        let accounts = vec![(1_i64, "VU2CPL".to_string(), FeedsUserConfig::default())];
        let (_, stored) = migrate_from_toml(
            &accounts,
            &[],
            &[],
            &[
                dest("RUMlog", "passthrough", &[]),
                dest("Log", "cluster", &[]),
            ],
        );
        let (_, cfg) = stored.unwrap();
        assert_eq!(cfg.destinations.len(), 1);
        assert_eq!(cfg.destinations[0].name, "VU2CPL:Log");
    }

    /// A destination's allowlist names sources. Qualify the sources and
    /// leave the allowlist alone and it stops matching — silently, with the
    /// destination simply going quiet.
    #[test]
    fn a_destination_allowlist_is_rewritten_to_match() {
        let accounts = vec![(1_i64, "VU2CPL".to_string(), FeedsUserConfig::default())];
        let (_, stored) = migrate_from_toml(
            &accounts,
            &[source("MSHV", 2333, true)],
            &[],
            &[dest("Log", "cluster", &["MSHV", "SOMETHING-ELSE"])],
        );
        let (_, cfg) = stored.unwrap();
        assert_eq!(
            cfg.destinations[0].sources,
            vec!["VU2CPL:MSHV".to_string(), "SOMETHING-ELSE".to_string()],
            "a name we own is qualified; one we do not is left alone"
        );
    }

    /// An empty allowlist means ALL and must not become a one-entry list.
    #[test]
    fn an_empty_allowlist_stays_empty() {
        let accounts = vec![(1_i64, "VU2CPL".to_string(), FeedsUserConfig::default())];
        let (_, stored) = migrate_from_toml(&accounts, &[], &[], &[dest("Log", "cluster", &[])]);
        assert!(stored.unwrap().1.destinations[0].sources.is_empty());
    }

    /// Runs on every open, so it has to be idempotent — a second pass must
    /// not re-qualify `VU2CPL:MSHV` into `VU2CPL:VU2CPL:MSHV`.
    #[test]
    fn migrating_twice_changes_nothing_the_second_time() {
        let first = FeedsUserConfig {
            udp_sources: vec![source("VU2CPL:MSHV", 2333, true)],
            ..Default::default()
        };
        let accounts = vec![(1_i64, "VU2CPL".to_string(), first)];
        let (what, stored) = migrate_from_toml(&accounts, &[source("MSHV", 2333, true)], &[], &[]);
        assert_eq!(what, Migration::AlreadyMoved);
        assert!(stored.is_none());
    }

    /// Two accounts and no way to know whose the TOML's cluster logins are.
    /// Guessing would hand one operator another's credentials.
    #[test]
    fn more_than_one_account_refuses_rather_than_guesses() {
        let accounts = vec![
            (1_i64, "VU2CPL".to_string(), FeedsUserConfig::default()),
            (2_i64, "VU2WJ".to_string(), FeedsUserConfig::default()),
        ];
        let (what, stored) = migrate_from_toml(&accounts, &[source("MSHV", 2333, true)], &[], &[]);
        assert_eq!(what, Migration::Ambiguous { accounts: 2 });
        assert!(stored.is_none(), "nothing written when it cannot be known");
    }

    /// A server with only passthrough has nothing to move, and must not
    /// report a migration it did not do.
    #[test]
    fn nothing_to_move_is_not_a_migration() {
        let accounts = vec![(1_i64, "VU2CPL".to_string(), FeedsUserConfig::default())];
        let (what, stored) =
            migrate_from_toml(&accounts, &[], &[], &[dest("RUMlog", "passthrough", &[])]);
        assert_eq!(what, Migration::NothingToMove);
        assert!(stored.is_none());
    }

    #[test]
    fn feeds_json_round_trips() {
        let cfg = FeedsUserConfig {
            udp_sources: vec![source("MSHV", 2333, true)],
            cluster_nodes: vec![ClusterNode {
                name: "N2WQ-2".into(),
                host: "cluster.n2wq.com".into(),
                port: 8300,
                login_call: "VU2CPL-2".into(),
                password: String::new(),
                enabled: true,
            }],
            destinations: Vec::new(),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert_eq!(serde_json::from_str::<FeedsUserConfig>(&json).unwrap(), cfg);
    }

    /// An account that predates the column deserializes from `{}` rather
    /// than failing, which is what lets `config_json` hand back a default.
    #[test]
    fn an_empty_object_is_a_valid_empty_config() {
        let cfg: FeedsUserConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.is_empty());
    }
}
