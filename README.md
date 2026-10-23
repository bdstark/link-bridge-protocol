# link-bridge-protocol

The messages between gma3-phrase-sync (a private project for now) and a **Link bridge**: protocol
LB1. The service sends a DJ deck's timeline (tempo, a beat and the time it falls at, playing or stopped) over UDP on
loopback; the bridge, a separate program built on Ableton's Link SDK, publishes it to a Link session.

This crate only encodes and decodes the messages. It has no Link code and no sockets, so programs under any licence
can use it: the MIT service and the GPL-2.0-or-later bridge in
[gma3-beat-sync](https://github.com/bdstark/gma3-beat-sync) both depend on it.

The protocol is specified in [docs/link-bridge.md](docs/link-bridge.md).

```toml
[dependencies]
link-bridge-protocol = { git = "https://github.com/bdstark/link-bridge-protocol.git", rev = "<commit>" }
```

```rust
use link_bridge_protocol::Message;

let ping = Message::Ping { nonce: 1, seq: 1, sender_us: 1500 };
assert_eq!(Message::decode(&ping.encode()), Ok(ping));
```

## License

MIT. See [LICENSE](LICENSE).

Ableton and Link are trademarks of Ableton AG. This project is not affiliated with or endorsed by Ableton.
