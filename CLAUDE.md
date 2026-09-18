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

`cargo test` is 268 tests and finishes in under a second, because **not one of
them opens a window, needs an `App`, or touches the platform.** For the five
GPUI-free modules that is guaranteed by the file: there is no gpui in them at
all. `src/ui/mod.rs` is the exception and the discipline there is a choice, not
a guarantee — the view and all its gpui imports are in the same file as the
tests, and its sixty-nine only reach free functions that take plain data and
return plain data: `shortcut_legend` from item 7, `plural` / `points` /
`reset_stats_summary` / `dialog_top_margin` from item 10, `subtitle` /
`guess_count` / `key_state` / `hint_tooltip` / `match_summary` /
`word_being_abandoned` from item 9, `switch_difficulty` / `load_words` from
item 13, `can_show_clue` / `clue_tooltip` / `loaded_summary` from item 3, and
`resume_or_start` / `save_match` from item 11, and `percent` /
`shake_offset` / `Reveal::progress` / `Reveal::span` / `to_rect` / `to_bounds`
alongside them. The item 13 pair is the one that takes
a `&mut Game` and changes it rather than only reading — which is still inside
the rule, because the rule is about needing no window, no `App` and no
platform, and a `Game` needs none of the three.
Anything added to that module has to keep to the same rule by hand, importing
what it tests by name rather than with a `use super::*` — see gotcha 10 for why
that matters.

**The rule is about what a test needs, not about which crate a type came
from**, and that distinction is worth holding on to. `Bounds<Pixels>` is a gpui
type, and `to_rect` / `to_bounds` are tested with real ones: it is four
arithmetic newtypes in a trenchcoat, constructing one starts nothing and the
window geometry it carries is the sort of thing a transposed field breaks in a
way review does not catch. A `Window`, an `App`, a `Context` or an element is
the opposite — needing any of those is the signal that the logic wants pulling
out into a free function instead. `SharedString` falls on the harmless side for
the same reason `Bounds` does — it is a `SmolStr` newtype — which is why
`Notice`, the view's one feedback-line type, still holds one even though
`match_summary` builds it and the tests read it. Changing that field to a plain
`String` bought the tests nothing and cost an allocation on every frame, since
both render sites clone the notice out of the view to read it; cloning a
`SmolStr` is an `Arc` bump instead. If a type really is the only thing standing
between a helper and a test, ask whether it needs a window before assuming it
does.

## Layout

- `src/words.rs` — the word-pack **file format**, and nothing else: `Word`,
  `Pack`, the parsing and the sanitising, with 16 in-file tests. **No GPUI
  types**, like `game.rs`. It is also where the serde derives for a word live,
  so `game.rs` needs none — the same split `stats.rs` has for the score. Three
  rules in it are load-bearing and each has a test: a **bundled** pack that
  will not parse *panics* (`Pack::bundled`), which is the opposite of the
  `settings.rs` rule on purpose — a settings file we cannot read is the
  player's, a word pack we cannot read is ours; **unknown keys are ignored
  rather than refused**, which with `#[serde(default)]` on every optional field
  is what stands in for a version number, so don't add `deny_unknown_fields`
  and don't add a `version` key; and `Pack::parse` picks JSON or
  one-word-per-line from the text's **first non-blank character**, never from
  the file name, because the picker has no extension filter and a player's
  naming is not ours to police. A leading byte-order mark is stripped before
  that test: `trim_start` does not remove one (U+FEFF is not Unicode
  whitespace), and Windows editors write one by default, so without the strip a
  BOM'd pack silently loads its own JSON lines as words. **A pack's `name` is
  taken through `Pack::display_name`, which trims it**, so `"name": "   "` is a
  pack with no name and `Game::pack_name` can go on testing emptiness alone —
  the same blank-is-absent rule `Word::said_something` applies to a category
  and a clue. `Word` derives `Serialize` as well as `Deserialize`, and since
  item 11 both halves have a caller: `words::sanitize` — which
  `Pack::into_words` is now a one-liner over — is the cleanup, and
  `settings::SavedMatch` writes the word and the pool still to play into
  `settings.json`.
