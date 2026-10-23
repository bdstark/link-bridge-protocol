// SPDX-License-Identifier: MIT

//! The messages between `gma3-phrase-sync` and a Link bridge: protocol LB1, specified in `docs/link-bridge.md`.
//!
//! The service sends the selected deck's timeline (tempo, a beat and the time it falls at, playing or stopped); the
//! bridge, a separate program, publishes it to a Link session. This crate only encodes and decodes the messages. It
//! has no Link code and no sockets, so both sides use it, whatever their licence: the bridge depends on it, never the
//! other way.
//!
//! A message is one UDP datagram of tab-separated UTF-8 text, at most [`MAX_LEN`] bytes, whose first field is the
//! protocol, [`PROTOCOL`].

use std::fmt;

/// The protocol this crate speaks, the first field of every message.
pub const PROTOCOL: &str = "LB1";
/// The longest message, in bytes.
pub const MAX_LEN: usize = 512;
/// The longest bar a timeline may count beats in.
pub const MAX_QUANTUM: u8 = 16;

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

/// One LB1 message.
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
    /// Either way: the answer to a message in another protocol, naming this side's.
    Version,
}

/// Why a datagram isn't a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// A message in another protocol (its first field, for example `LB2`).
    OtherProtocol(String),
    /// Not an LB1 message, or a malformed one.
    Malformed(String),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::OtherProtocol(p) => write!(f, "speaks {p}, not {PROTOCOL}"),
            DecodeError::Malformed(why) => write!(f, "malformed message: {why}"),
        }
    }
}

impl std::error::Error for DecodeError {}

impl Message {
    /// The datagram.
    pub fn encode(&self) -> Vec<u8> {
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
            Message::Version => "VERSION".to_owned(),
        };
        format!("{PROTOCOL}\t{fields}").into_bytes()
    }

    /// The message in a datagram.
    pub fn decode(data: &[u8]) -> Result<Message, DecodeError> {
        let bad = |why: &str| DecodeError::Malformed(why.to_owned());
        if data.len() > MAX_LEN {
            return Err(bad("too long"));
        }
        let text = std::str::from_utf8(data).map_err(|_| bad("not UTF-8"))?;
        let mut f = text.split('\t');
        let protocol = f.next().unwrap_or_default();
        if protocol != PROTOCOL {
            return Err(if protocol.starts_with("LB") && protocol.len() <= 8 {
                DecodeError::OtherProtocol(protocol.to_owned())
            } else {
                bad("not LB1")
            });
        }
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
        match kind {
            "PING" => {
                want(3)?;
                Ok(Message::Ping {
                    nonce: nonce(rest[0])?,
                    seq: uint(rest[1], "seq")?,
                    sender_us: int(rest[2], "time")?,
                })
            }
            "PONG" => {
                want(5)?;
                Ok(Message::Pong {
                    nonce: nonce(rest[0])?,
                    seq: uint(rest[1], "seq")?,
                    sender_us: int(rest[2], "sender time")?,
                    bridge_us: int(rest[3], "bridge time")?,
                    peers: uint(rest[4], "peers")?,
                })
            }
            "TL" => {
                want(7)?;
                let state = match rest[2] {
                    "play" => State::Play,
                    "stop" => State::Stop,
                    other => return Err(DecodeError::Malformed(format!("state: {other:?}"))),
                };
                let bpm = real(rest[5], "bpm")?;
                if bpm < 0.0 {
                    return Err(bad("bpm is negative"));
                }
                let quantum = rest[6].parse::<u8>().ok().filter(|q| *q <= MAX_QUANTUM).ok_or_else(|| bad("quantum"))?;
                Ok(Message::Timeline(Timeline {
                    nonce: nonce(rest[0])?,
                    seq: uint(rest[1], "seq")?,
                    state,
                    at_us: int(rest[3], "time")?,
                    beat: real(rest[4], "beat")?,
                    bpm,
                    quantum,
                }))
            }
            "BYE" => {
                want(1)?;
                Ok(Message::Bye { nonce: nonce(rest[0])? })
            }
            "VERSION" => {
                want(0)?;
                Ok(Message::Version)
            }
            other => Err(DecodeError::Malformed(format!("unknown message type {other:?}"))),
        }
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

    #[test]
    fn every_message_reads_back_as_sent() {
        let all = [
            Message::Ping { nonce: NONCE, seq: 1, sender_us: -5 },
            Message::Pong { nonce: NONCE, seq: 1, sender_us: -5, bridge_us: 9_000_000_000, peers: 3 },
            Message::Timeline(tl()),
            Message::Timeline(Timeline { state: State::Stop, bpm: 0.0, quantum: 0, ..tl() }),
            Message::Bye { nonce: NONCE },
            Message::Version,
        ];
        for m in all {
            assert_eq!(Message::decode(&m.encode()), Ok(m), "{}", String::from_utf8_lossy(&m.encode()));
        }
    }

    #[test]
    fn the_wire_form_is_tab_separated_text() {
        assert_eq!(
            Message::Timeline(tl()).encode(),
            b"LB1\tTL\t0123456789abcdef\t7\tplay\t1234567890\t64.12345678901234\t126.000001\t4"
        );
        assert_eq!(Message::Ping { nonce: 1, seq: 2, sender_us: 3 }.encode(), b"LB1\tPING\t0000000000000001\t2\t3");
        assert_eq!(Message::Version.encode(), b"LB1\tVERSION");
    }

    #[test]
    fn another_protocol_is_told_apart_from_garbage() {
        assert_eq!(Message::decode(b"LB2\tTL\tanything"), Err(DecodeError::OtherProtocol("LB2".into())));
        assert_eq!(Message::decode(b"LB2"), Err(DecodeError::OtherProtocol("LB2".into())));
        assert!(matches!(Message::decode(b"PS1\tPING\t1\t2"), Err(DecodeError::Malformed(_))));
        assert!(matches!(Message::decode(&[0xff, 0xfe]), Err(DecodeError::Malformed(_))));
        assert!(matches!(Message::decode(b""), Err(DecodeError::Malformed(_))));
    }

    #[test]
    fn malformed_fields_are_refused() {
        let bad = [
            &b"LB1"[..],
            b"LB1\tNOPE",
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
    }
}
