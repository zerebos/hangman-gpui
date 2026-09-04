# CLAUDE.md

A Rust + GPUI port of the 2015 Java hangman. `README.md` is the user-facing
doc and covers building, the controls and the differences from the original in
detail — read it first. This file is the part a future session needs *before*
touching the code, especially the UI.

**Zerebos develops and tests this on Windows.** Anything platform-specific
should be checked with that in mind; the Linux notes below are for CI-less
sandboxes and Linux contributors.

## Build and run

```sh
cargo run                    # silent, needs nothing extra
cargo run --features sound   # adds the win/loss cues
```

- **The toolchain is pinned to `beta`** in `rust-toolchain.toml`. `gpui-pre`
  0.3.3 (pulled in by `gpui-kit` 0.6.0) calls the unstable
  `std::hint::cold_path()`, so stable rustc fails with
  `error[E0658]: use of unstable library feature 'cold_path'`. Don't "fix" the
  pin.
- **Linux linking needs `libxkbcommon-dev` and `libxkbcommon-x11-dev`.** The
  confusing part: `cargo check` passes without them and only the *link* step
  fails, with `rust-lld: error: unable to find library -lxkbcommon`.
- **Sound is an off-by-default `sound` cargo feature.** Enabling it makes the
  ALSA headers a hard *build* dependency on Linux (`libasound2-dev` on
  Debian/Ubuntu, `alsa-lib-devel` on Fedora) — `alsa-sys`'s build script
  panics without them and takes the whole crate down with it, including
  `cargo check`. With the feature off none of that is compiled.

### Before pushing

There is **no CI**, so these four are the only gate:

```sh
cargo fmt --check
cargo clippy --all-targets
cargo check --all-targets
cargo test
```

## Layout

- `src/game.rs` — the rules. Deliberately **no GPUI types**, covered by 26 unit
  tests in-file. Keep it that way; UI work should not need to touch it.
- `src/ui/mod.rs` — the single view. `src/ui/gallows.rs` — the artwork element.
- `src/audio.rs` — **two** implementations of `Audio` with identical public
  signatures behind `#[cfg(feature = "sound")]` / `#[cfg(not(...))]`: a real one
  and a zero-sized no-op. That is what keeps `#[cfg]` out of every UI call site.
  Preserve the pattern rather than adding `#[cfg]` at call sites.
- The crate is lib + bin (`src/lib.rs` + `src/main.rs`) so the unused `pub fn`s
  on `game::Game` don't trip `dead_code`.

## Gotchas

### 1. Any interactive control inside `TitleBar` needs `.occlude()`

This one has already cost a debugging session. `render_title_bar` in
`src/ui/mod.rs` wraps the theme toggle in `h_flex().pr_2().occlude()` — that
call is load-bearing.

**Mechanism.** gpui-kit's `TitleBar` container registers a bubble-phase
`on_mouse_down` that arms a drag (`gpui-component-0.6.0/src/title_bar.rs:352`)
and an `on_mouse_move` that calls `window.start_window_move()` when armed
(`:364-369`). Both are gated only on the title bar's hitbox being hovered — and
that hitbox covers its children. `Button` calls `cx.stop_propagation()` **only
while it is in its loading state** (`gpui-component-0.6.0/src/button/button.rs:746`,
`:760`), so an ordinary press bubbles straight through to the title bar. Any
click that drifts a single pixel between press and release is therefore handed
to the window manager as a window drag: the button never sees its mouse-up,
`on_click` never fires, and it stays painted in its pressed state until
something else forces a repaint. `.occlude()` installs a mouse-blocking hitbox
in front of the title bar so hit-testing stops at the wrapper.

**Symptom signature**, which is misleading enough to send you the wrong way:

- The control looks stuck hovered/pressed and its tooltip stops appearing.
- Its own state never changes, but *the rest of the window works fine*.
- Any other click both works **and** unsticks it.

That combination reads like a broken handler or a stale render. It isn't; it's
the title bar eating the event. Real bug in this repo, fixed in commit
`b58e033`.

### 2. `gpui_kit::init` hard-codes light mode

`gpui_component::init` calls `theme::init`, which does
`Theme::change(ThemeMode::Light, None, cx)`
(`gpui-component-0.6.0/src/theme/mod.rs:35`). This game is dark-first, so
`main.rs` calls `Theme::change(ThemeMode::Dark, None, cx)` **after**
`gpui_kit::init(cx)`. Order matters.

`Theme` is a GPUI global, so one call restyles every component. Pass
`Some(window)` from a click handler — `Theme::change` calls `window.refresh()`
itself (`theme/mod.rs:261-262`), so you don't need to.

### 3. Don't trust gpui-kit docs/examples over the compiler

Names that do **not** exist in gpui-kit 0.6.0, but that external gpui-kit
documentation has been seen to use: `cx.theme().surface`, `cx.theme().hover`,
`Theme::toggle_mode`, a free `label()` function. (`Theme` has no `surface` or
`hover` field — `surface` exists only as an optional key in the theme *JSON
schema*, `theme/schema.rs`; `label` exists only as `Button::label`.) Verify any
gpui-kit API by compiling it, not by reading about it.

### 4. Traits that must be imported by hand

Methods won't resolve without these; `src/ui/mod.rs` imports them all as `_`:

`ActiveTheme`, `ButtonVariants`, `Sizable`, `Disableable`, `Selectable`,
`StyledExt`, `FluentBuilder`.

The confusing one is `StyledExt`. **All** the font-weight helpers —
`font_bold`, `font_semibold`, `font_medium`, … — live on `StyledExt`
(`gpui-base-0.6.0/src/styled.rs:187-195`); plain `Styled` only gives you
`font_weight(FontWeight::BOLD)`. So a `no method named font_bold` error means a
missing import, not a wrong method name. Note that rustc unhelpfully suggests
"there is a method `font_semibold` with a similar name" — that one is on the
same unimported trait and won't compile either.

### 5. Images need no `AssetSource`

`include_bytes!` → `Image::from_bytes` → `img()`, as in `src/ui/gallows.rs`.
Bytes you already hold go straight into the renderer's decode cache, keyed on
content hash; an `AssetSource` is only for resolving *paths*. This matters
because the app's one `with_assets` slot is already spent on gpui-kit's own
icon assets (`main.rs`), which the title bar's window-control buttons need.

### 6. Headless click verification gives false positives

A bare `xdotool click` generates no mouse-move, so it does not exercise the
same code path as a real click — it reported the theme toggle in gotcha 1 as
working while it was completely broken. If you must verify a click headlessly,
send **press, move, release** separately and compare before/after screenshots
taken from a single process run.
