# hangman-gpui

A Rust + [GPUI](https://www.gpui.rs/) port of [`zerebos/Hangman`](https://github.com/zerebos/Hangman),
a 2015 Java/Swing hangman game. Same rules, same word lists, same artwork, same
layout and the same (slightly unhinged) alert messages — rebuilt with
[gpui-kit](https://gpui-kit.com) instead of Swing.

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
word lists and the original's gallows artwork live in [`assets/`](assets/) and
are compiled into the binary, so there is nothing to install next to the
executable.

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
- **Sounds are not ported.** The original played `win.mp3` / `loss.mp3` through
  JavaFX. Adding an audio dependency for two cues was not worth it.
- **A game-result bug is fixed.** On the *final* word of a match, the original
  only ever announced the match result and silently skipped the win/loss message
  (and the sound) for that game. Here both fire: the alert line shows
  `You WIN!` / `Bring Add/Drop Form!` and the footer shows the match summary.
- **The window resizes.** The original was a fixed, non-resizable 800×400.
- **Small addition:** a muted `Word n of 10` counter next to the score, so you can
  tell how much of a match is left.

## Credits

Original game © Zack Rauen ([zerebos](https://github.com/zerebos)), 2015.
