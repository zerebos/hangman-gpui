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

`cargo test` is 148 tests and finishes in under a second — every one of them is
in-file in a module with no GPUI types in it, so nothing there opens a window.
That holds even for the six in `src/ui/mod.rs`: they cover its prose helpers
(`plural`, `points`, `reset_stats_summary`), which take plain data and return a
`String`, and the test module imports them by name rather than with a
`use super::*` — see gotcha 10 for why that matters.

## Layout

- `src/game.rs` — the rules. Deliberately **no GPUI types**, covered by 54 unit
  tests in-file. Keep it that way; UI work should not need to touch it. Its
  `words_won`/`words_lost` are *per-match* counters that exist only so
  `finish_match` can derive a `MatchOutcome`; they reset with the match, and
  they are not a score. The guess budget lives here too and is per-difficulty
  (`Difficulty::guess_budget`: 10/8/7/6, and `DEFAULT_GUESS_BUDGET` = 6 for a
  word list with no difficulty behind it). The four numbers are not free: they
  have to stay inside `gallows::CORE_PARTS..=gallows::PARTS.len()`, i.e. 6..=10,
  or a wrong guess stops being exactly one new body part, and they have to keep
  falling as `Difficulty::weight` climbs or playing up stops paying. Both are
  guarded by tests (`every_budget_buys_exactly_one_body_part_per_wrong_guess`
  here, `a_clean_win_pays_more_the_harder_the_list` in `stats.rs`) — read those
  before retuning either table. `set_difficulty` returns a `bool` for the same
  reason `new_game` does: it refuses, and changes nothing, when the difficulty
  asked for is the one already in play *and* the match is still running, so a
  stray click on the selected pill cannot cost the word in hand. Once the match
  is over that click is the only way to replay the list, so it restarts as
  normal — don't collapse the two cases into one. The predicate behind that
  `false` is `would_switch_to`, public because the view has to ask *before* it
  acts: a switch throws the current word away, and `has_word_to_lose` decides
  whether that word has to be paid for. Those two are the whole abandon rule —
  a word with a guess or a hint on it, walked away from by a difficulty switch
  or a new word list, is charged as a loss by the *view* (`record_abandoned`),
  because `game.rs` is being reset out from under it and has no stats to keep.
  Read both before touching either call site: the loss belongs to the
  difficulty the word came from, so the word and its difficulty have to be
  captured before the reset, and `load_word_list` must charge only on its
  success branch, since a list that will not parse leaves the word alone.
  `hint` spends that budget: it reveals a letter
  drawn from the game's own `rng` (so a seeded game hints reproducibly) and
  charges one wrong guess, which is why hints needed no scoring change — a
  spent guess is already worth ten points through `remaining_guesses`. It
  refuses at `remaining_guesses() <= 1` on purpose, and `can_hint` is the
  predicate the UI greys its button out on: with two guesses in hand before the
  charge, a hint can never be the guess that *loses* a word, so `hint` checks
  the win and never the loss.
- `src/ui/mod.rs` — the single view. `src/ui/gallows.rs` — the element that
  paints the gallows.
- `src/gallows.rs` — the gallows *drawing*, as plain coordinates: polylines in
  a fixed 300×350 design box, which body part belongs to which stage, and the
  transform that fits the box into the rectangle the window gives it. **No GPUI
  types**, like `game.rs`, with 35 in-file tests. Its partner `src/ui/gallows.rs`
  is the only thing that turns any of it into `PathBuilder` paths, and it holds
  the colours (from `cx.theme()`) and the draw-on animation. Keep the split:
  geometry that a test can check belongs here, not in the 1,600-line view.
  Nothing in either file assumes a budget of six wrong guesses — `parts_drawn`
  takes the budget as an argument, which is what let roadmap item 4 make the
  budget per-difficulty without touching either of them.
- `src/stats.rs` — points, streaks and the lifetime tally, with 28 in-file
  tests. **No GPUI types**, like `game.rs`, and it is where the serde derives
  for the score live so that `game.rs` needs none: `Difficulty` is mapped by
  hand there, exactly as `settings.rs` does it. `Stats` is the persisted value,
  `Session` is what the view holds, and all the arithmetic is in here rather
  than in the UI. The streak spans matches, difficulties and launches on
  purpose — only a lost word ends it — so nothing in `game.rs` may reset it.
- `src/settings.rs` — the JSON file that remembers the theme, the window
  geometry, the difficulty and the `stats`. Like `game.rs` it holds **no GPUI
  types** and is covered by 25 in-file tests; the conversions to `ThemeMode` and
  `Bounds<Pixels>` live in `src/ui/mod.rs` instead. Nothing in it may fail
  loudly: every read error falls back to `Settings::default()`, and a malformed
  `stats` key falls back on its own rather than taking the file with it.
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
(`gpui-component-0.6.0/src/theme/mod.rs:35`). This game is dark-first, so the
saved choice has to be applied **after** `gpui_kit::init(cx)`, not before —
`settings.theme` defaults to `ThemeMode::Dark`. Order matters.

