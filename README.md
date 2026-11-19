# Streamer Giveaway

A desktop app for Twitch chat giveaways. Viewers enter by typing a keyword in chat, the app counts
each person once, and you draw a winner on stream. You don't need a bot account, a login or a
browser source.

![Drawing a winner and waiting for their answer](docs/winner-draw.gif)

## Features

- The app reads your chat anonymously after you type your channel name. It never writes to chat.
- Each person enters once, however many times they post. The keyword can be required as a word, as
  the whole message, or anywhere in the message.
- Subs (tier 1, 2 and 3), VIPs and moderators can get more tickets than regular viewers. A group
  with 0 tickets cannot enter.
- You can let in only subscribers, or only people who chatted in the last few minutes or hours, so
  people who show up just for the giveaway don't count.
- The window is built to be captured on stream: a large keyword and entrant count, a short reveal
  when you draw, and a winner panel with a stopwatch that stops when the winner answers in chat.
- While entries are open, a line under the counters shows chat messages per second, new entrants
  in the last minute and how long ago the last one came in, so you can see when entries dry up.
  When entries close, the app saves a CSV log with the time and kind of every message (no message
  text) for measuring the giveaway afterwards.
- Five numbered steps with a short hint guide first-time users. The next step is highlighted, and
  a button you cannot press yet says why. The Rules step always shows who can enter and how many
  tickets each group gets, so you don't have to look for them.
- There is a dark and a light theme, and the app speaks English, Slovak, Czech and Spanish. Both
  follow your system settings until you pick one, and "Auto" or "System" goes back to that.

| Entries coming in | Winner answered | Editing the rules, light theme |
|---|---|---|
| ![Entries](docs/entries-dark.png) | ![Winner](docs/winner-dark.png) | ![Rules](docs/conditions-light.png) |

## Try it without a stream

```sh
cargo run --release -- --demo
```

Demo mode replaces Twitch with a simulated chat on `#pixelpaladin`: 6000 messages over three
minutes from about 2400 chatters, most of them typing `GIVEME`. The drawn winner answers a few
seconds after the draw. Demo settings are stored separately and never touch your real ones.

To rehearse a big giveaway, give the audience size instead:

```sh
cargo run --release -- --demo-viewers=8000
```

By default 40 % of the viewers enter, half of them within 20 seconds of the announcement, with
15 other chat messages per second around them. `--demo-share=0.6`, `--demo-half-life=45`,
`--demo-chat=30` and `--demo-cap=100` (Twitch delivering at most 100 messages per second) change
that. These defaults are guesses; the CSV log of a real giveaway shows the real values.

## Download

Windows builds are on the [releases page](https://github.com/vengex-org/streamer-giveaway/releases).
The .exe is not signed, so Windows SmartScreen may warn before the first start ("More info", then
"Run anyway").

## Privacy

The app sends nothing anywhere unless you ask it to. When you connect to a channel it reads that
channel's Twitch chat anonymously. Settings and giveaway logs stay on your computer.

## Build and run

You need [Rust](https://rustup.rs) (stable). On Windows also the Visual Studio C++ build tools and a
Windows SDK; on Linux the GPUI system packages (see the
[GPUI docs](https://github.com/zed-industries/zed/tree/main/crates/gpui)).

```sh
git clone https://github.com/vengex-org/streamer-giveaway.git
cd streamer-giveaway
cargo run --release
```

The first build takes several minutes.

## How to run a giveaway

1. Type your Twitch channel name and press Connect.
2. Type the word viewers should write and press Enter.
3. Check the rules. "Edit rules" lets you limit who can enter and give subs, VIPs or moderators
   more tickets.
4. Press "Open entries" and tell chat the keyword.
5. Press "Draw winner". Entries close, names flash for a moment, and then the winner shows with a
   stopwatch. When they write in chat, their messages appear in the panel. "Draw another" picks
   someone else, since a winner cannot win twice. "Clear list" starts over.

Settings are saved to `%APPDATA%\streamer-giveaway\config\config.toml` on Windows and to the
platform's config folder elsewhere. Giveaway logs go to `%APPDATA%\streamer-giveaway\data\logs`.
Entrants are not saved.

## Development

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

- `src/domain` holds the giveaway rules, with no UI or IO.
- `src/chat.rs` reads Twitch chat on its own thread; `src/demo.rs` is the simulated chat.
- `src/ui/` is the window: `steps.rs` (left column), `stage.rs` (the part viewers see),
  `header.rs`, `palette.rs` (colors for both themes).
- `cargo run --release --example chat_rate -- <channel> 60` measures a live chat for 60 seconds
  (messages per second, peak, unique chatters). `cargo run --release --example raffle_sim` prints
  how long giveaways take for several audience sizes and delivery caps.
- Texts live in `locales/app.toml`. A test fails when a language is missing or when UI code passes
  text that is not translated.

Built with [GPUI](https://gpui.rs) and [gpui-kit](https://github.com/longbridge/gpui-kit).

## License

[MIT](LICENSE)
