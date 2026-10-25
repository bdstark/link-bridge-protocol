# Service ↔ Link bridge protocol (LB1, LB2)

How `gma3-phrase-sync` hands the selected deck's tempo and beat to a **Link bridge**: a separate program, on the same
host, that publishes them to a Link session, and (LB2) how the bridge reports the session back. The bridge is built
on Ableton's Link SDK and is licensed GPL-2.0-or-later; it lives in the gma3-beat-sync repository. The service is
MIT and contains no Link code: the two are separate programs that only exchange these messages.

This document and the `link-bridge-protocol` crate (MIT, in this repository) are the whole contract. The crate
encodes and decodes every message below and knows nothing about Link, so both sides depend on it rather than each
writing its own copy.

## Revisions

| Protocol | Service | Adds |
| --- | --- | --- |
| `LB1` | PS-11b | Clock sync, timelines, goodbye. |
| `LB2` | AU-5 | The bridge reports the Link session to the service (`SESSION`), so the service can use Link's tempo and bar when no deck plays. |

Each revision has every message of the ones before it, written the same way after the first field. See
[Revisions on the wire](#revisions-on-the-wire) for which one each side writes.

## Transport

UDP on loopback, one message per datagram, at most 512 bytes. A message is UTF-8 text: fields separated by a single
tab, no newline. The first field is the protocol (`LB1` or `LB2`), the second the message type.

The bridge listens on a loopback address and port (7822 by default) and accepts datagrams only from loopback
addresses. The service sends from its own loopback socket, and the bridge replies to the address a datagram came
from. Both ends must be on loopback: the service's configuration refuses anything else.

## Times

Each side has its own clock in whole microseconds: the service's monotonic clock since it started, and the
bridge's **Link clock**, the clock the bridge gives the SDK for its timestamps. The service measures the offset
between them (below) and sends every time **on the bridge's clock**, so the bridge never converts anything and the
two programs never have to read the same clock.

What crosses the socket is a timeline (a beat and the time it falls at), never a tick on each beat, so the time a
datagram takes to arrive doesn't move the beat.

## Clock

| Direction | Message | Meaning |
| --- | --- | --- |
| service → bridge | `LB1 PING <nonce> <seq> <service-us>` | Sent every 100 ms until the service has 4 samples, then every second. |
| bridge → service | `LB1 PONG <nonce> <seq> <service-us> <bridge-us> <peers>` | Sent on receipt, with the ping's first three fields echoed, in the ping's protocol. `bridge-us` is the Link clock when the ping arrived, `peers` the number of other Link peers the bridge sees. |

`nonce` is 16 hex digits, one per service run. Both sides read their clocks at the moment a datagram goes or arrives, not when they next get round to it: a
reading taken late lands half its delay in the offset. (The service receives on a thread of its own and notes when
each datagram arrived.) For each pong the service computes the round trip
`rtt = now - service-us` and the offset `bridge-us - (service-us + rtt / 2)`, and uses the offset from the pong with
the smallest round trip among the latest 16: a short round trip bounds the error by half of it, a few tens of
microseconds on loopback. A pong with a round trip over 100 ms, or another run's nonce, is not a sample.

After 3 s without a pong the service logs that the bridge stopped answering, drops its clock and sends no timelines
until the bridge answers again.

## Timelines

| Direction | Message | Meaning |
| --- | --- | --- |
| service → bridge | `LB1 TL <nonce> <seq> <state> <at-us> <beat> <bpm> <quantum>` | The selected deck's timeline. |
| service → bridge | `LB1 BYE <nonce>` | The service is going away. |

- `seq` rises by one with each timeline of a run.
- `state` is `play` while the deck plays forward under its own power, and `stop` when it is paused, cued, scratched
  or reversed, or no deck is selected: nothing should be predicted from a stopped timeline.
- At `at-us` on the bridge's clock the deck is at `beat` (fractional). With `quantum` beats to the bar (1-16), every
  beat that is a multiple of `quantum` is a bar's first beat. `quantum` 0 means the bar's phase isn't known (a track
  with no phrases to count bars from): the timeline gives the tempo only.
