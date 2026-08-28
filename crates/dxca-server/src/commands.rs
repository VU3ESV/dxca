//! Telnet command policy — [`docs/TELNET-INTERACTIVE.md`] §4.
//!
//! DXSpider lets every command be abbreviated (`SHOW/DX` → `SH/DX`,
//! `ANNOUNCE` → `AN`) with **no documented minimum length and no uniqueness
//! rule**. That single fact decides the whole design here:
//!
//! * A denylist is impossible. Blocking `SET/HOMENODE` would mean
//!   enumerating `SET/HOME`, `SE/HOMENODE`, `S/HOME` and every other
//!   spelling a node might accept; miss one and it goes straight through to
//!   a session authenticated as the station owner.
//! * Prefix-matching on `SH/` is no better: `SH/` does not identify a show
//!   command, and `SET/…` shares its first letter.
//!
//! So: **canonicalize first, then allowlist.** An incoming verb is expanded
//! against the table below, and only the canonical form is judged. Anything
//! that does not expand — unknown, or ambiguous between two commands — is
//! refused. Fail closed, every time.
//!
//! The cost is that DXCA must carry a command inventory, and a node-specific
//! command it has never heard of gets refused even though the node would
//! accept it. That is the correct trade: an operator who needs an exotic
//! command can telnet to the node directly.

/// What DXCA does with a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// DXCA answers it; never reaches a node.
    Local,
    /// Forwarded to the operator's current node.
    ReadOnly,
    /// Node-side filters. Refused: DXCA holds **one** session per node,
    /// shared by every user and by the spot pipeline, so a filter set here
    /// would silently narrow the Spots feed and Telegram alerts for
    /// everybody — and persist on the node.
    Filter,
    /// Puts something on the air or in front of other operators, as the
    /// station owner. Refused until milestone 4 gates it on admin.
    Transmit,
    /// Mutates the node account, the shared session, or its mail. Always
    /// refused: `SET/PASSWORD` and `SYSOP` are the obvious ones, but
    /// `UNSET/DX` would stop the node sending spots at all, and `BYE` would
    /// drop the session every DXCA user shares.
    Mutate,
}