- `src/game.rs` — the rules. Deliberately **no GPUI types**, covered by 85 unit
  tests in-file. Keep it that way; UI work should not need to touch it. Since
  item 3 a match is `MATCH_WORDS` (10) words *drawn* from the pack by
  `draw_match` rather than the whole pack, so `total_words` is the match and
  `pack_words` is the pack — the two were the same number before it, and
  anything that still conflates them is now wrong. `budget_for` takes the
  pack's wish as well as the difficulty: a **loaded** pack may state a
  `guess_budget` and gets it clamped into 6..=10, a pack reached through a
  difficulty pill may not, because the four numbers below are the ladder and a
  downloaded file does not get to bend it. Its
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
  `false` is `would_switch_to`, and `has_word_to_lose` decides whether the word
  a switch throws away has to be paid for. Nothing outside this module calls
  `would_switch_to` since item 13 — `switch_difficulty` builds its `Dealt` out
  of `set_difficulty`'s own `bool` instead, so a charge cannot be handed back
  for a switch that did not happen — but it stays public and named: it is the
  rule the refusal is *about*, and asking it before acting is still the right
  move for any caller that needs the answer without committing to the switch.
  `would_switch_to` and `has_word_to_lose` are the whole abandon rule between
  them — a word with a guess or a hint on it, walked away from by a difficulty
  switch or a new word list, is charged as a loss by the *view*
  (`record_abandoned`), because `game.rs` is being reset out from under it and
  has no stats to keep.
  Read both before touching either call site: the loss belongs to the
  difficulty the word came from, so the word and its difficulty have to be
  captured before the reset, and the file path must charge only on its success
  branch, since a list that will not parse leaves the word alone. Both of those
  are *order* rather than arithmetic, so since roadmap item 13 they live in two
  free functions in `src/ui/mod.rs` instead of inline in the view:
  `switch_difficulty` and `load_words` take a `&mut Game`, do the reset, and
  hand back an `AbandonCharge` or nothing for the view to apply. That is what
  made them testable, and the seven tests on them fail if either order is
  reversed — verified by reversing each and watching exactly one test go red.
  **The match in flight is a `Snapshot`, and this module decides what a
  plausible one is.** `Game::snapshot` hands back `None` for a match that is
  over and everything else otherwise, including a word that has just been
  resolved — "where you were" is one rule with no exceptions in it, and the
  alternative charges the rest of the match for closing the window on a word
  you had just won. `Game::resume` is the way back *and* the validator: the
  file it comes out of is one the player is invited to edit, so a snapshot is
  checked the way a pack off disk is, and a refusal costs a fresh deal rather
  than a panic. Five checks, each with a test: the word and the pool go through
  `words::sanitize`; the **budget is re-derived through `budget_for` rather
  than restored**, so a hand edit cannot buy Easy's ten guesses at Insane's
  weight; `result` has to be one the rest of the state could have produced,
  which is also why it is *stored* rather than derived — a word given up on is
  lost with guesses in hand and letters still hidden, which is exactly what a
  word still in play looks like; and a resolved word with an empty pool behind
  it is a *finished* match, which `snapshot` never writes; and a resolved word
  has to be *on* the per-match tally, since `end_game` counts it in the same
  breath as it sets the result — without that check the word on the board falls
  out of the count and `word_number` reports 0. `total_words` is
  recounted rather than stored, because derived state written down twice is
  derived state that can come back disagreeing with itself. `word_complete` is
  free rather than a method for the same reason the item 13 pair is: `resume`
  has to ask the win check's question of a word and a letter set that are not a
  `Game` yet.
  `hint` spends that budget: it reveals a letter
  drawn from the game's own `rng` (so a seeded game hints reproducibly) and
  charges one wrong guess, which is why hints needed no scoring change — a
  spent guess is already worth ten points through `remaining_guesses`. It
  refuses at `remaining_guesses() <= 1` on purpose, and `can_hint` is the
  predicate the UI greys its button out on: with two guesses in hand before the
  charge, a hint can never be the guess that *loses* a word, so `hint` checks
  the win and never the loss.
  The **category and clue** a word carries are read off here by `category()` /
  `clue()` — the view puts the category on the word panel's heading row and the
  clue under the letters, and both are safe to show *during* play — a guarantee held up by
  `no_bundled_clue_gives_its_own_word_away` rather than by review: it checks
  every bundled category and clue for the word's own letters and for its first
  six, and it caught two while the content was being written. Nothing here
  tracks whether a clue has been *read*, because nothing about reading one is
  scored or charged; that flag is the view's (`clue_shown`), which is why
  `shortcut_legend` takes it as an argument rather than reading it off the
  game.
- `src/ui/mod.rs` — the single view. `src/ui/gallows.rs` — the element that
  paints the gallows. `HangmanView` itself is not testable, but the rules it
  reads off the game are: every helper that needed nothing but a `&Game` (and,
  for `match_summary`, a `&Session`) is a free function above the view rather
  than a `&self` method on it, so the in-file tests build no GPUI type and open
  no window, exactly like the other four modules'. That is the shape anything
  pulled out of the view has to keep — `&self` is the signature to avoid,
  because it drags the whole view into the test. Neither the legend nor
  any toolbar tooltip spells a chord out: `Kbd::binding_for_action` and
  `Button::tooltip_with_action` read it from the keymap `main.rs` registers, so
  rebinding a shortcut there updates every place it is shown. The one
  exception is `Enter`, which is handled in `on_key_down` rather than bound —
  a `KeyBinding` would fire even when a letter key has been tabbed to — so it
  has nothing in the keymap to read and `shortcut_kbd` spells it by hand.
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
- `src/stats.rs` — points, streaks and the lifetime tally, with 31 in-file
  tests. **No GPUI types**, like `game.rs`, and it is where the serde derives
  for the score live so that `game.rs` needs none: `Difficulty` is mapped by
  hand there, exactly as `settings.rs` does it. Note what a custom pack can and
  cannot reach from here, because it is the answer to "should a pack be allowed
  to set X": a pack may move `remaining_guesses` (6..=10) but never `weight`,
  which stays 1 for anything with no difficulty — so the most a pack can score
  itself is 150 a word, exactly Easy's rate and the lowest of the four. That
  bound is why `guess_budget` is safe to honour and why a pack-declared
  *weight* (roadmap item 16) would not be. `Stats` is the persisted value,
  `Session` is what the view holds, and all the arithmetic is in here rather
  than in the UI. The streak spans matches, difficulties and launches on
  purpose — only a lost word ends it — so nothing in `game.rs` may reset it.
