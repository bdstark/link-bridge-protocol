// SPDX-License-Identifier: MIT

//! The messages between `gma3-phrase-sync` and a Link bridge: protocols LB1 and LB2, specified in
//! `docs/link-bridge.md`.
//!
//! The service sends the selected deck's timeline (tempo, a beat and the time it falls at, playing or stopped); the
//! bridge, a separate program, publishes it to a Link session. In LB2 the bridge also reports the session as it sees
//! it ([`Session`]). This crate only encodes and decodes the messages. It has no Link code and no sockets, so both
//! sides use it, whatever their licence: the bridge depends on it, never the other way.
//!
//! A message is one UDP datagram of tab-separated UTF-8 text, at most [`MAX_LEN`] bytes, whose first field is the
//! protocol, a [`Protocol`].

use std::fmt;

/// The longest message, in bytes.
pub const MAX_LEN: usize = 512;
/// The longest bar a timeline may count beats in.
pub const MAX_QUANTUM: u8 = 16;

/// A revision of the protocol, the first field of every message. Each revision has every message of the ones before
/// it, so a side that speaks LB2 also speaks LB1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Protocol {
    /// Clock sync, timelines, goodbye.
    Lb1,
    /// LB1, and [`Message::Session`]: the bridge reports the Link session to the service.
    Lb2,
}

impl Protocol {
    /// The newest revision this crate speaks.
    pub const LATEST: Protocol = Protocol::Lb2;
    /// Every revision this crate speaks, oldest first.
    pub const ALL: [Protocol; 2] = [Protocol::Lb1, Protocol::Lb2];

    /// The first field of a message in this revision, such as `LB1`.
    pub fn name(self) -> &'static str {
        match self {
            Protocol::Lb1 => "LB1",
            Protocol::Lb2 => "LB2",
        }
    }

    /// The revision whose first field is `name`.
    pub fn from_name(name: &str) -> Option<Protocol> {
        Protocol::ALL.into_iter().find(|p| p.name() == name)
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Whether the deck is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Playing forward under its own power: the timeline predicts.
    Play,
    /// Stopped, paused, scratching, or no deck selected: nothing should be predicted from the timeline.
    Stop,
}

/// The selected deck's timeline, on the bridge's clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeline {
    /// The sender's run, so a restarted sender's sequence numbers aren't taken as old.
    pub nonce: u64,
    /// Increases with every timeline the sender sends in a run; a lower one than the last is late.
    pub seq: u32,
    /// Playing or stopped.
    pub state: State,
    /// At this time on the bridge's clock, in µs…
    pub at_us: i64,
    /// …the deck is at this beat. With a `quantum`, a beat that is a multiple of it is a bar's first beat.
    pub beat: f64,
    /// The playing tempo; 0 when unknown (the bridge keeps the session's tempo).
    pub bpm: f64,
    /// Beats per bar, 1 to [`MAX_QUANTUM`]; 0 when the bar's phase is unknown (the bridge sets only the tempo).
    pub quantum: u8,
}

/// The Link session as the bridge sees it (LB2), on the bridge's clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Session {
    /// The service run it is sent to: the `nonce` of that run's pings.
    pub nonce: u64,
    /// Counts the reports of one subscription from 1 (it starts at 1 again when the run subscribes again or the
    /// bridge restarts). A report with `seq` 1 is always taken; another at or below the last one taken is late.
    pub seq: u32,
    /// At this time on the bridge's clock, in µs…
    pub at_us: i64,
    /// …the session is at this beat at `quantum`. Its phase in the bar (`beat` modulo `quantum`) is the session's;
    /// its count of whole bars is the bridge's own, as with any Link peer.
    pub beat: f64,
    /// The session's tempo, BPM.
    pub bpm: f64,
    /// The beats per bar `beat` is counted in, 1 to [`MAX_QUANTUM`]: the bridge's setting (Link doesn't share one).
    pub quantum: u8,
    /// Whether the session plays: `None` when the bridge doesn't share start and stop, so it doesn't know.
    pub playing: Option<bool>,
    /// How many other Link peers the bridge sees.
    pub peers: u32,
    /// Whether the session's tempo or phase is this run's: the bridge applied one of this run's timelines that sets
    /// either in the last 3 s.
    pub publishing: bool,
}

