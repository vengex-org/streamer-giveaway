# streamer-giveaway

A desktop app for chat giveaways on Twitch. It reads your channel's chat (read-only, no login),
adds everyone who types the keyword, counts each person once and draws a winner. Subs, VIPs and
mods can get extra tickets. After the draw it shows how long the winner takes to answer and what
they write in chat.

The window is meant to be shown on stream: large keyword, entrant count and winner name, a short
reveal when drawing, dark and light theme. The app speaks Slovak, English, Czech and Spanish and
starts in your system's language and theme.

Built with Rust and [GPUI](https://gpui.rs) through [gpui-kit](https://github.com/longbridge/gpui-kit).

## Build and run

Needs Rust stable (`rustup`). On Windows also the MSVC build tools and a Windows SDK.

```sh
cargo run --release
```

The first build takes several minutes.

## Use

The left column walks you through four steps; the next one is highlighted.

1. **Connect**: type your Twitch channel name and press Connect. The header shows when the chat is
   read, or why the connection failed.
2. **Keyword**: type the word viewers have to write and press Enter. Pick how it must appear:
   - **Word**: as a separate word anywhere in the message.
   - **Whole message**: the message is only the keyword.
   - **Contains**: anywhere, also inside other words.

   Case never matters. Two optional sections sit under the keyword:
   - **Bonus tickets**: tickets per role (viewer, sub tier 1/2/3, VIP, moderator). More tickets,
     bigger chance. 0 keeps that group out. A chatter with several roles gets the highest number.
   - **Who can enter**: only subscribers, and/or only people who wrote something other than the
     keyword in the last N minutes or hours. Activity counts from when the app connected. People
     who miss a condition are left out quietly.
3. **Open entries**: everyone whose message matches is added once. The broadcaster is never added.
4. **Draw**: entries close and names flash for a moment before the winner shows. A stopwatch runs
   until the winner writes in chat; their messages appear below. **Draw another** picks someone
   else (a winner cannot enter again), **Done** goes back to the list. **Clear list** removes
   everyone and forgets past winners.

Settings (channel, keyword, mode, tickets, conditions, language, theme) are saved to
`%APPDATA%\streamer-giveaway\config\config.toml` on Windows, or to the platform's config directory
elsewhere. Settings from the older yep-giveaway app are taken over once.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

- `src/domain` holds the giveaway rules without UI or IO.
- `src/chat.rs` runs the Twitch connection on its own thread.
- `src/ui/` is the window: `steps.rs` (left column), `stage.rs` (what viewers see), `header.rs`,
  `palette.rs` (colors for both themes).
- Texts live in `locales/app.toml`; a test fails when a language is missing or a UI string is not
  translated.