/// The commands DXCA knows how to canonicalize. Anything absent is refused,
/// which is the point — see the module docs.
const COMMANDS: &[(&str, Tier)] = &[
    // --- local: answered by DXCA itself ---------------------------------
    ("HELP", Tier::Local),
    ("SHOW/NODES", Tier::Local),
    ("SET/NODE", Tier::Local),
    ("SHOW/DXCA", Tier::Local),
    ("BYE", Tier::Local),
    ("QUIT", Tier::Local),
    // --- read-only queries, forwarded -----------------------------------
    ("SHOW/DX", Tier::ReadOnly),
    ("SHOW/MYDX", Tier::ReadOnly),
    ("SHOW/FDX", Tier::ReadOnly),
    ("SHOW/DXCC", Tier::ReadOnly),
    ("SHOW/DXSTATS", Tier::ReadOnly),
    ("SHOW/DXQSL", Tier::ReadOnly),
    ("SHOW/WWV", Tier::ReadOnly),
    ("SHOW/WCY", Tier::ReadOnly),
    ("SHOW/MUF", Tier::ReadOnly),
    ("SHOW/SUN", Tier::ReadOnly),
    ("SHOW/MOON", Tier::ReadOnly),
    ("SHOW/PREFIX", Tier::ReadOnly),
    ("SHOW/QRZ", Tier::ReadOnly),
    ("SHOW/QRA", Tier::ReadOnly),
    ("SHOW/WM7D", Tier::ReadOnly),
    ("SHOW/DB0SDX", Tier::ReadOnly),
    ("SHOW/HEADING", Tier::ReadOnly),
    ("SHOW/SATELLITE", Tier::ReadOnly),
    ("SHOW/CONTEST", Tier::ReadOnly),
    ("SHOW/DATE", Tier::ReadOnly),
    ("SHOW/TIME", Tier::ReadOnly),
    ("SHOW/STATION", Tier::ReadOnly),
    ("SHOW/CONFIGURATION", Tier::ReadOnly),
    ("SHOW/LINKS", Tier::ReadOnly),
    ("SHOW/ROUTE", Tier::ReadOnly),
    ("SHOW/HFSTATS", Tier::ReadOnly),
    ("SHOW/HFTABLE", Tier::ReadOnly),
    ("SHOW/VHFSTATS", Tier::ReadOnly),
    ("SHOW/VHFTABLE", Tier::ReadOnly),
    ("SHOW/FILES", Tier::ReadOnly),
    ("SHOW/FILTER", Tier::ReadOnly),
    ("APROPOS", Tier::ReadOnly),
    ("DBAVAIL", Tier::ReadOnly),
    ("DBSHOW", Tier::ReadOnly),
    ("WHO", Tier::ReadOnly),
    // --- node-side filters, refused (shared session) ---------------------
    ("ACCEPT/SPOTS", Tier::Filter),
    ("ACCEPT/ANNOUNCE", Tier::Filter),
    ("ACCEPT/WWV", Tier::Filter),
    ("ACCEPT/WCY", Tier::Filter),
    ("ACCEPT/RBN", Tier::Filter),
    ("REJECT/SPOTS", Tier::Filter),
    ("REJECT/ANNOUNCE", Tier::Filter),
    ("REJECT/WWV", Tier::Filter),
    ("REJECT/WCY", Tier::Filter),
    ("REJECT/RBN", Tier::Filter),
    ("CLEAR/SPOTS", Tier::Filter),
    ("CLEAR/ANNOUNCE", Tier::Filter),
    ("CLEAR/WWV", Tier::Filter),
    ("CLEAR/WCY", Tier::Filter),
    ("CLEAR/RBN", Tier::Filter),
    ("CLEAR/ROUTE", Tier::Filter),
    // --- transmits, refused until M4 -------------------------------------
    ("DX", Tier::Transmit),
    ("ANNOUNCE", Tier::Transmit),
    ("WX", Tier::Transmit),
    ("TALK", Tier::Transmit),
    ("CHAT", Tier::Transmit),
    ("JOIN", Tier::Transmit),
    ("LEAVE", Tier::Transmit),
    ("SEND", Tier::Transmit),
    ("REPLY", Tier::Transmit),
    // --- account / session mutation, always refused ----------------------
    ("SET/NAME", Tier::Mutate),
    ("SET/QTH", Tier::Mutate),
    ("SET/QRA", Tier::Mutate),
    ("SET/LOCATION", Tier::Mutate),
    ("SET/ADDRESS", Tier::Mutate),
    ("SET/EMAIL", Tier::Mutate),
    ("SET/HOMENODE", Tier::Mutate),
    ("SET/PASSWORD", Tier::Mutate),
    ("SET/STARTUP", Tier::Mutate),
    ("SET/LANGUAGE", Tier::Mutate),
    ("SET/PROMPT", Tier::Mutate),
    ("SET/PAGE", Tier::Mutate),
    ("SET/USSTATE", Tier::Mutate),
    ("SET/DXCQ", Tier::Mutate),
    ("SET/DXITU", Tier::Mutate),
    ("SET/DXGRID", Tier::Mutate),
    ("SET/BEEP", Tier::Mutate),
    ("SET/ECHO", Tier::Mutate),
    ("SET/HERE", Tier::Mutate),
    ("SET/LOGININFO", Tier::Mutate),
    ("SET/ANNOUNCE", Tier::Mutate),
    ("SET/TALK", Tier::Mutate),
    ("SET/WWV", Tier::Mutate),
    ("SET/WCY", Tier::Mutate),
    ("SET/WX", Tier::Mutate),
    ("SET/DX", Tier::Mutate),
    ("SET/SEEME", Tier::Mutate),
    ("UNSET/ANNOUNCE", Tier::Mutate),
    ("UNSET/DX", Tier::Mutate),
    ("UNSET/TALK", Tier::Mutate),
    ("UNSET/WWV", Tier::Mutate),
    ("UNSET/WCY", Tier::Mutate),
    ("UNSET/WX", Tier::Mutate),
    ("UNSET/EMAIL", Tier::Mutate),
    ("UNSET/DXCQ", Tier::Mutate),
    ("UNSET/DXITU", Tier::Mutate),
    ("UNSET/DXGRID", Tier::Mutate),
    ("UNSET/USSTATE", Tier::Mutate),
    ("UNSET/PROMPT", Tier::Mutate),
    ("UNSET/STARTUP", Tier::Mutate),
    ("UNSET/BEEP", Tier::Mutate),
    ("UNSET/ECHO", Tier::Mutate),
    ("UNSET/HERE", Tier::Mutate),
    ("UNSET/LOGININFO", Tier::Mutate),
    ("UNSET/PRIVILEGE", Tier::Mutate),
    ("SYSOP", Tier::Mutate),
    ("KILL", Tier::Mutate),
    ("DIRECTORY", Tier::Mutate),
    ("READ", Tier::Mutate),
    ("ENABLE/FTX", Tier::Mutate),
    ("DISABLE/FTX", Tier::Mutate),
];

