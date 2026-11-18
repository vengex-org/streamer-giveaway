# streamer-giveaway

A small desktop app for Twitch giveaways. It reads a channel's chat (read-only, no login), adds
everyone who types the giveaway keyword, counts each person once and draws a winner. Subs, VIPs and
mods can get more tickets than regular viewers.

Built with Rust and [GPUI](https://gpui.rs) through [gpui-kit](https://github.com/longbridge/gpui-kit).

## Build and run

Needs Rust stable (`rustup`). On Windows also the MSVC build tools and a Windows SDK.

```sh
cargo run --release
```

The first build takes several minutes.

## Use

1. Type the channel name and press Connect. The dot turns green once the chat is joined. If the
   connection fails, the reason is shown next to the dot.
2. Set the keyword (default `YEP`) and how it has to appear:
   - **Word**: the keyword as a separate word anywhere in the message (`YEP lets go` counts, `YEPPERS` does not).
   - **Whole message**: the message is only the keyword.
   - **Contains**: the keyword anywhere, also inside other words.

   Case never matters. A new keyword takes effect when you press Enter, leave the field, switch
   the mode or open entries, never while you are still typing.
3. Press **Open entries**. Everyone whose message matches is added once. The broadcaster is never added.
4. Set the tickets per role (Pleb, Sub T1/T2/T3, VIP, Mod). A chatter with several roles gets the
   highest number. Weight 0 keeps that role out: new messages do not add them, and people already in
   the list stay there but cannot win.
5. Press **Close entries**, then **Draw winner**. The winner leaves the pool and cannot enter again,
   so the next draw picks someone else. **Reset** empties the list and forgets past winners.

A chatter's role is refreshed by each of their later messages, so a viewer who subscribes after
entering gets the sub weight.

Settings (channel, keyword, mode, weights) are saved to `%APPDATA%\yep-giveaway\config\config.toml`
on Windows, or to the platform's config directory elsewhere. Entrants are not saved.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

`src/domain` holds the giveaway rules without UI or IO. `src/chat.rs` runs the Twitch connection on
its own thread, and `src/ui.rs` is the window.