- `bpm` is the playing tempo (pitch included), or 0 when it isn't known.
- Numbers are decimal; `beat` and `bpm` are written as the shortest text that reads back as the same 64-bit float.

The service sends a timeline only once it has a clock, and then:
- at once when the state, the quantum or the tempo (to 0.001 BPM) changes;
- when the predicted time of a beat moves by more than 1 ms, at most every 50 ms;
- at least every second, as a repeat, since a datagram may be lost.

It sends `BYE` when it shuts down, if the bridge has answered.

## Session (LB2)

| Direction | Message | Meaning |
| --- | --- | --- |
| bridge → service | `LB2 SESSION <nonce> <seq> <bridge-us> <beat> <bpm> <quantum> <playing> <peers> <publishing>` | The Link session as the bridge sees it. |

- `nonce` is the run the report is for: the `nonce` of the LB2 pings it answers.
- `seq` counts the reports of one subscription (below) from 1: it starts at 1 again when a run subscribes again or
  the bridge restarts. A `SESSION` with `seq` 1 is always taken; any other at or below the last one taken is late and
  ignored.
- At `bridge-us` on the bridge's clock the session is at `beat` (fractional), counted in bars of `quantum` beats, at
  `bpm`. As with any Link peer, the phase in the bar (`beat` modulo `quantum`) is the session's and the count of whole
  bars is the bridge's own: compare phases, not beat numbers.
- `quantum` (1-16) is the bridge's setting, 4 by default. Link doesn't share a quantum: every peer picks its own.
- `playing` is `play` or `stop` when the bridge shares start/stop with the session, and `unknown` when it doesn't (it
  then doesn't hear the other peers' start and stop).
- `peers` is the number of other Link peers, as in `PONG`.
- `publishing` is `1` while the session's tempo or phase is this run's own: in the last 3 s the bridge took one of
  this run's timelines that sets the tempo (`bpm` isn't 0) or the phase (`play` with a `quantum`), and the run hasn't
  said `BYE`. Otherwise `0`. A `stop` timeline with `bpm` 0 sets nothing, so it doesn't count.
- Numbers are decimal; `beat` and `bpm` are written as the shortest text that reads back as the same 64-bit float.

The bridge sends `SESSION` only to its **subscriber**: the address and run of the latest LB2 `PING`. An LB2 ping
from another run takes over (the latest run wins); LB1 pings never subscribe. The subscription ends after 3 s
without an LB2 ping from it, or when its run says `BYE`. To the subscriber, the bridge sends a `SESSION`:
- after each `PONG` to the subscriber's pings (the pong first, so the clock sample isn't delayed);
- when the session changes: the tempo by more than 0.001 BPM, the phase by more than 1 ms from where the last
  `SESSION` puts it (at that one's tempo), or `quantum`, `playing`, `peers` or `publishing`;
- at most 20 a second (50 ms apart). One held back by the limit goes as soon as the limit allows, with the session
  as it is then.

The bridge reads the session in its own loop, never between a datagram's arrival and its reading of the clock for
that datagram.

The service puts `bridge-us` on its own clock with the offset it measured, takes a `SESSION` only while it has a
clock, and treats the session as unknown when it has no clock or its latest `SESSION` is more than 3 s old. A session
with `publishing` 1 is the service's own timeline coming back, not another app's.

## Revisions on the wire

The service writes all its messages in one revision; the bridge answers each message in that message's revision.
Each side reads every revision it speaks.

- **The bridge** answers in the protocol of the message it answers: a `PONG` to an `LB1 PING` is `LB1`, to an
  `LB2 PING` is `LB2`. It takes `TL` and `BYE` in either. It sends `SESSION` only to an LB2 subscriber, so an LB1
  service sees exactly LB1, unchanged.
- **An LB2 service** starts in LB2: its pings, timelines and goodbye are LB2. A bridge that speaks only LB1 answers
  an LB2 ping with `LB1 VERSION` (at most once a second) and no pong. On an `LB1 VERSION` the service falls back to
  LB1: it sends LB1 pings, timelines and goodbye from then on, logs once that the bridge has no session report, and
  has no session (Link is not a tempo source). It starts in LB2 again whenever it drops its clock (3 s without a pong:
  the bridge may have been restarted, perhaps a newer one). An `LB2 VERSION` (the bridge didn't understand
  something) is logged and changes nothing.
- **A message in a newer protocol** (a first field like `LB3`) is answered with `VERSION` in the newest protocol the
  receiver speaks, as in rule 7 below.

## What the bridge does

1. **Answers every `PING`** at once with a `PONG` in the ping's protocol, reading its Link clock when the ping
   arrives.
2. **Takes timelines in order.** A `TL` from the current run whose `seq` is at or below the last one applied is late
   and ignored. A `TL` with a new `nonce` is a new run (the service restarted) and replaces the old one.
3. **Publishes a `play` timeline:** the session's tempo becomes `bpm` (when it isn't 0), and when `quantum` isn't 0,
   the session's phase is aligned so that `beat` falls at `at-us` at that quantum. The bar's phase is what's
   aligned, not the session's beat count, so a jump in `beat` by whole bars (a new track) changes nothing for the
   other peers. If the bridge shares start/stop with the session, it is playing.
