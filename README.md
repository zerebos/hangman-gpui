# hangman-gpui

A Rust + [GPUI](https://www.gpui.rs/) port of [`zerebos/Hangman`](https://github.com/zerebos/Hangman),
a 2015 Java/Swing hangman game. Same rules, same word lists, same artwork, same
layout, the same two sound cues and the same (slightly unhinged) alert messages
— rebuilt with [gpui-kit](https://gpui-kit.com) instead of Swing.

```
┌────────────────────────────────────────────────────────┐
│ Hangman!                                               │
│ Difficulty: [Easy][Medium][Hard][Insane]  [Change Word]│
├──────────────────────────────┬─────────────────────────┤
│ Hangman!                     │        ┌────────┐       │
│ You WIN!                     │        │        │       │
│ Wins: 3  Losses: 1           │        │       (o)      │
│ Word:  A P P L E             │        │        |       │
│ Letters Available:           │       ─┴─      / \      │
│ [A][B][C][D][E][F][G]        │      ═══════            │
│ ...                          │                         │
│ [ New Game? ]                │                         │
└──────────────────────────────┴─────────────────────────┘
```

The crate is a lib + bin: [`src/game.rs`](src/game.rs) is the pure, UI-free rule
engine (with 26 unit tests), and [`src/ui/`](src/ui/) is everything GPUI. The
word lists, the original's gallows artwork and its two mp3 cues live in
[`assets/`](assets/) and are compiled into the binary, so there is nothing to
install next to the executable.

## Building and running

```sh
cargo run
```

### Toolchain: beta, and why

`rust-toolchain.toml` pins the toolchain to `beta`, and you need it. GPUI ships
to crates.io as `gpui-pre`, whose `src/profiler.rs` calls `std::hint::cold_path()`.
That function is still unstable on current stable rustc (1.94.1), so a stable
build fails with:

```
error[E0658]: use of unstable library feature `cold_path`
```

`rustup toolchain install beta` once and the pin takes care of the rest. If your
setup ignores `rust-toolchain.toml`, use `cargo +beta run`.

### Linux system libraries

`cargo check` passes with nothing installed, but **linking** needs the X11
keyboard libraries. Without them the build dies at the very end with
`rust-lld: error: unable to find library -lxkbcommon`. On Debian/Ubuntu:

```sh
sudo apt install -y libxkbcommon-dev libxkbcommon-x11-dev
```

You will most likely already have `libfontconfig-dev` and `libfreetype-dev`; if
not, add them too. Wayland and Vulkan are `dlopen`ed at run time rather than
linked, so they are only needed to actually display the window.

## Sound: the `sound` feature

The original played `win.mp3` when you guessed the word and `loss.mp3` when the
drawing was finished. Both are ported, but behind a cargo feature named `sound`
that is **off by default**, so a plain `cargo run` needs nothing new — it builds
and runs exactly as it did before, just silently.

To turn the cues on:

```sh
cargo run --features sound
```

The reason it is opt-in is that on Linux it makes the **ALSA headers a
build-time requirement**. The feature pulls in [rodio], which pulls in `cpal`,
whose `alsa-sys` build script shells out to `pkg-config` for them. If they are
missing that build script *panics*, and cargo fails the **whole crate** — not
just the audio — so `cargo build`, `cargo test` and even `cargo check` would
stop working for anyone who only cares about the game. Hence the flag. Install
them first:

```sh
sudo apt install -y libasound2-dev      # Debian / Ubuntu
sudo dnf install -y alsa-lib-devel      # Fedora
```

On a PipeWire-only machine this still works at run time: `cpal` has no PipeWire
backend, but PipeWire's ALSA compatibility plugin (`pipewire-alsa`) is installed
by default on every mainstream distro that ships PipeWire, and rodio goes
through that. The headers are needed to *build* either way.

If the feature is enabled but no usable output device is found, the game says so
once on stderr and then just plays silently — it never fails to start. (The
message may be preceded by several lines of diagnostics from libasound itself;
those come from the C library, not from the game.)

[rodio]: https://crates.io/crates/rodio

## Controls

| Action | How |
| --- | --- |
| Guess a letter | Type it, or click its button |
| Next word after a game ends | `New Game?` button, or Enter / Space |
| Give up on the current word (counts as a loss) | `Change Word` button, or `Ctrl+N` |
| Load your own word list | `Open word list…` button, or `Ctrl+O` |
| Change difficulty (starts a fresh match) | The Easy / Medium / Hard / Insane buttons |
| Quit | Close the window |

A word list is a plain `.txt` file with one word per line. Lines are trimmed and
upper-cased, and lines with no letters in them are dropped.

A *match* is one pass through the whole word list — ten words for the bundled
lists, drawn at random without repeats. When the list runs out the match is
scored (more wins than losses, fewer, or a tie) and you pick a difficulty or a
new word list to start over.

## Differences from the original

- **The serial / LCD layer is gone.** The original could drive an external PS/2
  keyboard and a character LCD over RS-232, which changed the window size, swapped
  the letter grid for read-only labels and added a whole `Setup` menu. None of
  that is ported.
- **Keyboard input was added.** The original had no key listener at all: in
  standalone mode you could only click the letter buttons. Here, typing a letter
  guesses it.
- **The menu bar became a toolbar.** gpui-kit has no menu-bar component, so the
  original's `Game` menu (Open File…, Change Word, Difficulty ▸, Exit) is a button
  strip under the title bar. `Ctrl+O` and `Ctrl+N` still work.
- **Sound is opt-in.** The original played `win.mp3` / `loss.mp3` through JavaFX,
  unconditionally. The same two cues are here, but only when built with
  `--features sound`, because the audio dependency makes the ALSA headers a
  build-time requirement on Linux. See [Sound: the `sound` feature](#sound-the-sound-feature).
- **A game-result bug is fixed.** On the *final* word of a match, the original
  only ever announced the match result and silently skipped the win/loss message
  (and the sound) for that game. Here both fire: the alert line shows
  `You WIN!` / `Bring Add/Drop Form!`, the cue plays, and the footer shows the
  match summary.
- **The window resizes.** The original was a fixed, non-resizable 800×400.
- **Small addition:** a muted `Word n of 10` counter next to the score, so you can
  tell how much of a match is left.

## Roadmap

The port has caught up with the Java original, so from here the game stops
mirroring it. These are the nine ideas agreed for where it goes next, roughly in
the order they were argued about rather than in any committed order.

### Game and rules

1. **Hints, at a cost.** Reveal an unguessed letter in exchange for a wrong
   guess, or out of a small per-match budget.
2. **Scoring and streaks.** Retire the bare `wins` / `losses` counters in favour
   of points per word, plus a current and a best streak.
3. **Structured word packs.** Move the four ten-word lists into a serde format
   that carries a category, a hint and a clue per word, so a match no longer
   exhausts the pool.
4. **Difficulty that changes the guess budget.** `MAX_WRONG_GUESSES` is a hard 6
   today. Blocked on item 6: the seven artwork frames assume exactly six.

### UI and UX

5. **Animation and game feel** *(in progress).* Cross-fade the gallows, shake the
   word on a wrong guess, stagger-reveal the letters on a win, and pulse the
   wrong-guess pips.
6. **Draw the gallows procedurally**, with `canvas()` and `PathBuilder`, so it
   scales, follows the theme and copes with any guess budget. The trade-off is
   real: it retires the bundled artwork.
7. **Keyboard hints.** Surface the shortcuts in the window itself with gpui-kit's
   `Kbd` and `Tooltip::action`.

### Craft

8. **Persist settings and stats.** Nothing is written to disk today, so the theme
   choice and the window geometry reset on every launch.
9. **Make the UI testable.** Pull the pure helpers out of
   [`src/ui/mod.rs`](src/ui/mod.rs) — which has no tests at all — and cover them.

**Not planned:** networked multiplayer — the original's external layer is the one
thing the port deliberately dropped, and this would only bring it back — fetching
words from a dictionary API, and custom JSON themes with hot reload.

## Credits

Original game © Zack Rauen ([zerebos](https://github.com/zerebos)), 2015.