- `src/settings.rs` — the JSON file that remembers the theme, the window
  geometry, the difficulty, the `stats` and, since item 11, the match
  `in_flight`. `SavedMatch` is the file's shape for a `game::Snapshot` plus the
  one number a snapshot does not carry, `Session`'s `match_points`; it
  validates **nothing**, on purpose — what a plausible match is, is a rule, and
  the rules are `game.rs`'s. It is forgiving in the same two ways `stats` is
  (`in_flight_or_default`), and the view writes it on **every guess** rather
  than at exit, because a save that only happened on a clean close would still
  hand a free reroll to anyone who killed the process mid-word. That needs no
  debouncing and should not get any: a guess only writes when it *lands*, and
  the duplicate check means a letter lands once, so the ceiling is the budget
  plus the word's distinct letters — **measured at 21 writes per word on Easy
  and 18–19 on the rest**, of a file measured at 2.6 KB with a full pool in it.
  Mashing the keyboard cannot beat that bound, because the 27th key of a word
  is necessarily a duplicate and the word itself dies once the budget is spent.
  A timer would trade an exact bound for a window in which a kill loses the
  guesses inside it, which is the one thing this key is here to prevent.
  Two things are deliberately not in it: the view's `clue_shown`, since a clue
  costs nothing and re-reading one costs nothing either (revisit if item 17
  ever prices it), and any RNG seed — the word order is already decided by the
  stored pool, so all a seed would still buy is which letter a hint picks.
  Like `game.rs` it holds **no GPUI types** and is covered by 32 in-file
  tests; the conversions to `ThemeMode` and `Bounds<Pixels>` live in
  `src/ui/mod.rs` instead. Nothing in it may fail loudly: every read error
  falls back to `Settings::default()`, and a malformed `stats` key falls back
  on its own rather than taking the file with it.
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

The **scrollbar mode is the same story**: `theme::init` also calls
`sync_scrollbar_appearance`, which picks `Scrolling` or `Hover` from the
desktop's auto-hide preference (`theme/mod.rs:215-223`). `main.rs` overrides it
with `Theme::set_scrollbar_mode(ScrollbarMode::Always, cx)` **after**
`gpui_kit::init` for the same reason, because the play column is the only thing
in the window that scrolls and the other two modes only show the bar once you
are already scrolling. Unlike the theme mode, this one *survives*
`Theme::change` — `apply_config` does not touch it and `set_scrollbar_mode`
syncs the Base projection itself — so the light/dark toggle does not undo it and
it needs setting exactly once.

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

`OVERLAY_DIM_LIGHT` / `OVERLAY_DIM_DARK` in `src/ui/mod.rs` override it to 45%
and 60%, which measure 250 → 137 and 22 → 9. Note that the two numbers are not
a pair: light is *lower* and dims *more*, because white has further to fall.
The override is applied in `apply_theme` and gotcha 2 is why it has to live
there rather than at startup.

**The dialog's position cannot be overridden, but the margin in front of it
can.** `Dialog::render` puts the box at `x = width / 2 - dialog / 2` and
`y = margin_top.unwrap_or(view_size.height / 10.)`
(`gpui-component-0.6.0/src/dialog/dialog.rs:498-499`), and that pair is applied
*after* the caller's own `refine_style` under the comment "There style is high
priority, can't be overridden" — then `top` is set a second time inside the open
animation as `top(y * delta)`. So styling `top` is a dead end, and it is one
that fails silently. What works is that the box is positioned **`relative`**,
not absolute: its `top` is an offset from wherever the box would otherwise sit,
and an ordinary top margin moves that. `AlertDialog` implements `Styled`
straight onto the surface it builds (`dialog/alert_dialog.rs:313`), so
`.mt(px(n))` on it moves the dialog down by exactly `n` — verified on captures
at `n = 200`. `DIALOG_TOP_FRACTION` and `dialog_top_margin` in `src/ui/mod.rs`
are that margin, put where it lands the top edge three tenths down the window
rather than gpui-kit's one tenth.

Two things that do **not** work, so nobody has to re-derive them: auto margins
(`my_auto`) move nothing here, because this layout distributes no free space;
and `margin_top`, which would express the offset directly, is on `Dialog` with
no passthrough from `AlertDialog` — and swapping to `Dialog` costs the header
block and the OK/Cancel footer that `AlertDialog` composes (`Dialog::header` is
`pub(crate)`, and a bare `Dialog` renders no buttons of its own), as well as the
structural refusal to dismiss on a backdrop click. The margin is also the reason
the placement is a fraction of the window rather than true centring: the
dialog's own height is not known until it has been laid out, which is after the
builder that would need it has run.
