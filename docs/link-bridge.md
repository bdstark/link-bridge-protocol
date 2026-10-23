# Service ↔ Link bridge protocol (LB1)

How `gma3-phrase-sync` hands the selected deck's tempo and beat to a **Link bridge**: a separate program, on the same
host, that publishes them to a Link session. The bridge is built on Ableton's Link SDK and is licensed
GPL-2.0-or-later; it lives in the gma3-beat-sync repository. The service is MIT and contains no Link code: the two
are separate programs that only exchange these messages.

This document and the `link-bridge-protocol` crate (MIT, in this repository) are the whole contract. The crate
encodes and decodes every message below and knows nothing about Link, so both sides depend on it rather than each
writing its own copy.

## Revisions

| Protocol | Service | Adds |
| --- | --- | --- |
| `LB1` | PS-11b | Clock sync, timelines, goodbye. |

## Transport

UDP on loopback, one message per datagram, at most 512 bytes. A message is UTF-8 text: fields separated by a single
tab, no newline. The first field is the protocol (`LB1`), the second the message type.

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
| bridge → service | `LB1 PONG <nonce> <seq> <service-us> <bridge-us> <peers>` | Sent on receipt, with the ping's first three fields echoed. `bridge-us` is the Link clock when the ping arrived, `peers` the number of other Link peers the bridge sees. |

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

## What the bridge does

1. **Answers every `PING`** at once with a `PONG`, reading its Link clock when the ping arrives.
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
7. **Answers another protocol** (a first field like `LB2`) with `VERSION` in its own protocol, so the other side can
   report the mismatch: at most once a second, and never in answer to a `VERSION`, so two sides on different
   protocols can't answer each other forever. Anything malformed is ignored. The service does the same with a
   bridge's messages.

## Example

```
service → bridge   LB1  PING  000001a2b3c4d5e6  1  1500
bridge → service   LB1  PONG  000001a2b3c4d5e6  1  1500  90001520  2
service → bridge   LB1  TL    000001a2b3c4d5e6  1  play  90002000  31.5  126  4
```

The service's clock read 1500 µs when it pinged and 1540 µs when the pong came back (a 40 µs round trip), so the
bridge's clock is 90 000 000 µs ahead. At 90 002 000 µs on the bridge's clock the deck is half way through beat 31,
at 126 BPM, four beats to the bar, so the next bar starts at beat 32.

## Not in LB1

- More than one service per bridge: the latest run wins.
- Status from the bridge beyond `peers`, and anything going from the bridge into the service's timeline (Link
  tempo changes made by other apps don't reach the decks or the demo).