/// Why a command was not forwarded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Unknown,
    /// Expanded to more than one command; the operator must be less terse.
    Ambiguous(Vec<String>),
    Filter,
    Transmit,
    Mutate,
}

impl Refusal {
    /// The line sent back. Always says *why* — a silently dropped command
    /// is indistinguishable from a broken link.
    pub fn message(&self, verb: &str) -> String {
        match self {
            Refusal::Unknown => format!(
                "{verb}: not a command DXCA knows, so it is not forwarded. \
                 Telnet the node directly if you need it."
            ),
            Refusal::Ambiguous(candidates) => format!(
                "{verb}: ambiguous — could be {}. Type more of it.",
                candidates.join(", ")
            ),
            Refusal::Filter => format!(
                "{verb}: refused. DXCA shares one session per node with every \
                 user and with the spot feed, so a node-side filter would \
                 narrow everyone's spots, not just yours."
            ),
            Refusal::Transmit => {
                format!("{verb}: refused. Transmitting is not enabled on this passthrough.")
            }
            Refusal::Mutate => format!(
                "{verb}: refused. It would change the node account or the \
                 shared session that every DXCA user depends on."
            ),
        }
    }
}

/// The outcome of classifying one input line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Classified {
    /// DXCA answers this itself; `canonical` says which.
    Local { canonical: String, args: String },
    /// Send `line` to the operator's current node.
    Forward { canonical: String, line: String },
    Refused { canonical: Option<String>, why: Refusal },
}