4. **A `stop` timeline** changes the tempo only if `bpm` isn't 0, and, if the bridge shares start/stop, stops the
   session.
5. **Applying the same timeline again is harmless**, which is what makes the repeats safe.
6. **Lets go** after `BYE`, or after 3 s without a timeline from the current run: it stops publishing and leaves the
   session's tempo and phase as they are. It stays in the session.
7. **Answers another protocol** (a first field like `LB3`) with `VERSION` in the newest protocol it speaks, so the
   other side can fall back or report the mismatch: at most once a second, and never in answer to a `VERSION`, so two
   sides on different protocols can't answer each other forever. Anything malformed is ignored. The service does the
   same with a bridge's messages.
8. **Reports the session** (LB2) to its subscriber, as in [Session](#session-lb2).

## Example

```
service → bridge   LB1  PING  000001a2b3c4d5e6  1  1500
bridge → service   LB1  PONG  000001a2b3c4d5e6  1  1500  90001520  2
service → bridge   LB1  TL    000001a2b3c4d5e6  1  play  90002000  31.5  126  4
```

The service's clock read 1500 µs when it pinged and 1540 µs when the pong came back (a 40 µs round trip), so the
bridge's clock is 90 000 000 µs ahead. At 90 002 000 µs on the bridge's clock the deck is half way through beat 31,
at 126 BPM, four beats to the bar, so the next bar starts at beat 32.

In LB2, with no deck playing:

```
service → bridge   LB2  PING     000001a2b3c4d5e6  1  1500
bridge → service   LB2  PONG     000001a2b3c4d5e6  1  1500  90001520  2
bridge → service   LB2  SESSION  000001a2b3c4d5e6  1  90001530  1031.5  124  4  unknown  2  0
service → bridge   LB2  TL       000001a2b3c4d5e6  1  stop  90002000  0  0  0
```

At 90 001 530 µs on the bridge's clock (1530 µs on the service's) the session is half way through the last beat of a
four-beat bar (1031.5 modulo 4 is 3.5), at 124 BPM, with two other peers. The bridge doesn't share start/stop, so
whether the session plays is unknown, and the tempo is not this service's (`publishing` 0): another app's, which the
service may follow. The stopped timeline with no tempo leaves the session alone.

The same service and a bridge that speaks only LB1:

```
service → bridge   LB2  PING     000001a2b3c4d5e6  1  1500
bridge → service   LB1  VERSION
service → bridge   LB1  PING     000001a2b3c4d5e6  2  101500
bridge → service   LB1  PONG     000001a2b3c4d5e6  2  101500  90101520  2
```

The service falls back to LB1 and has no session.

## Not in LB2

- More than one service per bridge: the latest run wins, for timelines and for the session report.
- A quantum chosen by the service for the session report: it is the bridge's setting.
- Whether the session plays, when the bridge doesn't share start/stop.
- Which peer changed the session, or the other peers' own state.
- Anything the bridge does with the session report: what the service makes of it (a tempo source, a check against
  what it hears) is the service's.
