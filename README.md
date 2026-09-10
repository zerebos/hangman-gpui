# hangman-gpui

A Rust + [GPUI](https://www.gpui.rs/) port of [`zerebos/Hangman`](https://github.com/zerebos/Hangman),
a 2015 Java/Swing hangman game. Same rules, same word lists, same layout, the
same two sound cues and the same (slightly unhinged) alert messages — rebuilt
with [gpui-kit](https://gpui-kit.com) instead of Swing. Two things are not the
original's: the gallows is drawn line by line at run time rather than shipped as
pictures, and difficulty now sets [the guess budget](#the-guess-budget) as well
as the word list — a budget you can trade a guess out of for
[a hint](#hints). See
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
engine (with 54 unit tests), [`src/stats.rs`](src/stats.rs) scores the words and
keeps the streak (28 more), [`src/settings.rs`](src/settings.rs) is the equally
UI-free file that remembers your choices between launches (25 more),
[`src/gallows.rs`](src/gallows.rs) is the gallows drawing as plain coordinates
(35 more), and [`src/ui/`](src/ui/) is everything GPUI. That is 147 tests, and
`cargo test` runs the lot in well under a second. The word lists and the
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
      "height": 800.0
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
read-only disk, and the game falls back to its defaults — dark, centred at
1000 × 800, Easy —
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
| Reveal a letter (costs one wrong guess) | `Hint` button, or `Ctrl+H` |
| Next word after a game ends | `New Game?` button, or Enter / Space |
| Give up on the current word (counts as a loss) | `Change Word` button, or `Ctrl+N` |
| Load your own word list | `Open word list…` button, or `Ctrl+O` |
| Change difficulty (starts a fresh match, and changes the guess budget) | The Easy / Medium / Hard / Insane buttons |
| Abandon the word you are on (counts as a loss) | Changing difficulty or loading a word list mid-word |
| Replay the difficulty you just finished | The button already selected, once the match is over |
| Show the lifetime stats | The `Stats` button in the toolbar |
| Quit | Close the window |

Nothing in that table has to be memorised: the strip along the bottom of the
window lists the shortcuts as you play, dimming the ones the game would refuse
right now — `Ctrl+H` once a hint would cost your last guess, `Ctrl+N` once the
word has ended — and showing `Enter` only while there is a next word to deal.
The same key is drawn beside the matching toolbar button's tooltip. Both are
read from the bindings themselves, so on macOS they read `⌃H` rather than
`Ctrl+H`.

A word list is a plain `.txt` file with one word per line. Lines are trimmed and
upper-cased, and lines with no letters in them are dropped.

A *match* is one pass through the whole word list — ten words for the bundled
lists, drawn at random without repeats. When the list runs out the match is
scored (more wins than losses, fewer, or a tie) and you pick a difficulty or a
new word list to start over — including the difficulty you were already on,
which is the one click that restarts it. Mid-match that same click does
nothing, so the word in hand survives a stray press on the button that is
already selected.

Leaving a word part-played is losing it. Switching difficulty or loading a new
list while a word is in progress counts that word as a loss, ends your streak
and says so, exactly as `Change Word` does — otherwise the quickest way out of a
word you were about to fail would also be the one that cost nothing. A word you
have not guessed a letter of yet is not in progress, so picking a difficulty
before you start is free, and so is re-picking the one you are already on.

## The guess budget

How many wrong guesses you get is part of the difficulty, not a constant:

| Difficulty | Wrong guesses | Word multiplier |
| --- | ---: | ---: |
| Easy | 10 | ×1 |
| Medium | 8 | ×2 |
| Hard | 7 | ×3 |
| Insane | 6 | ×4 |
| A word list of your own | 6 | ×1 |

Insane is the original game: six guesses and the classic figure. Everything
easier buys you slack, and the drawing spends it — the gallows has ten body
parts, six of which make a whole hangman and four of which are the hands and
feet, so one wrong guess is always exactly one new part however many you get.
Ten is the ceiling for that reason and not an arbitrary one.

A word list you load from a file gets six, because nothing in a `.txt` file
says how hard it is meant to be.

## Hints

`Hint` — the toolbar button, or `Ctrl+H` — reveals one letter of the word you
have not guessed yet, and charges you **one wrong guess** for it. The letter
lights up its cells and its key exactly as if you had guessed it, and the
gallows gains a body part exactly as if you had guessed wrong.

That price is deliberately the only one. A hint costs a tenth of Easy's budget
and a sixth of Insane's, so it is worth what the difficulty says it is worth
with no second number to tune, and the ten points an unspent guess is worth
under [Scoring](#scoring) is the whole penalty — there is no hint fee on top of
it.

**A hint is refused when you have one guess left**, and the button greys out
and says so. A hint that spent your last guess would reveal a letter and lose
you the word in the same breath, which is a trap rather than a choice — and it
would raise the question of whether a word completed by the hint that killed
you counts as a win. Stopping one guess short means the question never comes
up. A hint *can* finish a word, and when it does you win.

## Scoring

Solving a word is worth points, and the number is small enough to work out in
your head while you play:

```
(50 + 10 × guesses left) × difficulty weight  +  25 × streak steps
```

| Term | What it is |
| --- | --- |
| `50` | the flat rate for solving a word at all |
| `10 × guesses left` | your unspent budget, so a clean win beats a last-guess scrape by ten points per guess the difficulty gave you |
| difficulty weight | Easy 1, Medium 2, Hard 3, Insane 4 — and 1 for a word list of your own |
| streak steps | `min(streak − 1, 4)`, so the bonus builds to 100 and stops there |

Easy hands out the most guesses to leave unspent and Insane the fewest, but the
weight more than makes up for it, so playing up always pays: a clean win is
worth 150 on Easy, 260 on Medium, 360 on Hard and 440 on Insane before any
streak. The best a single word can do is a clean Insane win on a streak:
`(50 + 60) × 4 + 100` = **540**. The worst is **60**: an Easy word solved on the
very last guess you had, with no streak behind it. A word you lose, or give up
on, is worth nothing.

A [hint](#hints) is charged through the guess budget rather than by a penalty
of its own: it spends a wrong guess, so the `10 × guesses left` term takes ten
points per difficulty weight off the word and nothing else changes. A word
solved with a hint scores exactly what the same word solved with one guess
fewer in hand would, and it extends the streak like any other win.

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
  guesses — which is what let difficulty start changing that budget.
- **Difficulty changes the rules, not just the list.** The original had a
  `setMaximumWrongGuesses` setter that nothing ever called, so every difficulty
  gave you the same six guesses. Here Easy gives 10, Medium 8, Hard 7 and
  Insane the original's 6 — see [The guess budget](#the-guess-budget).
- **The window resizes.** The original was a fixed, non-resizable 800×400.
- **Scoring replaced the tally.** The original's scoreboard was two numbers,
  `Wins` and `Losses` for the match in hand. Here it is the match's points, the
  current streak, the best streak ever, and a muted `Word n of 10` counter so you
  can tell how much of a match is left. See [Scoring](#scoring).
- **Small addition:** hints. The original had none. `Hint` reveals a letter for
  the price of a wrong guess, and refuses when that would be the last guess you
  have — see [Hints](#hints).
- **Small addition:** lifetime stats. The original remembered nothing between
  launches; this one keeps every point, both streaks and the win/loss tally,
  broken down by difficulty, behind the toolbar's `Stats` button.

## Roadmap

The port has caught up with the Java original, so from here the game stops
mirroring it. These are the eleven ideas agreed for where it goes next, roughly in
the order they were argued about rather than in any committed order.

### Game and rules

1. **Hints, at a cost** *(done).* `Hint` in the toolbar, or `Ctrl+H`, reveals a
   letter you have not guessed and charges a wrong guess for it — the first of
   the two variants that were on the table. The second, a small per-match
   allowance, was not built: it needs a counter in [`Game`](src/game.rs), a
   ruling on whether it refills between words, and a field in the settings file
   to survive a launch, and it would still need a price for the hint after the
   allowance ran out. Charging a guess needs none of that. It reuses the loss
   path `guess` already runs, item 4 sizes it per difficulty for free — a tenth
   of Easy, a sixth of Insane — and it needs no scoring change at all, because
   a spent guess already costs the word ten points through `remaining_guesses`.
   A hint is refused with one guess left, so it can never be the thing that
   loses you the word; a hint that completes the word wins it. See
   [Hints](#hints).
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
4. **Difficulty that changes the guess budget** *(done).* The budget was a hard
   6 for everyone; it is `Difficulty::guess_budget` now — 10 on Easy, 8 on
   Medium, 7 on Hard and the original's 6 on Insane, with 6 for a word list of
   your own. The counter, the pips and the drawing all follow it, and the six
   the original gave everybody survives as `DEFAULT_GUESS_BUDGET`, the fallback
   for a game with no difficulty behind it. Item 6 is what made it a small
   change: the gallows already took the budget as an argument, and its ten body
   parts are exactly what a budget of 6..=10 needs to draw one new part per
   wrong guess. See [The guess budget](#the-guess-budget). It also makes item 1
   cheaper — a hint priced "in exchange for a wrong guess" now spends a budget
   that difficulty already sizes, so the easy lists can afford one and Insane
   can be made to hurt, with no second knob to invent.

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
7. **Keyboard hints** *(done).* The shortcuts are on screen instead of being
   folklore. A strip along the bottom of the window lists each one as a
   `Kbd` chip beside what it does, greying the ones the game would currently
   refuse and adding `Enter` only while there is a next word to deal, and the
   toolbar's tooltips carry the same chip through `tooltip_with_action`
   instead of the chord being typed into the sentence. Both read the binding
   out of the keymap `main.rs` registers rather than repeating it, so they
   cannot drift from it and each chord spells itself the way the platform does
   — `Ctrl+H` here, `⌃H` on macOS. See [Controls](#controls).
10. **Try gpui-kit's `Modal` and `Dialog`.** The window has never used either —
    the stats panel folds out inline and the warning before you abandon a word
    is a tooltip, both because an unproven component API is the trap the project
    notes warn about, not because a dialog would be wrong. Build one somewhere
    small and see how it behaves: whether it takes the theme, whether Escape and
    a click outside dismiss it, whether it traps focus, and how it reads on
    Windows. If it comes out well there is more than one place for it — a
    confirm before a difficulty switch costs you a word, a confirm before
    `Reset stats` throws the lifetime tally away, and the end-of-match summary
    are all currently shaped around not having one.

### Craft

8. **Persist settings** *(done).* The theme, the window geometry and the chosen
   difficulty are written to a JSON file in the platform's config directory and
   restored at startup — see [Settings](#settings). Item 2 added the lifetime
   stats to the same file; the match's own score is still the one thing that is
   not kept, because it belongs to the match and dies with it.
9. **Make the UI testable.** Pull the pure helpers out of
   [`src/ui/mod.rs`](src/ui/mod.rs) and cover them. Item 7 opened the file's
   first test module by keeping its own rule — which shortcuts the legend
   offers, and which of them are live — in a plain function over a `&Game`;
   the four `&self` helpers next to it (`match_summary`, `subtitle`,
   `guess_count`, `key_state`) are the rest of the job.
11. **Resume the word you were on.** Closing the window mid-word is the last
    silent way out of a word you are losing: the settings file keeps the theme,
    the window, the difficulty and the lifetime stats, and nothing at all about
    the word in flight, so quitting and relaunching is a free reroll that keeps
    your streak. Charging a loss on close would shut that door, but it would
    also tax someone who just quit for the night, and invisibly — they would
    never see it happen. Saving the word instead means quitting is not an escape
    because you come back to it: the word, the letters guessed, the wrong-guess
    count, the pool still to play and the match's points, restored at launch.
    That needs a new key in [`Settings`](src/settings.rs) and a way to rehydrate
    a [`Game`](src/game.rs) from one, both under the existing rule that a
    malformed value falls back rather than failing loudly. It is worth having on
    its own account as much as for the hole it closes.

**Not planned:** networked multiplayer — the original's external layer is the one
thing the port deliberately dropped, and this would only bring it back — fetching
words from a dictionary API, and custom JSON themes with hot reload.

## Credits

Original game © Zack Rauen ([zerebos](https://github.com/zerebos)), 2015.