`Theme` is a GPUI global, so one call restyles every component. `Theme::change`
calls `window.refresh()` itself when you pass it `Some(window)`
(`theme/mod.rs:261-262`), so you don't normally need to.

**Nothing may call `Theme::change` directly any more.** `ui::apply_theme` is
the only entry point, and both `main.rs` at startup and the title-bar toggle go
through it. The reason is that this game overrides exactly one theme token —
the dialog backdrop, see gotcha 10 — and `Theme::change` rebuilds the entire
colour set from the theme config (`theme/mod.rs:245-255`), so an override
written once at startup is silently reverted by the first press of the toggle.
The symptom of getting this wrong is a dialog that dims correctly until you
change theme and never again, which reads as a dialog bug rather than a theme
one. `apply_theme` therefore passes `None` to `Theme::change` and calls
`window.refresh()` itself, after the override is back in — refreshing first
would paint the frame with the token that was just reset.

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

### 5. Images need no `AssetSource` — but nothing here draws one any more

The gallows used to be seven PNGs and was the only image in the app; it is
drawn with `canvas()` and `PathBuilder` now, so the tree has no `img()` call
left. Keep the fact to hand anyway, because the obvious next move when you do
want a picture is the wrong one:

`include_bytes!` → `Image::from_bytes` → `img()` is all it takes. Bytes you
already hold go straight into the renderer's decode cache, keyed on content
hash; an `AssetSource` is only for resolving *paths*. This matters because the
app's one `with_assets` slot is already spent on gpui-kit's own icon assets
(`main.rs`), which the title bar's window-control buttons need — so an image
that needed a second one would be stuck. The last version that did it this way
is the parent of the commit that removed `assets/images/`.

### 6. The title bar's close button does not go through `on_window_should_close`

Only on Linux, and it is why `render_title_bar` passes `TitleBar::on_close_window`
as well as `HangmanView::new` registering `window.on_window_should_close`.

gpui-kit draws its own window controls on Linux, and its X calls
`window.remove_window()` from an ordinary `on_click`
(`gpui-component-0.6.0/src/title_bar.rs:236`), which only sets `removed = true`
and never asks the platform whether the window should close. On Windows and
macOS those buttons are the system's — Windows marks them with
`window_control_area`, macOS uses the real traffic lights — so the close goes
through the platform and `on_window_should_close` does fire. `on_close_window`
is the hook for the Linux button, and gpui-kit ignores it outright on the other
two platforms (`title_bar.rs:98`), so it needs no `#[cfg]`.

The consequence: anything that must happen as the window closes needs **both**
hooks, or it will silently not happen on Linux (saving the window geometry, in
this case). `Window::on_window_should_close` also replaces any previously
registered handler rather than adding to it — there is one slot per window.

### 7. Headless click verification gives false positives

A bare `xdotool click` generates no mouse-move, so it does not exercise the
same code path as a real click — it reported the theme toggle in gotcha 1 as
working while it was completely broken. If you must verify a click headlessly,
send **press, move, release** separately and compare before/after screenshots
taken from a single process run.

### 8. Animating what a `canvas()` paints needs a component, not a canvas

`with_animation` hands its animator the *element* and wants one back
(`animator: impl Fn(Self, f32) -> Self`, `gpui-pre-0.3.3/src/elements/animation.rs:83`).
That works for anything the builder API can restyle — opacity, size, offset —
but `canvas()` takes its paint callback as a boxed `FnOnce` when it is *built*
(`src/elements/canvas.rs:10-19`), so there is nothing left to change afterwards
and no way to feed it the frame's `delta`.

The way through is `src/ui/gallows.rs`: a small `RenderOnce` struct holding the
drawing's parameters, with the canvas built inside its `render`. Animating the
struct rewrites a field, and the canvas is rebuilt from it every frame.

Two smaller ones from the same file:

- `#[derive(IntoElement)]` expands to `gpui::IntoElement`, and this crate has no
  dependency spelled `gpui` — `use gpui_kit::gpui;` in the module makes the
  derive resolve. Same reason `gpui_kit` re-exports its own `actions!`.
- `Pixels`' tuple field is private outside gpui. Use `Pixels::as_f32()`; the
  `.0` you will see in gpui-kit's own source only compiles inside gpui-kit.

### 9. A cross-axis-centred flex column with no width of its own shrink-wraps

`render_stage` in `src/ui/mod.rs` puts the gallows drawing and the stage pips in
one `v_flex()` inside a `panel(cx)` that sets `.items_center()`. The `.w_full()`
on that column is load-bearing.

**Mechanism.** `items_center` does not stretch a child across the cross axis, so
the column is sized to its content, and taffy 0.13 measures that content
*intrinsically* — with no container width to resolve percentages against. The
drawing's own `w_full` (`src/ui/gallows.rs`) therefore contributes nothing to
the measurement, and the only child left with a width was the pip row: one pip
of `PIP_SIZE` (`px(9.)`) per wrong guess in the budget, with a `gap_1p5`
(`rems(0.375)`, 6px) between each pair. At the time the budget was a hard 6, so
that row was 6×9 + 5×6 = exactly 84px. The column shrink-wrapped to 84px inside
a 300px panel (`STAGE_WIDTH` 332, less `p_4` on both sides), and the canvas
dutifully fitted its 300×350 design box into that: the gallows drew at 28%
scale, its 7-unit frame lines under 2px.

