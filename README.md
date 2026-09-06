# hangman-gpui

A Rust + [GPUI](https://www.gpui.rs/) port of [`zerebos/Hangman`](https://github.com/zerebos/Hangman),
a 2015 Java/Swing hangman game. Same rules, same word lists, same layout, the
same two sound cues and the same (slightly unhinged) alert messages — rebuilt
with [gpui-kit](https://gpui-kit.com) instead of Swing. The gallows is the one
thing that is not the original's: it is drawn line by line at run time rather
than shipped as pictures. See
[Differences from the original](#differences-from-the-original).

```
┌────────────────────────────────────────────────────────┐
│ Hangman!                                               │
│ Difficulty: [Easy][Medium][Hard][Insane]  [Change Word]│
├──────────────────────────────┬─────────────────────────┤
│ Hangman!                     │        ┌────────┐       │
│ You WIN! +330                │        │        │       │
│ Score: 940  Streak: 3        │        │       (o)      │
│ Word:  A P P L E             │        │        |       │
│ Letters Available:           │       ─┴─      / \      │
│ [A][B][C][D][E][F][G]        │      ═══════            │
│ ...                          │                         │
│ [ New Game? ]                │                         │
└──────────────────────────────┴─────────────────────────┘
```

The crate is a lib + bin: [`src/game.rs`](src/game.rs) is the pure, UI-free rule
engine (with 28 unit tests), [`src/stats.rs`](src/stats.rs) scores the words and
keeps the streak (25 more), [`src/settings.rs`](src/settings.rs) is the equally
UI-free file that remembers your choices between launches (25 more),
[`src/gallows.rs`](src/gallows.rs) is the gallows drawing as plain coordinates
(32 more), and [`src/ui/`](src/ui/) is everything GPUI. The word lists and the
two mp3 cues live in [`assets/`](assets/) and are compiled into the binary, so
there is nothing to install next to the executable.

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

## Settings

The theme, the window's size and position, and the difficulty you last picked
are remembered between launches. They are written to a small JSON file the
moment you change one of them — the window's geometry when you close it — in the
usual place for your platform:

| Platform | Path |
| --- | --- |
| Windows | `%APPDATA%\hangman-gpui\settings.json` |
| macOS | `~/Library/Application Support/hangman-gpui/settings.json` |
| Linux | `$XDG_CONFIG_HOME/hangman-gpui/settings.json`, or `~/.config/hangman-gpui/settings.json` |

```json
{
  "theme": "dark",
  "difficulty": "Medium",
  "window": {
    "rect": {
      "x": 460.0,
      "y": 160.0,
      "width": 1000.0,
      "height": 760.0
    },
    "maximized": false
  },
  "stats": {
    "points": 9210,
    "words_won": 31,
    "words_lost": 9,
    "streak": 3,
    "best_streak": 11,
    "matches_won": 3,
    "matches_lost": 1,
    "matches_tied": 0,
    "by_difficulty": {
      "Easy": { "points": 1650, "words_won": 10, "words_lost": 0,
                "matches_won": 1, "matches_lost": 0, "matches_tied": 0 },
      "Medium": { "points": 0, "words_won": 0, "words_lost": 0,
                  "matches_won": 0, "matches_lost": 0, "matches_tied": 0 },
      "Hard": { "points": 0, "words_won": 0, "words_lost": 0,
                "matches_won": 0, "matches_lost": 0, "matches_tied": 0 },
      "Insane": { "points": 7560, "words_won": 21, "words_lost": 9,
                  "matches_won": 2, "matches_lost": 1, "matches_tied": 0 }
    }
  }
}
```

`stats` is the lifetime tally behind the **Stats** button in the toolbar, and it
is as forgiving as the rest of the file: a `stats` key that is missing — every
settings file written before this feature existed — reads as an empty tally, a
`by_difficulty` name this version does not know is dropped, and one that holds
nonsense falls back to zeroes without costing you the theme or the window. Words
played from a word list of your own count in the totals and the streak but in
none of the four buckets, because they belong to no difficulty.

Nothing in there is required: delete the file, edit it by hand, or leave it on a
read-only disk, and the game falls back to its defaults — dark, centred, Easy —
saying so on stderr at worst. A saved window that no longer fits the monitors
you have is resized and moved back on screen rather than trusted, so unplugging
a second monitor can never strand the window somewhere you cannot reach it.

The lifetime stats — points, the streak, the best streak, and the win/loss
tally broken down by difficulty — are saved in the same file, and written the
moment a word ends. The score of the *match* you are playing is not: it belongs
to the match and starts again from zero when you pick a difficulty or load a new
word list. The streak deliberately does neither, which is the point of it.

## Controls

| Action | How |
| --- | --- |
| Guess a letter | Type it, or click its button |
| Next word after a game ends | `New Game?` button, or Enter / Space |
| Give up on the current word (counts as a loss) | `Change Word` button, or `Ctrl+N` |
| Load your own word list | `Open word list…` button, or `Ctrl+O` |
| Change difficulty (starts a fresh match) | The Easy / Medium / Hard / Insane buttons |
| Show the lifetime stats | The `Stats` button in the toolbar |
| Quit | Close the window |

A word list is a plain `.txt` file with one word per line. Lines are trimmed and
upper-cased, and lines with no letters in them are dropped.

A *match* is one pass through the whole word list — ten words for the bundled
lists, drawn at random without repeats. When the list runs out the match is
scored (more wins than losses, fewer, or a tie) and you pick a difficulty or a
new word list to start over.

## Scoring

Solving a word is worth points, and the number is small enough to work out in
your head while you play:

```
(50 + 10 × guesses left) × difficulty weight  +  25 × streak steps
```

| Term | What it is |
| --- | --- |
| `50` | the flat rate for solving a word at all |
| `10 × guesses left` | your unspent budget, so a clean win beats a scrape by 50 before the weight |
| difficulty weight | Easy 1, Medium 2, Hard 3, Insane 4 — and 1 for a word list of your own |
| streak steps | `min(streak − 1, 4)`, so the bonus builds to 100 and stops there |

The best a single word can do is a clean Insane win on a streak:
`(50 + 60) × 4 + 100` = **540**. The worst is **60**: an Easy word solved on the
very last guess you had, with no streak behind it. A word you lose, or give up
on, is worth nothing.

The **streak** is how many words you have solved in a row. It is the one number
here that survives everything: it carries across the end of a match, across a
difficulty change, across loading a new word list and across quitting the game.
Only failing a word puts it back to zero — and `Change Word` is failing a word.
The **best streak** is the high-water mark, and nothing but the `Reset stats`
button lowers it.

`SCORE` on the scoreboard is what the *match* on screen has earned so far; it
starts again at zero when you pick a difficulty or load a word list, and is
quoted in the end-of-match line. Everything else — lifetime points, words won
and lost, win rate, both streaks, the match tally and a breakdown of all of it
per difficulty — is behind the `Stats` button in the toolbar and is
[saved between launches](#settings).

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
- **The gallows is drawn, not drawn *once*.** The original shipped seven PNGs
  and swapped between them. Here the picture is geometry — a post, a beam, a
  brace, a rope and up to ten body parts — stroked onto a `canvas()` every
  frame. It stays sharp at any size, takes its colours from the theme instead
  of being a fixed image that only suits one, draws each new part on rather
  than cutting to the next frame, and is not tied to a budget of six wrong
  guesses.
- **The window resizes.** The original was a fixed, non-resizable 800×400.
- **Scoring replaced the tally.** The original's scoreboard was two numbers,
  `Wins` and `Losses` for the match in hand. Here it is the match's points, the
  current streak, the best streak ever, and a muted `Word n of 10` counter so you
  can tell how much of a match is left. See [Scoring](#scoring).
- **Small addition:** lifetime stats. The original remembered nothing between
  launches; this one keeps every point, both streaks and the win/loss tally,
  broken down by difficulty, behind the toolbar's `Stats` button.

## Roadmap

The port has caught up with the Java original, so from here the game stops
mirroring it. These are the nine ideas agreed for where it goes next, roughly in
the order they were argued about rather than in any committed order.

### Game and rules

1. **Hints, at a cost.** Reveal an unguessed letter in exchange for a wrong
   guess, or out of a small per-match budget.
2. **Scoring and streaks** *(done).* The bare `wins` / `losses` counters are
   gone. A solved word now scores on the guesses you had left and the difficulty
   you were playing, a run of solved words builds a bonus on top, and the
   scoreboard shows the match's points beside the current and best streak. The
   streak spans matches, difficulties and launches; only failing a word ends it.
   The lifetime tally lives behind the toolbar's `Stats` button and is saved with
   the rest of the settings — see [Scoring](#scoring).
3. **Structured word packs.** Move the four ten-word lists into a serde format
   that carries a category, a hint and a clue per word, so a match no longer
   exhausts the pool.
4. **Difficulty that changes the guess budget.** `MAX_WRONG_GUESSES` is a hard 6
   today. Item 6 unblocked it: the drawing now takes the budget as an argument
   and finishes the figure on the last guess whatever that budget is, so all
   that is left is letting each difficulty pick one.

### UI and UX

5. **Animation and game feel** *(done).* Draw the newest bit of the gallows on
   — a cross-fade between frames until item 6 replaced the frames — shake the
   word on a wrong guess, fade the cells a correct guess turns over up into
   place, stagger-reveal the letters on a win, pulse the wrong-guess pips, and
   settle a letter key into the colour its guess earned it. Still snapping: the
   keys that go out of play when the game ends.
6. **Draw the gallows procedurally** *(done).* The seven pre-rendered PNGs are
   gone; [`src/gallows.rs`](src/gallows.rs) describes the picture as polylines
   and `src/ui/gallows.rs` paints them with `canvas()` and `PathBuilder`. It
   scales to whatever room it is given, takes all three of its colours from the
   theme, and spreads its body parts over any guess budget — which is what
   unblocked item 4. The trade-off was real: it retired the bundled artwork.
7. **Keyboard hints.** Surface the shortcuts in the window itself with gpui-kit's
   `Kbd` and `Tooltip::action`.

### Craft

8. **Persist settings** *(done).* The theme, the window geometry and the chosen
   difficulty are written to a JSON file in the platform's config directory and
   restored at startup — see [Settings](#settings). Item 2 added the lifetime
   stats to the same file; the match's own score is still the one thing that is
   not kept, because it belongs to the match and dies with it.
9. **Make the UI testable.** Pull the pure helpers out of
   [`src/ui/mod.rs`](src/ui/mod.rs) — which has no tests at all — and cover them.

**Not planned:** networked multiplayer — the original's external layer is the one
thing the port deliberately dropped, and this would only bring it back — fetching
words from a dictionary API, and custom JSON themes with hot reload.

## Credits

Original game © Zack Rauen ([zerebos](https://github.com/zerebos)), 2015.