/// One message.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Message {
    /// Service → bridge: measure the clocks. Answered with a [`Message::Pong`] at once.
    Ping {
        /// The sender's run.
        nonce: u64,
        /// The ping's number.
        seq: u32,
        /// The sender's clock when it sent the ping, µs.
        sender_us: i64,
    },
    /// Bridge → service: the answer to a ping.
    Pong {
        /// The ping's `nonce`, echoed.
        nonce: u64,
        /// The ping's `seq`, echoed.
        seq: u32,
        /// The ping's `sender_us`, echoed.
        sender_us: i64,
        /// The bridge's clock when it received the ping, µs: the clock its timeline times are on.
        bridge_us: i64,
        /// How many other Link peers the bridge sees.
        peers: u32,
    },
    /// Service → bridge: the selected deck's timeline.
    Timeline(Timeline),
    /// Service → bridge: the sender is going away; the bridge stops publishing its timeline.
    Bye {
        /// The sender's run.
        nonce: u64,
    },
    /// Bridge → service, LB2 only: the Link session.
    Session(Session),
    /// Either way: the answer to a message in another protocol, naming this side's.
    Version,
}

/// Why a datagram isn't a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// A message in a protocol this crate doesn't speak (its first field, for example `LB3`).
    OtherProtocol(String),
    /// Not a message, or a malformed one.
    Malformed(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::OtherProtocol(p) => write!(f, "speaks {p}, not LB1 or LB2"),
            DecodeError::Malformed(why) => write!(f, "malformed message: {why}"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl Message {
    /// The oldest protocol that has this message: LB2 for [`Message::Session`], LB1 for the rest.
    pub fn since(&self) -> Protocol {
        match self {
            Message::Session(_) => Protocol::Lb2,
            _ => Protocol::Lb1,
        }
    }

    /// The datagram, in the oldest protocol that has the message ([`Message::since`]), which every side that knows
    /// the message reads.
    pub fn encode(&self) -> Vec<u8> {
        self.encode_in(self.since())
    }

    /// The datagram in `protocol`, or in [`Message::since`] if `protocol` is older and doesn't have the message.
    pub fn encode_in(&self, protocol: Protocol) -> Vec<u8> {
        let fields = match *self {
            Message::Ping { nonce, seq, sender_us } => format!("PING\t{nonce:016x}\t{seq}\t{sender_us}"),
            Message::Pong { nonce, seq, sender_us, bridge_us, peers } => {
                format!("PONG\t{nonce:016x}\t{seq}\t{sender_us}\t{bridge_us}\t{peers}")
            }
            Message::Timeline(t) => {
                let state = match t.state {
                    State::Play => "play",
                    State::Stop => "stop",
                };
                // `{}` gives the shortest text that reads back as the same f64.
                format!("TL\t{:016x}\t{}\t{state}\t{}\t{}\t{}\t{}", t.nonce, t.seq, t.at_us, t.beat, t.bpm, t.quantum)
            }
            Message::Bye { nonce } => format!("BYE\t{nonce:016x}"),
            Message::Session(s) => {
                let playing = match s.playing {
                    Some(true) => "play",
                    Some(false) => "stop",
                    None => "unknown",
                };
                format!(
                    "SESSION\t{:016x}\t{}\t{}\t{}\t{}\t{}\t{playing}\t{}\t{}",
                    s.nonce,
                    s.seq,
                    s.at_us,
                    s.beat,
                    s.bpm,
                    s.quantum,
                    s.peers,
                    u8::from(s.publishing)
                )
            }
            Message::Version => "VERSION".to_owned(),
        };
        format!("{}\t{fields}", protocol.max(self.since())).into_bytes()
    }

    /// The message in a datagram in any protocol this crate speaks. [`Message::decode_versioned`] also says which.
    pub fn decode(data: &[u8]) -> Result<Message, DecodeError> {
        Message::decode_versioned(data).map(|(_, m)| m)
    }

    /// The protocol a datagram is in and its message. A message the protocol doesn't have (a `SESSION` in LB1) is
    /// malformed.
    pub fn decode_versioned(data: &[u8]) -> Result<(Protocol, Message), DecodeError> {
        let bad = |why: &str| DecodeError::Malformed(why.to_owned());
        if data.len() > MAX_LEN {
            return Err(bad("too long"));
        }
        let text = std::str::from_utf8(data).map_err(|_| bad("not UTF-8"))?;
        let mut f = text.split('\t');
        let first = f.next().unwrap_or_default();
        let Some(protocol) = Protocol::from_name(first) else {
            return Err(if first.starts_with("LB") && first.len() <= 8 {
                DecodeError::OtherProtocol(first.to_owned())
            } else {
                bad("not LB1 or LB2")
            });
        };
        let kind = f.next().ok_or_else(|| bad("no message type"))?;
        let rest: Vec<&str> = f.collect();
        let want = |n: usize| {
            if rest.len() == n { Ok(()) } else { Err(DecodeError::Malformed(format!("{kind} needs {n} fields"))) }
        };
        let nonce = |s: &str| {
            if s.len() == 16 {
                u64::from_str_radix(s, 16).map_err(|_| bad("nonce is not 16 hex digits"))
            } else {
                Err(bad("nonce is not 16 hex digits"))
            }
        };
        let int = |s: &str, what: &str| s.parse::<i64>().map_err(|_| DecodeError::Malformed(format!("{what}: {s:?}")));
        let uint = |s: &str, what: &str| s.parse::<u32>().map_err(|_| DecodeError::Malformed(format!("{what}: {s:?}")));
        let real = |s: &str, what: &str| match s.parse::<f64>() {
            Ok(v) if v.is_finite() => Ok(v),
            _ => Err(DecodeError::Malformed(format!("{what}: {s:?}"))),
        };
        let tempo = |s: &str| {
            let bpm = real(s, "bpm")?;
            if bpm < 0.0 { Err(bad("bpm is negative")) } else { Ok(bpm) }
        };
        let message = match kind {
            "PING" => {
                want(3)?;
                Message::Ping { nonce: nonce(rest[0])?, seq: uint(rest[1], "seq")?, sender_us: int(rest[2], "time")? }
            }
            "PONG" => {
                want(5)?;
                Message::Pong {
                    nonce: nonce(rest[0])?,
                    seq: uint(rest[1], "seq")?,
                    sender_us: int(rest[2], "sender time")?,
                    bridge_us: int(rest[3], "bridge time")?,
                    peers: uint(rest[4], "peers")?,
                }
            }
            "TL" => {
                want(7)?;
                let state = match rest[2] {
                    "play" => State::Play,
                    "stop" => State::Stop,
                    other => return Err(DecodeError::Malformed(format!("state: {other:?}"))),
                };
                let quantum = rest[6].parse::<u8>().ok().filter(|q| *q <= MAX_QUANTUM).ok_or_else(|| bad("quantum"))?;
                Message::Timeline(Timeline {
                    nonce: nonce(rest[0])?,
                    seq: uint(rest[1], "seq")?,
                    state,
                    at_us: int(rest[3], "time")?,
                    beat: real(rest[4], "beat")?,
                    bpm: tempo(rest[5])?,
                    quantum,
                })
            }
            "BYE" => {
                want(1)?;
                Message::Bye { nonce: nonce(rest[0])? }
            }
            "SESSION" if protocol >= Protocol::Lb2 => {
                want(9)?;
                let quantum = rest[5]
                    .parse::<u8>()
                    .ok()
                    .filter(|q| (1..=MAX_QUANTUM).contains(q))
                    .ok_or_else(|| bad("quantum"))?;
                let playing = match rest[6] {
                    "play" => Some(true),
                    "stop" => Some(false),
                    "unknown" => None,
                    other => return Err(DecodeError::Malformed(format!("playing: {other:?}"))),
                };
                let publishing = match rest[8] {
                    "1" => true,
                    "0" => false,
                    other => return Err(DecodeError::Malformed(format!("publishing: {other:?}"))),
                };
                Message::Session(Session {
                    nonce: nonce(rest[0])?,
                    seq: uint(rest[1], "seq")?,
                    at_us: int(rest[2], "time")?,
                    beat: real(rest[3], "beat")?,
                    bpm: tempo(rest[4])?,
                    quantum,
                    playing,
                    peers: uint(rest[7], "peers")?,
                    publishing,
                })
            }
            "VERSION" => {
                want(0)?;
                Message::Version
            }
            other => return Err(DecodeError::Malformed(format!("unknown message type {other:?} in {protocol}"))),
        };
        Ok((protocol, message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONCE: u64 = 0x0123_4567_89ab_cdef;

    fn tl() -> Timeline {
        Timeline {
            nonce: NONCE,
            seq: 7,
            state: State::Play,
            at_us: 1_234_567_890,
            beat: 64.123_456_789_012_34,
            bpm: 126.000_001,
            quantum: 4,
        }
    }

    fn session() -> Session {
        Session {
            nonce: NONCE,
            seq: 3,
            at_us: 90_002_000,
            beat: 1_031.499_999_999_999_8,
            bpm: 125.999_999_9,
            quantum: 4,
            playing: Some(true),
            peers: 2,
            publishing: false,
        }
    }

    fn lb1_messages() -> [Message; 6] {
        [
            Message::Ping { nonce: NONCE, seq: 1, sender_us: -5 },
            Message::Pong { nonce: NONCE, seq: 1, sender_us: -5, bridge_us: 9_000_000_000, peers: 3 },
            Message::Timeline(tl()),
            Message::Timeline(Timeline { state: State::Stop, bpm: 0.0, quantum: 0, ..tl() }),
            Message::Bye { nonce: NONCE },
            Message::Version,
        ]
    }

    #[test]
    fn every_message_reads_back_as_sent() {
        for m in lb1_messages() {
            assert_eq!(Message::decode(&m.encode()), Ok(m), "{}", String::from_utf8_lossy(&m.encode()));
        }
    }

    #[test]
    fn every_message_reads_back_as_sent_in_each_protocol() {
        let sessions = [
            Message::Session(session()),
            Message::Session(Session { playing: Some(false), publishing: true, peers: 0, ..session() }),
            Message::Session(Session { playing: None, quantum: 1, beat: -0.25, ..session() }),
            Message::Session(Session { quantum: MAX_QUANTUM, bpm: 20.0, ..session() }),
        ];
        for p in Protocol::ALL {
            for m in lb1_messages() {
                assert_eq!(Message::decode_versioned(&m.encode_in(p)), Ok((p, m)), "{m:?} in {p}");
            }
        }
        // A session is LB2 only: asked for in LB1, it is written in LB2.
        for m in sessions {
            assert_eq!(m.encode_in(Protocol::Lb1), m.encode_in(Protocol::Lb2));
            assert_eq!(Message::decode_versioned(&m.encode()), Ok((Protocol::Lb2, m)), "{m:?}");
        }
    }

    #[test]
    fn the_wire_form_is_tab_separated_text() {
        assert_eq!(
            Message::Timeline(tl()).encode(),
            b"LB1\tTL\t0123456789abcdef\t7\tplay\t1234567890\t64.12345678901234\t126.000001\t4"
        );
        assert_eq!(Message::Ping { nonce: 1, seq: 2, sender_us: 3 }.encode(), b"LB1\tPING\t0000000000000001\t2\t3");
        assert_eq!(
            Message::Ping { nonce: 1, seq: 2, sender_us: 3 }.encode_in(Protocol::Lb2),
            b"LB2\tPING\t0000000000000001\t2\t3"
        );
        assert_eq!(Message::Version.encode(), b"LB1\tVERSION");
        assert_eq!(Message::Version.encode_in(Protocol::LATEST), b"LB2\tVERSION");
        assert_eq!(
            Message::Session(session()).encode(),
            b"LB2\tSESSION\t0123456789abcdef\t3\t90002000\t1031.4999999999998\t125.9999999\t4\tplay\t2\t0"
        );
        let unknown = Session { playing: None, publishing: true, beat: 2.0, bpm: 120.0, ..session() };
        assert_eq!(
            Message::Session(unknown).encode(),
            b"LB2\tSESSION\t0123456789abcdef\t3\t90002000\t2\t120\t4\tunknown\t2\t1"
        );
    }

    #[test]
    fn floats_are_written_as_the_shortest_text_that_reads_back() {
        // Whole numbers have no point, and every value reads back as the same bits.
        for (v, text) in [(120.0, "120"), (0.1, "0.1"), (1.0 / 3.0, "0.3333333333333333"), (-0.0, "-0")] {
            let s = Session { beat: v, bpm: v.abs(), ..session() };
            let wire = String::from_utf8(Message::Session(s).encode()).unwrap();
            let fields: Vec<&str> = wire.split('\t').collect();
            assert_eq!((fields[5], fields[6]), (text, text.trim_start_matches('-')), "{v}");
            let Ok(Message::Session(back)) = Message::decode(wire.as_bytes()) else { panic!("{wire}") };
            assert_eq!((back.beat.to_bits(), back.bpm.to_bits()), (v.to_bits(), v.abs().to_bits()), "{wire}");
        }
    }

    #[test]
    fn another_protocol_is_told_apart_from_garbage() {
        assert_eq!(Message::decode(b"LB3\tTL\tanything"), Err(DecodeError::OtherProtocol("LB3".into())));
        assert_eq!(Message::decode(b"LB3"), Err(DecodeError::OtherProtocol("LB3".into())));
        assert_eq!(Message::decode_versioned(b"LB1\tVERSION"), Ok((Protocol::Lb1, Message::Version)));
        assert_eq!(Message::decode_versioned(b"LB2\tVERSION"), Ok((Protocol::Lb2, Message::Version)));
        assert!(matches!(Message::decode(b"PS1\tPING\t1\t2"), Err(DecodeError::Malformed(_))));
        assert!(matches!(Message::decode(&[0xff, 0xfe]), Err(DecodeError::Malformed(_))));
        assert!(matches!(Message::decode(b""), Err(DecodeError::Malformed(_))));
        assert!(matches!(Message::decode(b"lb2\tVERSION"), Err(DecodeError::Malformed(_))));
    }

    #[test]
    fn protocols_are_named_and_ordered() {
        assert_eq!(Protocol::ALL.map(Protocol::name), ["LB1", "LB2"]);
        assert_eq!(Protocol::from_name("LB2"), Some(Protocol::Lb2));
        assert_eq!(Protocol::from_name("LB3"), None);
        assert!(Protocol::Lb1 < Protocol::Lb2 && Protocol::LATEST == Protocol::Lb2);
        assert_eq!(Protocol::Lb2.to_string(), "LB2");
    }

    #[test]
    fn malformed_fields_are_refused() {
        let bad = [
            &b"LB1"[..],
            b"LB1\tNOPE",
            b"LB2\tNOPE",
            b"LB1\tPING\t0123456789abcdef\t1",                     // a field missing
            b"LB1\tPING\t0123456789abcdef\t1\t2\t3",               // one too many
            b"LB1\tPING\t123\t1\t2",                               // short nonce
            b"LB1\tPING\t0123456789abcdeg\t1\t2",                  // not hex
            b"LB1\tPING\t0123456789abcdef\t-1\t2",                 // negative seq
            b"LB1\tTL\t0123456789abcdef\t1\tgo\t0\t0\t120\t4",     // state
            b"LB1\tTL\t0123456789abcdef\t1\tplay\t0\tNaN\t120\t4", // beat
            b"LB1\tTL\t0123456789abcdef\t1\tplay\t0\t0\tinf\t4",   // bpm
            b"LB1\tTL\t0123456789abcdef\t1\tplay\t0\t0\t-1\t4",    // negative bpm
            b"LB1\tTL\t0123456789abcdef\t1\tplay\t0\t0\t120\t17",  // quantum
            b"LB1\tVERSION\textra",
            // SESSION is not in LB1.
            b"LB1\tSESSION\t0123456789abcdef\t1\t0\t0\t120\t4\tplay\t1\t0",
            b"LB2\tSESSION\t0123456789abcdef\t1\t0\t0\t120\t4\tplay\t1", // a field missing
            b"LB2\tSESSION\t0123456789abcdef\t1\t0\t0\t120\t4\tplay\t1\t0\t0", // one too many
            b"LB2\tSESSION\t0123456789abcdef\t1\t0\tinf\t120\t4\tplay\t1\t0", // beat
            b"LB2\tSESSION\t0123456789abcdef\t1\t0\t0\t-120\t4\tplay\t1\t0", // negative bpm
            b"LB2\tSESSION\t0123456789abcdef\t1\t0\t0\t120\t0\tplay\t1\t0", // quantum 0
            b"LB2\tSESSION\t0123456789abcdef\t1\t0\t0\t120\t17\tplay\t1\t0", // quantum
            b"LB2\tSESSION\t0123456789abcdef\t1\t0\t0\t120\t4\t1\t1\t0", // playing
            b"LB2\tSESSION\t0123456789abcdef\t1\t0\t0\t120\t4\tplay\t-1\t0", // peers
            b"LB2\tSESSION\t0123456789abcdef\t1\t0\t0\t120\t4\tplay\t1\t2", // publishing
            b"LB2\tSESSION\t0123456789abcdef\t1\t0\t0\t120\t4\tplay\t1\ttrue", // publishing
        ];
        for d in bad {
            assert!(matches!(Message::decode(d), Err(DecodeError::Malformed(_))), "{}", String::from_utf8_lossy(d));
        }
        let long = [b'a'; MAX_LEN + 1];
        assert_eq!(Message::decode(&long), Err(DecodeError::Malformed("too long".into())));
    }

    #[test]
    fn the_longest_message_fits() {
        // The longest real values: a beat a billion in either way (an 8-year track), with full precision.
        let t = Timeline {
            nonce: u64::MAX,
            seq: u32::MAX,
            at_us: i64::MIN,
            beat: -1.000_000_000_123_456_7e9,
            bpm: 999.999_999_999_999_9,
            ..tl()
        };
        assert!(Message::Timeline(t).encode().len() <= MAX_LEN);
        let p =
            Message::Pong { nonce: u64::MAX, seq: u32::MAX, sender_us: i64::MIN, bridge_us: i64::MIN, peers: u32::MAX };
        assert!(p.encode().len() <= MAX_LEN);
        let s = Session {
            nonce: u64::MAX,
            seq: u32::MAX,
            at_us: i64::MIN,
            beat: -1.000_000_000_123_456_7e9,
            bpm: 999.999_999_999_999_9,
            quantum: MAX_QUANTUM,
            playing: None,
            peers: u32::MAX,
            publishing: true,
        };
        assert!(Message::Session(s).encode().len() <= MAX_LEN);
    }
}