The budget is per-difficulty since roadmap item 4, so the row is now 84px on
Insane and 10×9 + 9×6 = 144px on Easy — still under half of the 300px, so the
column would still have shrink-wrapped, just less obviously. Note what that
means for the next person: a wider sibling would have *hidden* this bug rather
than fixed it. The `w_full` is what fixes it, at any budget.

**Symptom signature.** A `w_full` child renders far too small, and the width it
renders at turns out to be the width of one of its *siblings*. That reads as a
scaling bug in the drawing code, and it isn't — the drawing filled the box it
was handed; the box was wrong one level up.

The fix is `.w_full()` on the column, `flex_1` instead of a fixed height on the
drawing so it takes the column's slack, and `flex_none` on the pip row so it
keeps its own. Real bug in this repo, fixed in commit `315e8ef`.

### 10. A dialog needs a layer rendered, a guard on the keyboard, and no `use super::*` in its tests

`Reset stats` is the window's one gpui-kit dialog (`confirm_reset_stats` in
`src/ui/mod.rs`), and it took four surprises to get there. The component itself
is good: `AlertDialog` takes both themes from `cx.theme()` with nothing
hard-coded, traps Tab, blocks the mouse behind it, and gets Escape and Enter
for free — `gpui_kit::init` reaches `gpui_base::dialog::init`, which binds
`escape` to `Cancel` and `enter` to `Confirm` in a `Dialog` key context
(`gpui-base-0.6.0/src/dialog.rs:89-91`). The surprises are around it.

**`Root` does not paint the dialog layer.** `impl Render for Root` renders the
view, the tooltip overlay and the native-menu overlay, and nothing else
(`gpui-component-0.6.0/src/root.rs:577`). `window.open_dialog` pushes onto
`Root::active_dialogs` and stops there, so a window whose view never renders
`Root::render_dialog_layer(window, cx)` opens dialogs that are invisible and
un-dismissable — indistinguishable from a dialog that never opened. This view
already renders it, next to `render_notification_layer`; keep both.

**That layer is a child of this view**, so it is *inside* `key_context(KEY_CONTEXT)`
and under the `on_key_down` that guesses letters. Keys pressed at a dialog
bubble all the way up to it: without a guard, typing `A` at the confirmation
guesses A on the board behind, and `Ctrl+H` spends a wrong guess the player
cannot see happen. `dialog_is_open` (`window.has_active_dialog(cx)`) is that
guard and every key path calls it first. Escape and Enter are the exception
and need no guard — they are dispatched from the dialog's own key context,
which is nearer the focus.

**`AlertDialog` is the confirm-shaped one, `Dialog` the general one.** Alert
defaults to no ✕ and refuses backdrop dismissal outright — its
`overlay_closable` is `#[deprecated]` to a no-op rather than merely defaulted
off (`dialog/alert_dialog.rs`) — which is what a destructive confirm wants.
`Dialog` closes on a backdrop click by default. Both `on_ok` and `on_cancel`
return a `bool` saying whether to close, so a dialog can refuse to go. Neither
is a `cx.listener`: they are plain `Fn(&ClickEvent, &mut Window, &mut App)`,
so reaching the view means a `WeakEntity`. The builder passed to
`open_alert_dialog` is an `Fn` re-run every frame the dialog is on screen, so
everything it captures must be cloned inside, not moved.

**`#[test]` does not compile in a module that does `use super::*` here.**
`src/ui/mod.rs` has `use gpui_kit::*`, which re-exports gpui's own `test`
attribute macro; a glob beats the prelude, so `#[test]` resolves to that one
and rustc dies with `recursion limit reached while expanding #[test]`. The
error names neither gpui nor the glob and its suggestion — raise
`recursion_limit` — is a blind alley. Import the handful of items the tests
need by name instead. `game.rs` and friends never hit this because they import
no gpui.

**The backdrop dim is a theme token, and gpui-kit's default is too weak for
this window.** It is `cx.theme().overlay`, which `default-theme.json` sets to
black at **5%** in light and **20%** in dark, and there is no per-dialog knob
for it — `Dialog::overlay(bool)` is on or off and nothing else. The dark
default is the instructive one: four times the alpha of the light default, and
it lands *softer*, because a black wash over a board that is already near-black
has almost nothing left to darken. Measured off headless captures the stock
tokens moved the board from 22 to 18 in dark and 250 to 237 in light — a dialog
that reads as "slightly greyed", not "modal".

`OVERLAY_DIM_LIGHT` / `OVERLAY_DIM_DARK` in `src/ui/mod.rs` override it to 35%
and 50%, which measure 250 → 163 and 22 → 11. Note that the two numbers are not
a pair: light is *lower* and dims *more*, because white has further to fall.
The override is applied in `apply_theme` and gotcha 2 is why it has to live
there rather than at startup.