/// Expand an abbreviated verb to its canonical form.
///
/// Segment-wise prefix matching (`SH/DX` → `SHOW/DX`), with ties broken by
/// **exact segment matches**: `SH/DX` prefix-matches `SHOW/DXCC` and
/// `SHOW/DXQSL` too, but only `SHOW/DX` matches its second segment exactly,
/// so the everyday command still resolves. `SH/D`, which matches nothing
/// exactly, stays ambiguous and is refused.
pub fn canonicalize(verb: &str) -> Result<(&'static str, Tier), Refusal> {
    let upper = verb.to_uppercase();
    let input: Vec<&str> = upper.split('/').collect();
    if input.iter().any(|s| s.is_empty()) {
        return Err(Refusal::Unknown);
    }

    let mut best_exact = 0usize;
    let mut best: Vec<(&'static str, Tier)> = Vec::new();
    for (name, tier) in COMMANDS {
        let canon: Vec<&str> = name.split('/').collect();
        if canon.len() != input.len() {
            continue;
        }
        let mut exact = 0;
        let mut ok = true;
        for (i, seg) in input.iter().enumerate() {
            if !canon[i].starts_with(seg) {
                ok = false;
                break;
            }
            if canon[i] == *seg {
                exact += 1;
            }
        }
        if !ok {
            continue;
        }
        if exact > best_exact {
            best_exact = exact;
            best.clear();
        }
        if exact == best_exact {
            best.push((name, *tier));
        }
    }

    match best.len() {
        0 => Err(Refusal::Unknown),
        1 => Ok(best[0]),
        _ => Err(Refusal::Ambiguous(
            best.iter().map(|(n, _)| (*n).to_string()).collect(),
        )),
    }
}

/// Classify one whole input line from an authenticated session.
pub fn classify(line: &str) -> Classified {
    let line = line.trim();
    let (verb, args) = match line.split_once(char::is_whitespace) {
        Some((v, rest)) => (v, rest.trim()),
        None => (line, ""),
    };
    match canonicalize(verb) {
        Err(why) => Classified::Refused {
            canonical: None,
            why,
        },
        Ok((canonical, tier)) => match tier {
            Tier::Local => Classified::Local {
                canonical: canonical.to_string(),
                args: args.to_string(),
            },
            Tier::ReadOnly => Classified::Forward {
                canonical: canonical.to_string(),
                // The node gets the canonical verb, not the abbreviation:
                // what DXCA judged and what the node runs must be the same
                // string, or the allowlist is decorative.
                line: if args.is_empty() {
                    canonical.to_string()
                } else {
                    format!("{canonical} {args}")
                },
            },
            Tier::Filter => Classified::Refused {
                canonical: Some(canonical.to_string()),
                why: Refusal::Filter,
            },
            Tier::Transmit => Classified::Refused {
                canonical: Some(canonical.to_string()),
                why: Refusal::Transmit,
            },
            Tier::Mutate => Classified::Refused {
                canonical: Some(canonical.to_string()),
                why: Refusal::Mutate,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(v: &str) -> &'static str {
        canonicalize(v).unwrap_or_else(|e| panic!("{v} did not resolve: {e:?}")).0
    }

    #[test]
    fn everyday_abbreviations_resolve() {
        assert_eq!(canon("sh/dx"), "SHOW/DX");
        assert_eq!(canon("SH/DX"), "SHOW/DX");
        assert_eq!(canon("show/dx"), "SHOW/DX");
        assert_eq!(canon("sh/wwv"), "SHOW/WWV");
        assert_eq!(canon("sh/mu"), "SHOW/MUF");
        assert_eq!(canon("an"), "ANNOUNCE");
        assert_eq!(canon("dx"), "DX");
    }

    /// `SH/DX` also prefix-matches SHOW/DXCC, SHOW/DXQSL and SHOW/DXSTATS.
    /// The exact-segment tiebreak is what keeps the commonest command in
    /// the whole protocol working; without it this is ambiguous.
    #[test]
    fn exact_segments_beat_longer_prefix_matches() {
        assert_eq!(canon("sh/dx"), "SHOW/DX");
        assert_eq!(canon("sh/dxcc"), "SHOW/DXCC");
        assert_eq!(canon("sh/dxq"), "SHOW/DXQSL");
    }

    #[test]
    fn genuinely_ambiguous_input_is_refused_not_guessed() {
        match canonicalize("sh/d") {
            Err(Refusal::Ambiguous(c)) => {
                assert!(c.len() > 1, "should list the candidates: {c:?}");
                assert!(c.contains(&"SHOW/DXCC".to_string()));
            }
            other => panic!("sh/d must not resolve to one command: {other:?}"),
        }
    }

    #[test]
    fn unknown_verbs_fail_closed() {
        assert_eq!(canonicalize("frobnicate"), Err(Refusal::Unknown));
        assert_eq!(canonicalize("sh/frob"), Err(Refusal::Unknown));
        assert_eq!(canonicalize(""), Err(Refusal::Unknown));
        assert_eq!(canonicalize("sh/"), Err(Refusal::Unknown));
        assert_eq!(canonicalize("/dx"), Err(Refusal::Unknown));
    }

    /// The whole reason a denylist was rejected: every abbreviation of a
    /// dangerous command must land on the same refusal.
    #[test]
    fn no_abbreviation_of_a_dangerous_command_slips_through() {
        for spelling in [
            "set/password",
            "set/passw",
            "set/pass",
            "SET/PASSWORD",
            "se/password",
            "s/password",
        ] {
            let (canonical, tier) = canonicalize(spelling)
                .unwrap_or_else(|e| panic!("{spelling} should resolve, got {e:?}"));
            assert_eq!(canonical, "SET/PASSWORD", "for {spelling}");
            assert_eq!(tier, Tier::Mutate, "for {spelling}");
        }
        // And the one that would silently kill the shack's spot feed.
        assert_eq!(canonicalize("unset/dx").unwrap().1, Tier::Mutate);
        assert_eq!(canonicalize("uns/dx").unwrap().1, Tier::Mutate);
        assert_eq!(canonicalize("sysop").unwrap().1, Tier::Mutate);
        assert_eq!(canonicalize("sys").unwrap().1, Tier::Mutate);
    }

    #[test]
    fn read_only_queries_forward_with_their_arguments() {
        match classify("sh/dx 20") {
            Classified::Forward { canonical, line } => {
                assert_eq!(canonical, "SHOW/DX");
                assert_eq!(line, "SHOW/DX 20", "the node runs what we judged");
            }
            other => panic!("expected a forward, got {other:?}"),
        }
        match classify("sh/wwv") {
            Classified::Forward { line, .. } => assert_eq!(line, "SHOW/WWV"),
            other => panic!("expected a forward, got {other:?}"),
        }
    }

    #[test]
    fn spotting_is_refused_in_this_milestone() {
        match classify("dx 14074.0 K1JT testing") {
            Classified::Refused { why, canonical } => {
                assert_eq!(why, Refusal::Transmit);
                assert_eq!(canonical.as_deref(), Some("DX"));
            }
            other => panic!("DX must not be forwarded yet: {other:?}"),
        }
    }

    #[test]
    fn node_side_filters_are_refused_because_the_session_is_shared() {
        for spelling in ["acc/spots dx", "rej/spots freq hf", "clear/spots all"] {
            match classify(spelling) {
                Classified::Refused { why, .. } => assert_eq!(why, Refusal::Filter, "{spelling}"),
                other => panic!("{spelling} must be refused: {other:?}"),
            }
        }
    }

    #[test]
    fn refusals_explain_themselves() {
        let msg = Refusal::Filter.message("acc/spots");
        assert!(msg.contains("acc/spots"));
        assert!(msg.to_lowercase().contains("shar"), "says why: {msg}");
        let msg = Refusal::Mutate.message("set/homenode");
        assert!(msg.to_lowercase().contains("node account"), "says why: {msg}");
    }

    #[test]
    fn local_commands_never_reach_a_node() {
        for spelling in ["help", "sh/nodes", "set/node DB0SUE", "bye", "sh/dxca"] {
            match classify(spelling) {
                Classified::Local { .. } => {}
                other => panic!("{spelling} must be handled locally: {other:?}"),
            }
        }
        match classify("set/node DB0SUE") {
            Classified::Local { canonical, args } => {
                assert_eq!(canonical, "SET/NODE");
                assert_eq!(args, "DB0SUE");
            }
            other => panic!("got {other:?}"),
        }
    }

    /// Nothing in the table may be reachable only by its full name — every
    /// entry must survive a round trip through the canonicalizer, or the
    /// tier it was given is unreachable and the table is lying.
    #[test]
    fn every_table_entry_canonicalizes_to_itself() {
        for (name, tier) in COMMANDS {
            let (got, got_tier) = canonicalize(name)
                .unwrap_or_else(|e| panic!("{name} does not resolve to itself: {e:?}"));
            assert_eq!(got, *name, "{name} resolved to {got}");
            assert_eq!(got_tier, *tier, "{name} changed tier");
        }
    }

    /// A dangerous command must never be reachable by a spelling that
    /// resolves to a *harmless* one. Belt and braces over the tier table.
    #[test]
    fn no_spelling_of_a_mutating_command_resolves_to_a_forwarded_one() {
        for (name, tier) in COMMANDS {
            if *tier != Tier::Mutate && *tier != Tier::Transmit {
                continue;
            }
            // Every prefix of the dangerous name, segment by segment.
            let segs: Vec<&str> = name.split('/').collect();
            for cut in 1..=segs[0].len() {
                let mut spelling = segs[0][..cut].to_string();
                for s in &segs[1..] {
                    spelling.push('/');
                    spelling.push_str(s);
                }
                if let Ok((_, resolved_tier)) = canonicalize(&spelling) {
                    assert!(
                        resolved_tier != Tier::ReadOnly,
                        "{spelling} (abbreviating {name}) resolved to a FORWARDED command"
                    );
                }
            }
        }
    }
}
