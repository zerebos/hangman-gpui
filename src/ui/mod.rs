//! The GPUI user interface: one dark-first window, laid out as a game board.
//!
//! The window is a title bar (wordmark plus a light/dark toggle), a toolbar
//! strip carrying the original's `Game` menu, and a body split into a left
//! play column — scoreboard, word, keyboard, result — and a right stage panel
//! holding the gallows drawing. Every colour comes from a gpui-kit theme
//! token, so both themes are usable and neither is hard-coded.

mod gallows;

use std::time::Duration;

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{
    Button, ButtonCustomVariant, ButtonGroup, ButtonVariant, ButtonVariants as _,
};
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{
    ActiveTheme as _, Colorize as _, Disableable as _, Icon, IconName, Root, Selectable as _,
    Sizable as _, StyledExt as _, Theme, ThemeMode, TitleBar, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::audio::Audio;
use crate::game::{Cell, Difficulty, Game, GameResult, GuessResult, HintResult, MatchOutcome};
use crate::settings::{Rect, Settings, ThemeChoice, WindowFrame};
use crate::stats::{DifficultyStats, Session, Stats};
use gallows::gallows;

/// The key context this view claims. Key bindings registered against it (see
/// `main.rs`) only fire while something inside the view has focus.
pub const KEY_CONTEXT: &str = "Hangman";

/// The size the window opens at when there is nothing saved to restore.
///
/// The height is picked so the play column does not scroll in the state that
/// needs the most room *of the ones that fit at all*: a finished word, where
/// the result panel replaces the one-line status. That measures 771px with the
/// shortcut bar in place, so this is that plus slack, because a wrapped alert
/// line or the end-of-match footer would put an exact fit straight back into
/// scrolling.
///
/// It is deliberately not a promise that nothing ever scrolls. The lifetime
/// stats fold out inline under the result, and that state wants a window
/// around 1110px tall — bigger than most laptops have — so the column has to
/// stay scrollable whatever this says.
pub const DEFAULT_WINDOW_SIZE: Size<Pixels> = size(px(1000.), px(800.));
/// The smallest the window may be. Below this the toolbar wraps into the board
/// and the stage column starts clipping the artwork.
///
/// This is a floor on what stays *usable*, not on what fits without
/// scrolling — see [`DEFAULT_WINDOW_SIZE`] for why the two cannot be the same
/// number.
pub const MIN_WINDOW_SIZE: Size<Pixels> = size(px(880.), px(660.));

// The original's alert strings, verbatim.
const GAME_LOST: &str = "Bring Add/Drop Form!";
const GAME_WON: &str = "You WIN!";
const GAVE_UP: &str = "Giving up counts as a loss in my book.";
const INVALID_GUESS: &str = "You managed to guess an invalid character, congrats.";
const FILE_ERROR: &str = "Sorry, we couldnt read in your file.";

/// What the status line says when there is nothing else to report.
const IDLE_HINT: &str = "Type a letter, or click one above.";

/// What the Hint button promises, and the two reasons it can be off.
///
/// The price is in the tooltip rather than behind the click because it is a
/// real one: a hint spends a wrong guess, which is a body part on the gallows
/// and ten points off the word.
///
/// None of the three names its chord any more. Every toolbar button that has
/// one is built with `tooltip_with_action`, which draws the binding the app
/// actually registered as a `Kbd` chip beside the text — so the chord is read
/// out of the keymap rather than typed twice, and it spells itself the way the
/// platform does (`Ctrl+H` on Windows and Linux, `⌃H` on macOS).
const HINT_TOOLTIP: &str = "Reveal a letter — costs one wrong guess";
const HINT_TOOLTIP_LAST_GUESS: &str = "No hint: it would cost the last guess you have";
const HINT_TOOLTIP_OVER: &str = "No hint: this word is already finished";

/// The `Reset stats` confirmation, which is the one dialog in the window.
///
/// Its wording carries the whole point of asking: the button is one click from
/// a tally built up over weeks, and `Reset` on its own does not say what goes.
const RESET_TITLE: &str = "Reset lifetime stats?";
const RESET_OK: &str = "Reset";
const RESET_CANCEL: &str = "Keep them";
/// The dialog is reachable with nothing saved — a first launch with the stats
/// panel open — and a warning about losing nothing would be a lie.
const RESET_NOTHING: &str = "There is nothing saved yet, so this costs you nothing.";
/// Said after the numbers, and the reason the dialog exists at all.
///
/// It names the match score as well as the lifetime tally because
/// `Session::reset_stats` clears both: `SCORE` on the board is the points the
/// match on screen has earned, and those words are being unmade. The word
/// itself — its letters, its guesses, its gallows — survives, which is the
/// half a player is most likely to be worried about.
const RESET_FOREVER: &str = "go back to zero in every difficulty, along with the score of the match you \
     are playing. There is no undo, though the word itself is not touched.";

/// The alert line after a hint lands. It names the letter, because the word
/// row is not the only place the player is looking, and it says what the hint
/// cost, because the pip that just turned red does not explain itself.
const HINT_GIVEN: &str = "That cost you a guess:";

// The original's letter grid: seven buttons per row, with V-Z indented by one
// cell because the Java wrap rule started the fourth row at column 1.
const LETTERS_PER_ROW: usize = 7;
const INDENTED_ROW: usize = 3;

/// One square letter key. Square, so the grid reads as a keyboard.
const KEY_SIZE: Pixels = px(42.);
/// The gap between keys, and between the cells of the word.
const KEY_GAP: Pixels = px(6.);

/// The label column of the per-difficulty breakdown, sized for its own
/// "DIFFICULTY" heading rather than for the four short names under it.
const BREAKDOWN_LABEL_WIDTH: Pixels = px(88.);

/// The stage column's width: the 300px drawing plus its panel padding.
const STAGE_WIDTH: Pixels = px(332.);

/// The channel between the play column and the stage, matching the `p_5` that
/// frames the board on its other three sides.
///
/// The play column does not get all of it. It reserves [`SCROLLBAR_GUTTER`] out
/// of its own right edge and the flex gap is the remainder, so the two add back
/// up to this and the board looks the same whether the scrollbar is there or
/// not.
const COLUMN_GAP: Pixels = px(20.);
/// The strip kept clear down the right of the play column for its scrollbar.
///
/// gpui-kit paints the bar as an overlay across the scroll area rather than as
/// a sibling that takes room, so without this it would sit on top of the right
/// edge of the scoreboard and word panels. 16px is the bar's full track width
/// (`gpui-base-0.6.0/src/scrollbar.rs:21`, `THUMB_ACTIVE_INSET * 2 +
/// THUMB_ACTIVE_WIDTH`). The strip is reserved whether or not the column is
/// currently overflowing: the alternative is measuring the content to decide,
/// which means last frame's layout deciding this one's — and it would shift
/// every panel sideways the moment a word ended.
const SCROLLBAR_GUTTER: Pixels = px(16.);

/// How wide one character of the word is, and how tall its glyph row is.
const WORD_CELL_WIDTH: Pixels = px(34.);
const WORD_CELL_HEIGHT: Pixels = px(38.);
/// The word is spaced wider than the keyboard: it should read as a puzzle.
const WORD_GAP: Pixels = px(10.);
/// The rule drawn under each guessable character.
const WORD_RULE_HEIGHT: Pixels = px(3.);

// ---------------------------------------------------------------- settings
//
// `crate::settings` is deliberately free of UI types so it can be tested
// without a window, which leaves the translation between its plain data and
// gpui's own types here — in the one module that already speaks both.

impl From<ThemeChoice> for ThemeMode {
    fn from(choice: ThemeChoice) -> Self {
        match choice {
            ThemeChoice::Dark => ThemeMode::Dark,
            ThemeChoice::Light => ThemeMode::Light,
        }
    }
}

impl From<ThemeMode> for ThemeChoice {
    fn from(mode: ThemeMode) -> Self {
        if mode.is_dark() {
            ThemeChoice::Dark
        } else {
            ThemeChoice::Light
        }
    }
}

/// How far the dialog backdrop dims the board, per theme.
///
/// gpui-kit's own token is black at 5% in light and 20% in dark
/// (`gpui-component-0.6.0/src/theme/default-theme.json`), and both are too
/// weak to read as a modal here. The dark one is the instructive case: it is
/// four times the alpha of the light one and lands *softer*, because it is a
/// black wash over a board that is already near-black and there is almost
/// nothing left to darken. So the two numbers are not one number and its
/// counterpart — light needs less because white has further to fall.
const OVERLAY_DIM_LIGHT: f32 = 0.45;
const OVERLAY_DIM_DARK: f32 = 0.6;

/// Set the theme, and re-apply the one token this game overrides.
///
/// **Both callers must come through here**, `main.rs` at startup as much as
/// the toggle in the title bar. `Theme::change` rebuilds the whole colour set
/// from the theme config (`theme/mod.rs:245-255`), so an overlay written once
/// at startup is silently reverted by the first press of the toggle: the
/// dialog would dim properly until you changed theme, and never again.
pub fn apply_theme(mode: impl Into<ThemeMode>, window: Option<&mut Window>, cx: &mut App) {
    let mode = mode.into();

    // `None`, not `window`: `Theme::change` refreshes at the end, which would
    // repaint with the token it has just reset. The refresh is done below,
    // once the override is back in.
    Theme::change(mode, None, cx);

    let dim = if mode.is_dark() {
        OVERLAY_DIM_DARK
    } else {
        OVERLAY_DIM_LIGHT
    };
    Theme::global_mut(cx).colors.overlay = hsla(0., 0., 0., dim);
    // The Base layer holds its own copy of the tokens and a field written
    // straight onto `Theme` does not reach it. `overlay` is read by
    // gpui-component rather than Base, so this is not load-bearing today — it
    // is the documented contract for `global_mut`, and the next token someone
    // overrides here may well need it.
    Theme::sync_base(cx);

    if let Some(window) = window {
        window.refresh();
    }
}

/// Where to open the window: where the last run left it, if that still lands on
/// a display that exists, and the centred default otherwise.
///
/// Called from `main.rs` before the window exists, which is why it takes the
/// app rather than a window.
pub fn window_bounds(settings: &Settings, cx: &App) -> WindowBounds {
    // `visible_bounds` is the part of each display a window can actually use:
    // it leaves out the taskbar, the dock and any other reserved strip.
    let displays: Vec<Rect> = cx
        .displays()
        .iter()
        .map(|display| to_rect(display.visible_bounds()))
        .collect();

    let restored = settings.window.and_then(|frame| {
        let rect = frame.rect.fit_onto(
            &displays,
            MIN_WINDOW_SIZE.width.as_f32(),
            MIN_WINDOW_SIZE.height.as_f32(),
        )?;
        Some((to_bounds(rect), frame.maximized))
    });

    match restored {
        Some((bounds, true)) => WindowBounds::Maximized(bounds),
        Some((bounds, false)) => WindowBounds::Windowed(bounds),
        None => WindowBounds::centered(DEFAULT_WINDOW_SIZE, cx),
    }
}

/// The window as it stands right now, in the form the settings file stores.
fn window_frame(window: &Window) -> WindowFrame {
    let bounds = window.window_bounds();
    WindowFrame {
        // For a maximized or full-screen window this is the *restore* size, the
        // one it springs back to — which is the size worth remembering.
        rect: to_rect(bounds.get_bounds()),
        maximized: !matches!(bounds, WindowBounds::Windowed(_)),
    }
}

fn to_rect(bounds: Bounds<Pixels>) -> Rect {
    Rect::new(
        bounds.origin.x.as_f32(),
        bounds.origin.y.as_f32(),
        bounds.size.width.as_f32(),
        bounds.size.height.as_f32(),
    )
}

fn to_bounds(rect: Rect) -> Bounds<Pixels> {
    Bounds {
        origin: point(px(rect.x), px(rect.y)),
        size: size(px(rect.width), px(rect.height)),
    }
}

// ------------------------------------------------------------------ motion
//
// Everything below is UI feel only: no timing here can change a rule, a score
// or a layout dimension. Each effect is a one-shot animation whose end state is
// exactly how the element looks at rest, so a finished animation leaves the
// window looking as it did before any of this existed.
//
// None of them checks a motion preference, on purpose: `with_animation` already
// honours `App::reduce_motion` for us, rendering a one-shot animation's end
// state and scheduling no frames at all
// (`gpui-pre-0.3.3/src/elements/animation.rs:406-419`).

/// How long the word row's wrong-guess shake lasts.
const SHAKE: Duration = Duration::from_millis(320);
/// How far the shake throws the row, at its widest.
const SHAKE_DISTANCE: f32 = 5.;
/// How many sine cycles it runs through. A multiple of a half cycle, so the
/// wave lands back on zero and the row ends exactly where it started.
const SHAKE_CYCLES: f32 = 2.5;

/// The win flourish: the whole word resolving left to right, once, as the
/// celebration for having finished it.
const WIN_REVEAL: Reveal = Reveal {
    fade: 0.26,
    step: 0.055,
    rise: 7.,
};
/// A correct guess mid-game: only the cells that letter just turned over.
/// Quicker and tighter than the win, and it moves less far, because this is
/// feedback on one guess rather than the end of the word.
const GUESS_REVEAL: Reveal = Reveal {
    fade: 0.2,
    step: 0.07,
    rise: 5.,
};

/// One wrong-guess pip.
const PIP_SIZE: Pixels = px(9.);
/// How far past the pip its pulse expands before fading out.
const PIP_HALO: Pixels = px(7.);
/// How solid that halo is when it starts.
const PIP_HALO_ALPHA: f32 = 0.45;
/// How long the pulse takes.
const PIP_PULSE: Duration = Duration::from_millis(420);

/// How long a letter key takes to settle into the colour of the guess it just
/// took. Short: this is the answer to a keypress, so it has to feel prompt.
const KEY_SETTLE: Duration = Duration::from_millis(240);

/// The word row's horizontal offset, `delta` of the way through a shake.
///
/// A sine wave damped to nothing, so it starts and ends at zero however it is
/// sampled — a shake that stopped off-centre would move the row for good.
fn shake_offset(delta: f32) -> f32 {
    SHAKE_DISTANCE * (1. - delta) * (delta * SHAKE_CYCLES * std::f32::consts::TAU).sin()
}

/// A staggered fade-in: letters resolving one after another, each rising the
/// last few pixels into place as it appears.
///
/// One shape, two settings: the word row uses it for the win flourish and for
/// the cells a correct guess just turned over, which differ only in how fast
/// and how far they move.
#[derive(Clone, Copy)]
struct Reveal {
    /// How long one letter's own fade lasts, in seconds.
    fade: f32,
    /// How far apart consecutive letters start, in seconds. This is the stagger.
    step: f32,
    /// How far below its place a letter starts, in pixels.
    rise: f32,
}

impl Reveal {
    /// How long the whole animation of the letter at `index` runs: its share of
    /// the stagger, and then its own fade.
    fn span(self, index: usize) -> Duration {
        Duration::from_secs_f32(self.step * index as f32 + self.fade)
    }

    /// How far the letter at `index` is through its own fade, `delta` of the way
    /// through [`Reveal::span`].
    ///
    /// The animation runs on a linear easing, so `delta` times the span is
    /// elapsed seconds; the letter sits still until its slot comes round and is
    /// finished at `delta == 1`, whatever its index.
    fn progress(self, index: usize, delta: f32) -> f32 {
        let delay = self.step * index as f32;
        let span = delay + self.fade;
        ((delta * span - delay) / self.fade).clamp(0., 1.)
    }

    /// The glyph at `index`, drawn `delta` of the way through the reveal.
    ///
    /// Only the paint moves: `top` on a relative element is an offset, so the
    /// row measures the same the whole way through, and at `delta == 1` the
    /// glyph is fully opaque and exactly where it belongs.
    fn draw(self, glyph: Div, index: usize, delta: f32) -> Div {
        let progress = ease_in_out(self.progress(index, delta));
        glyph.opacity(progress).top(px(self.rise * (1. - progress)))
    }
}

/// `from` blended `progress` of the way towards `to`, in Oklab.
///
/// `Colorize::mix_oklab` weights its *receiver* by the factor it is handed
/// (`gpui-component-0.6.0/src/theme/color.rs:212`), which reads backwards from
/// a lerp, hence the inversion: a progress of 0 is all `from`, 1 all `to`.
/// Oklab rather than channel-wise, so a near-grey key on its way to green does
/// not detour through a hue that is in neither end state.
fn blend(from: Hsla, to: Hsla, progress: f32) -> Hsla {
    from.mix_oklab(to, 1. - progress)
}

/// `"840 points"`, and `"1 point"` — because the summary line reads as a
/// sentence and `1 points` in it would be the only thing anyone noticed.
fn points(points: u32) -> String {
    plural(points.into(), "point")
}

/// `"3 words"`, `"1 word"` — the same rule as [`points`], for any noun that
/// takes a plain `s`.
fn plural(count: u64, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// What the `Reset stats` dialog says is about to be thrown away.
///
/// Pure, and separate from the dialog that shows it, for the reason the rest
/// of this crate splits that way: the component is gpui-kit's and can only be
/// judged by eye, but the sentence in it is arithmetic over [`Stats`] and a
/// test can hold it to account. It names the three numbers the panel behind
/// the dialog is showing, so the player is warned in the same terms they were
/// just reading.
fn reset_stats_summary(stats: &Stats) -> String {
    // `Stats::is_empty`, not the three numbers quoted below: those are what a
    // player would notice, but the reset clears every field, and a hand-edited
    // file can hold a match count or one difficulty's bucket with none of the
    // three behind it. Promising it costs nothing would then be a lie.
    if stats.is_empty() {
        return RESET_NOTHING.to_string();
    }

    let played = u64::from(stats.words_played());

    format!(
        "{}, {} and a best streak of {} {RESET_FOREVER}",
        plural(stats.points, "point"),
        plural(played, "word"),
        stats.best_streak,
    )
}

/// How far down the window the dialog's top edge should sit, and how far down
/// gpui-kit puts it on its own.
///
/// `Dialog::render` positions the box at `view_size.height / 10.`
/// (`gpui-component-0.6.0/src/dialog/dialog.rs:498`) and that `top` cannot be
/// overridden: it is applied after the caller's own style, and set a second
/// time inside the open animation as `top(y * delta)`. What *can* be moved is
/// where the box starts from — the popup is positioned `relative`, so its
/// `top` is an offset from its flow position, and an ordinary top margin
/// moves that. [`dialog_top_margin`] is the difference between the two
/// fractions, and `AlertDialog` takes it as a plain `.mt()` because it
/// implements `Styled`.
///
/// A tenth of the way down reads as hung off the top edge rather than placed,
/// the more so since the default window became 1000 × 800. A third is the
/// familiar spot: just above the true centre, which is where a modal is
/// expected and where dead centre would look slightly low.
const DIALOG_TOP_FRACTION: f32 = 0.3;
const GPUI_KIT_DIALOG_TOP_FRACTION: f32 = 0.1;

/// The top margin that moves the dialog from gpui-kit's tenth to
/// [`DIALOG_TOP_FRACTION`], for a window this tall.
///
/// A fraction of the window rather than `(window - dialog) / 2` because the
/// dialog's own height is not known until it has been laid out, and the
/// builder that would need it runs before that. The trade is that this is a
/// placement, not an exact centring — but it is one that holds at every
/// window size, and the builder re-runs on every frame, so it follows a
/// resize.
fn dialog_top_margin(window_height: f32) -> f32 {
    (window_height * (DIALOG_TOP_FRACTION - GPUI_KIT_DIALOG_TOP_FRACTION)).max(0.)
}

/// A whole-number percentage of a `0.0..=1.0` rate, e.g. `"75%"`.
fn percent(rate: f32) -> String {
    format!("{:.0}%", rate * 100.)
}

/// Whether a gpui-kit dialog is on screen.
///
/// The dialog layer is rendered as a child of the very element that owns this
/// window's key context and its `on_key_down`, so a key pressed while a dialog
/// is up still bubbles all the way here: without this guard a letter typed at
/// the confirmation would be guessed on the board behind it, and Ctrl+H would
/// spend a wrong guess the player cannot see happen. The dialog's own Escape
/// and Enter never reach this — gpui-kit binds them to `Cancel`/`Confirm` in
/// the dialog's own `Dialog` key context, which is closer to the focus.
fn dialog_is_open(window: &mut Window, cx: &mut App) -> bool {
    window.has_active_dialog(cx)
}

actions!(hangman, [OpenWordList, ChangeWord, Hint]);

// ------------------------------------------------------------ keyboard legend
//
// A tooltip only tells you about a shortcut once you already suspect there is
// one — you have to hover the button to find out that you never needed the
// button. So the shortcuts also live in a strip along the bottom of the
// window, on screen the whole time, next to nothing else competing for the
// row.

/// One entry of the keyboard legend.
///
/// The three chords are the original's `Game` menu accelerators plus `Ctrl+H`,
/// and they are the app's standing shortcuts: always listed, greyed when the
/// key would currently do nothing, on the same reasoning that keeps the
/// disabled `Hint` button on screen instead of hiding it. `NextWord` is the
/// odd one out — it means something only once a word has ended — so it is
/// listed only while it works rather than sitting greyed through every game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shortcut {
    Hint,
    ChangeWord,
    OpenWordList,
    NextWord,
}

impl Shortcut {
    /// What the key does, in the fewest words that still say it.
    fn label(self) -> &'static str {
        match self {
            Shortcut::Hint => "Reveal a letter",
            Shortcut::ChangeWord => "Give up on this word",
            Shortcut::OpenWordList => "Open a word list",
            Shortcut::NextWord => "Next word",
        }
    }
}

/// A legend entry and whether pressing it right now would change anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ShortcutHint {
    shortcut: Shortcut,
    /// `false` greys the entry: the key is real, but the game is in a state
    /// that refuses it — the same three refusals the toolbar already models.
    live: bool,
}

/// What the legend says about the game in hand.
///
/// Kept out of the render tree, and free of GPUI types, so the rule about
/// which keys are offered when is a plain function a test can call — the view
/// itself opens a window and cannot be.
fn shortcut_legend(game: &Game) -> Vec<ShortcutHint> {
    // The three that are always listed. `Game::give_up` refuses on a word that
    // has already ended, and `Game::can_hint` refuses on the last guess and on
    // a finished word; loading a list of your own is never refused.
    let mut hints = vec![
        ShortcutHint {
            shortcut: Shortcut::Hint,
            live: game.can_hint(),
        },
        ShortcutHint {
            shortcut: Shortcut::ChangeWord,
            live: !game.is_game_over(),
        },
        ShortcutHint {
            shortcut: Shortcut::OpenWordList,
            live: true,
        },
    ];
    // Exactly the condition `Game::new_game` deals a word under, so the
    // legend never offers Enter when the result panel is showing the match
    // summary instead of a `New Game` button.
    if game.is_game_over() && !game.is_match_over() {
        hints.push(ShortcutHint {
            shortcut: Shortcut::NextWord,
            live: true,
        });
    }
    hints
}

/// The chip for one legend entry, as the keymap currently spells it.
///
/// `None` means nothing is bound to that action, which is why the entry is
/// dropped rather than drawn with a blank key.
fn shortcut_kbd(shortcut: Shortcut, window: &Window) -> Option<Kbd> {
    match shortcut {
        Shortcut::Hint => Kbd::binding_for_action(&Hint, Some(KEY_CONTEXT), window),
        Shortcut::ChangeWord => Kbd::binding_for_action(&ChangeWord, Some(KEY_CONTEXT), window),
        Shortcut::OpenWordList => Kbd::binding_for_action(&OpenWordList, Some(KEY_CONTEXT), window),
        // The one key here with no action behind it. Enter and Space are
        // handled in `on_key_down` rather than bound, so that they keep
        // activating whichever button has been tabbed to and only mean "next
        // word" while the board itself holds focus — a `KeyBinding` would fire
        // either way. There is therefore nothing in the keymap to read, and
        // this is the only chord in the window still spelled out by hand.
        Shortcut::NextWord => Keystroke::parse("enter").ok().map(Kbd::new),
    }
}

/// A line of feedback, styled after the original's `alertMessage` label:
/// italic, and green for good news or red for bad.
///
/// Named `Notice` rather than `Alert` because gpui-kit ships an `Alert`
/// component, which the result panel below uses.
#[derive(Debug, Clone)]
struct Notice {
    text: SharedString,
    good: bool,
}

impl Notice {
    fn good(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            good: true,
        }
    }

    fn bad(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            good: false,
        }
    }
}

/// How one letter key should look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyState {
    /// Not guessed yet, and the game is still on: clickable.
    Available,
    /// Guessed, and it was in the word.
    Correct,
    /// Guessed, and it was not.
    Wrong,
    /// The game is over, so this key is out of play.
    OutOfPlay,
}

/// The panel surface every card in the window is drawn on.
///
/// gpui-kit's theme has no `card` token, so a panel is the `muted` surface
/// laid over the window background at partial alpha, ringed by `border`. That
/// lands one step above the page in both themes — a shade lighter than black
/// in dark mode, a shade darker than white in light — instead of picking a
/// literal colour that could only be right in one of them.
fn panel(cx: &App) -> Div {
    v_flex()
        .bg(cx.theme().muted.alpha(0.45))
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg)
}

/// A small upper-case section label, the quietest text in the window.
fn eyebrow(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .font_semibold()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// The whole Hangman window.
///
/// The [`Game`] is stored inline rather than behind an `Entity`. An `Entity`
/// buys shared ownership and change subscriptions across views; there is only
/// one view here, so it would be pure ceremony.
pub struct HangmanView {
    game: Game,
    /// The most recent per-game message, or `None` for the original's idle
    /// blank line. The end-of-match message is derived on the fly instead, in
    /// [`HangmanView::match_summary`].
    notice: Option<Notice>,
    /// The letter of the last guess that actually landed — a correct or a
    /// wrong one, never a duplicate or an invalid character. It is what tells
    /// the word row which cells this guess just turned over and the keyboard
    /// which key to settle, so it lives here rather than in [`Game`]: it is a
    /// fact about the last frame, not about the rules. Cleared whenever a
    /// fresh word starts, so nothing animates at the top of a game.
    last_guess: Option<char>,
    /// GPUI only delivers key events to elements on the focus path, so the root
    /// element has to own a focus handle and actually be focused before typing
    /// a letter can reach us.
    focus_handle: FocusHandle,
    /// The win/loss cues. Without the `sound` feature this is a zero-sized
    /// no-op, so the calls below need no `#[cfg]` of their own. It has to be
    /// owned here rather than created per clip: it holds the output device open.
    audio: Audio,
    /// The choices that outlive the process, as they were loaded at startup and
    /// as they stand now. Every field that changes is written straight back to
    /// disk from the handler that changed it, so this is only ever a mirror of
    /// the file rather than state waiting to be flushed.
    settings: Settings,
    /// The score: the match on screen, and the lifetime tally behind it. All
    /// the arithmetic lives in [`crate::stats`], so nothing in this module
    /// adds a point up for itself.
    session: Session,
    /// Whether the lifetime stats panel is expanded under the board. Pure view
    /// state — nothing here is remembered between launches.
    show_stats: bool,
}

impl HangmanView {
    /// Build the view from the settings the last run left behind, which
    /// `main.rs` has already used to pick the theme and the window's bounds.
    pub fn new(settings: Settings, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // The window's own geometry is the one setting with no moment of
        // change to save on: a drag or a resize reports every intermediate
        // pixel, so writing there would mean a file write per frame. Its
        // settled value is the one it closes at, so that is when it is written.
        //
        // Closing the window takes two different routes, and the game should
        // remember its place whichever one is taken. The operating system's own
        // close — the title bar's X on Windows and macOS, the traffic light, the
        // window menu, Alt+F4 — arrives here.
        let view = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            view.update(cx, |view, _| view.save_window_frame(window))
                .ok();
            true
        });

        Self {
            game: Game::new(settings.difficulty.unwrap_or_default()),
            notice: None,
            last_guess: None,
            focus_handle: cx.focus_handle(),
            audio: Audio::new(),
            // The streak and the lifetime totals pick up exactly where the last
            // run left them; the match score starts at zero, because the match
            // on screen has only just been dealt.
            session: Session::new(settings.stats.clone()),
            show_stats: false,
            settings,
        }
    }

    // ---------------------------------------------------------------- actions

    fn guess(&mut self, letter: char, cx: &mut Context<Self>) {
        let outcome = self.game.guess(letter);
        if outcome.result == GuessResult::Ignored {
            return;
        }

        // Only a guess that changed something is worth animating. A duplicate
        // or an invalid character leaves the previous guess's letter in place,
        // whose animations have long since finished, so nothing replays.
        if matches!(outcome.result, GuessResult::Correct | GuessResult::Wrong) {
            self.last_guess = Some(letter.to_ascii_uppercase());
        }

        // Score it before the message is built: the win line quotes the points.
        let earned = self.record(outcome.game, outcome.match_);

        self.notice = match outcome.result {
            GuessResult::Invalid => Some(Notice::bad(INVALID_GUESS)),
            // The original cleared the line on any ordinary guess, and only the
            // guess that ends the game leaves a message behind.
            _ => match outcome.game {
                // The two moments the Java played a cue. Giving up is handled
                // in `give_up`, which stays silent just as the original did.
                Some(GameResult::Won) => {
                    self.audio.play_win();
                    Some(Notice::good(format!("{GAME_WON} +{earned}")))
                }
                Some(GameResult::Lost) => {
                    self.audio.play_loss();
                    Some(Notice::bad(GAME_LOST))
                }
                None => None,
            },
        };

        // GPUI does not diff state: a mutated view is only redrawn if it says so.
        cx.notify();
    }

    fn give_up(&mut self, cx: &mut Context<Self>) {
        // The guard is what keeps a finished word from being scored twice:
        // `Game::give_up` already refuses, but it refuses silently, and the
        // recording below would happily count a second loss.
        if self.game.is_game_over() {
            return;
        }
        let match_ = self.game.give_up();
        // A word given up on is a word lost: no points, and the streak ends.
        self.record(Some(GameResult::Lost), match_);
        self.notice = Some(Notice::bad(GAVE_UP));
        cx.notify();
    }

    /// Buy a letter with a wrong guess.
    ///
    /// Everything after the call is the guess path verbatim, because as far as
    /// the rest of the view is concerned a hint *is* a guess: the letter goes
    /// into `last_guess`, so the cells it turns over fade up and its key
    /// settles into the colour a correct guess earns; the wrong-guess counter
    /// it spent moves the pips and draws the next body part; and the word can
    /// end on it, which is scored exactly as any other win.
    fn hint(&mut self, cx: &mut Context<Self>) {
        let outcome = self.game.hint();
        // Refused — the button was disabled, or the shortcut was pressed
        // anyway. `Game::hint` changed nothing, so neither does this.
        let HintResult::Revealed(letter) = outcome.result else {
            return;
        };
        self.last_guess = Some(letter);

        let earned = self.record(outcome.game, outcome.match_);
        self.notice = match outcome.game {
            // A hint can complete the word, and that is a win like any other.
            Some(GameResult::Won) => {
                self.audio.play_win();
                Some(Notice::good(format!("{GAME_WON} +{earned}")))
            }
            // Unreachable: `Game::hint` refuses at one guess left precisely so
            // a hint can never be the guess that loses the word. Handled
            // rather than asserted, so the rules stay the rules module's.
            Some(GameResult::Lost) => {
                self.audio.play_loss();
                Some(Notice::bad(GAME_LOST))
            }
            None => Some(Notice::good(format!("{HINT_GIVEN} {letter}."))),
        };
        cx.notify();
    }

    fn on_hint(&mut self, _: &Hint, window: &mut Window, cx: &mut Context<Self>) {
        if dialog_is_open(window, cx) {
            return;
        }
        self.hint(cx);
    }

    /// Put a finished word — and, when it was the last of the match, the match
    /// — on the scoreboard, then write the new lifetime tally to disk.
    ///
    /// Returns what the word scored, which is 0 for anything but a win. Called
    /// with the `game`/`match_` fields of a [`crate::game::GuessOutcome`], so
    /// an ordinary guess passes `None` and this does nothing at all.
    fn record(&mut self, game: Option<GameResult>, match_: Option<MatchOutcome>) -> u32 {
        let Some(result) = game else {
            return 0;
        };

        // Read *after* the guess landed, which is what the word is worth: the
        // budget still unspent when the word was finished.
        let difficulty = self.game.difficulty();
        let earned = self
            .session
            .record_word(difficulty, result, self.game.remaining_guesses());
        if let Some(outcome) = match_ {
            self.session.record_match(difficulty, outcome);
        }

        // The stats are only ever a mirror of the session, like every other
        // setting: written the moment they change rather than at exit.
        self.settings.stats = self.session.stats().clone();
        self.settings.save();
        earned
    }

    /// Ask before throwing the lifetime tally away, then do it if told to.
    ///
    /// The window's one dialog, and deliberately its most destructive button:
    /// everything else here is either reversible or costs a single word, so
    /// this is the only place an interruption earns its keep.
    ///
    /// gpui-kit's `AlertDialog` is a good fit for exactly that shape. It is
    /// the opinionated wrapper over `Dialog`: no close ✕, no dismissal by
    /// clicking the backdrop — `AlertDialog::overlay_closable` is deprecated
    /// to a no-op rather than merely defaulted off — so the only ways out are
    /// the two buttons and Escape, which is what a confirm should offer. Both
    /// callbacks return a `bool` saying whether to close, so a dialog can
    /// refuse to go; this one always accepts.
    ///
    /// The callbacks are plain `Fn(&ClickEvent, &mut Window, &mut App)`, not
    /// `cx.listener`s, so reaching the view from inside means a `WeakEntity`
    /// — and the builder is an `Fn` re-run on every frame the dialog is on
    /// screen, so everything it captures has to be cloned rather than moved.
    fn confirm_reset_stats(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let summary = reset_stats_summary(self.session.stats());
        let view = cx.weak_entity();

        window.open_alert_dialog(cx, move |alert, window, cx| {
            let view = view.clone();
            alert
                .mt(px(dialog_top_margin(
                    window.viewport_size().height.as_f32(),
                )))
                .icon(Icon::new(IconName::TriangleAlert).text_color(cx.theme().red))
                .title(RESET_TITLE)
                .description(summary.clone())
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(RESET_OK)
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text(RESET_CANCEL)
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    view.update(cx, |this, cx| this.reset_stats(cx)).ok();
                    true
                })
        });
    }

    /// Throw the lifetime tally away, once [`Self::confirm_reset_stats`] has
    /// been answered.
    fn reset_stats(&mut self, cx: &mut Context<Self>) {
        self.session.reset_stats();
        self.settings.stats = self.session.stats().clone();
        self.settings.save();
        cx.notify();
    }

    fn toggle_stats(&mut self, cx: &mut Context<Self>) {
        self.show_stats = !self.show_stats;
        cx.notify();
    }

    fn new_game(&mut self, cx: &mut Context<Self>) {
        // The *next word* of the match, not a new match — so the match score
        // deliberately carries on adding up here. `Session::start_match` is
        // called only where a fresh word pool is dealt: `set_difficulty` and
        // `load_word_list`.
        //
        // Returns false once the word list is exhausted; the footer shows the
        // match summary in that case rather than a button that does nothing.
        if self.game.new_game() {
            self.notice = None;
            self.last_guess = None;
            cx.notify();
        }
    }

    /// The word that is about to be thrown away, if throwing it away costs
    /// anything: its text, and the difficulty it belongs to.
    ///
    /// Both have to be read *before* the throw, because dealing a new pool
    /// takes the word with it and may change the difficulty out from under the
    /// loss — which belongs to the list the word came from, not the one being
    /// switched to.
    fn word_being_abandoned(&self) -> Option<(String, Option<Difficulty>)> {
        self.game
            .has_word_to_lose()
            .then(|| (self.game.word().to_string(), self.game.difficulty()))
    }

    /// Charge a loss for a word walked away from, and say so.
    ///
    /// This is [`HangmanView::record`] for the abandon case, with `difficulty`
    /// passed in rather than read back off the game for the reason above. The
    /// other two arguments `record` reads are not needed: an abandoned word is
    /// lost, a lost word is worth no points, so the unspent budget never enters
    /// the arithmetic, and the match is being discarded rather than finished so
    /// there is no `MatchOutcome` to record.
    fn record_abandoned(&mut self, word: &str, difficulty: Option<Difficulty>) -> Notice {
        self.session.record_word(difficulty, GameResult::Lost, 0);
        self.settings.stats = self.session.stats().clone();
        self.settings.save();
        Notice::bad(format!("Leaving {word} counts as a loss in my book."))
    }

    fn set_difficulty(&mut self, difficulty: Difficulty, cx: &mut Context<Self>) {
        // The pills are a `ButtonGroup`, so the selected one is still a button
        // and clicking it still fires. Ask first, because a click that changes
        // nothing must not cost the word in hand — and because the answer is
        // what decides whether there is a word to charge for at all. Once the
        // match is over the same click does restart, which is the footer's
        // "pick a difficulty to start a new match".
        if !self.game.would_switch_to(difficulty) {
            return;
        }
        // Read before the switch deals a new pool and takes the word with it.
        let abandoned = self.word_being_abandoned();

        self.game.set_difficulty(difficulty);
        // A fresh match, so the match score starts again from zero. The streak
        // and the lifetime tally are untouched on purpose — see `crate::stats`.
        self.session.start_match();
        // Walking out on a half-played word is losing it, exactly as giving up
        // is: without this the streak keeps a free escape hatch.
        self.notice = abandoned.map(|(word, from)| self.record_abandoned(&word, from));
        self.last_guess = None;
        self.settings.difficulty = Some(difficulty);
        self.settings.save();
        cx.notify();
    }

    /// Remember where the window is, on its way out. See [`HangmanView::new`]
    /// for why this is the moment the geometry is written.
    fn save_window_frame(&mut self, window: &Window) {
        self.settings.window = Some(window_frame(window));
        self.settings.save();
    }

    /// Flip the whole application between the light and dark palettes.
    ///
    /// `Theme` is a GPUI *global* — one value owned by the app rather than by
    /// any view — so swapping it restyles every gpui-kit component at once.
    /// The swap goes through [`apply_theme`] rather than `Theme::change`,
    /// which is what puts the overlay override back afterwards; handing it the
    /// window is what repaints, and it does that itself, last.
    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = if cx.theme().is_dark() {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        apply_theme(next, Some(window), cx);
        self.settings.theme = next.into();
        self.settings.save();
        cx.notify();
    }

    fn on_change_word(&mut self, _: &ChangeWord, window: &mut Window, cx: &mut Context<Self>) {
        if dialog_is_open(window, cx) {
            return;
        }
        self.give_up(cx);
    }

    /// The original's "Game > Open File...": pick a `.txt` with one word per line.
    fn on_open_word_list(&mut self, _: &OpenWordList, window: &mut Window, cx: &mut Context<Self>) {
        if dialog_is_open(window, cx) {
            return;
        }

        // The native picker answers on a channel, so the rest of this runs in a
        // task. `PathPromptOptions` has no extension filter, hence the prompt text.
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a word list (.txt, one word per line)".into()),
        });

        cx.spawn_in(window, async move |view, cx| {
            // Cancelled, or the platform has no file picker: leave the game alone.
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };

            let contents = std::fs::read_to_string(&path);
            _ = view.update_in(cx, |this, window, cx| {
                this.load_word_list(contents, window, cx);
            });
        })
        .detach();
    }

    fn load_word_list(
        &mut self,
        contents: std::io::Result<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Read before the load, for the same reason as in `set_difficulty` —
        // and only charged for on the success branch below, because a list that
        // turns out to be unreadable or empty leaves the current word alone.
        let abandoned = self.word_being_abandoned();

        let loaded = contents.ok().and_then(|text| {
            let words = text.lines().map(str::to_owned).collect();
            self.game.set_word_list(words).ok()
        });

        match loaded {
            Some(()) => {
                // A loaded list starts a fresh match, exactly as picking a
                // difficulty does, so the match score restarts with it — and,
                // exactly as picking a difficulty does, it loses you the word
                // you walked out on.
                self.session.start_match();
                self.notice = abandoned.map(|(word, from)| self.record_abandoned(&word, from));
                self.last_guess = None;
                window.push_notification(
                    format!("Loaded {} words. New match!", self.game.total_words()),
                    cx,
                );
            }
            None => self.notice = Some(Notice::bad(FILE_ERROR)),
        }
        cx.notify();
    }

    /// Typing a letter guesses it — the original had no keyboard input at all.
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if dialog_is_open(window, cx) {
            return;
        }

        let keystroke = &event.keystroke;
        // Let chords through so ctrl-o and ctrl-n stay shortcuts, not guesses.
        if keystroke.modifiers.control || keystroke.modifiers.alt || keystroke.modifiers.platform {
            return;
        }
        // Enter and space also activate whichever button has been tabbed to, so
        // only treat them as "New Game?" while the root itself holds focus.
        let root_focused = self.focus_handle.is_focused(window);

        match keystroke.key.as_str() {
            "enter" | "space" if root_focused && self.game.is_game_over() => self.new_game(cx),
            key => {
                let letter = key
                    .chars()
                    .next()
                    .filter(|c| key.len() == 1 && c.is_ascii_alphabetic());
                if let Some(letter) = letter {
                    self.guess(letter.to_ascii_uppercase(), cx);
                }
            }
        }
    }

    // -------------------------------------------------------------- derived

    /// The end-of-match message, derived rather than stored so it cannot drift
    /// out of sync with the score.
    fn match_summary(&self) -> Option<Notice> {
        let outcome = self.game.match_outcome()?;
        let wins = self.game.words_won();
        let total = wins + self.game.words_lost();
        let scored = points(self.session.match_points());

        Some(match outcome {
            MatchOutcome::Win => Notice::good(format!(
                "Good job, you got {wins} out of {total} for {scored}"
            )),
            MatchOutcome::Loss => Notice::bad(format!(
                "Nice try, you only got {wins} out of {total} for {scored}"
            )),
            MatchOutcome::Tie => Notice::good(format!(
                "A tie, not bad, you got {wins} out of {total} for {scored}"
            )),
        })
    }

    /// What the Hint button says it will do, or why it will not.
    ///
    /// The disabled tooltip is the whole reason the button stays on screen
    /// greyed out instead of disappearing: "it would cost your last guess" is
    /// a rule worth learning, and a button that vanishes teaches nothing.
    fn hint_tooltip(&self) -> &'static str {
        if self.game.can_hint() {
            HINT_TOOLTIP
        } else if self.game.is_game_over() {
            HINT_TOOLTIP_OVER
        } else {
            HINT_TOOLTIP_LAST_GUESS
        }
    }

    /// What the title bar shows beside the wordmark.
    fn subtitle(&self) -> SharedString {
        match self.game.difficulty() {
            Some(difficulty) => difficulty.label().into(),
            None => "Custom word list".into(),
        }
    }

    /// How many guesses have landed this game.
    ///
    /// Element ids that carry this number mount fresh on every accepted guess,
    /// which is what lets a one-shot animation play again instead of once at
    /// mount — `with_animation` stamps its start time only when its element
    /// state is missing (`gpui-pre-0.3.3/src/elements/animation.rs:400-405`).
    fn guess_count(&self) -> usize {
        self.game.guessed_letters().len()
    }

    /// How the key for `letter` should be drawn right now.
    fn key_state(&self, letter: char) -> KeyState {
        if !self.game.guessed_letters().contains(&letter) {
            return if self.game.is_game_over() {
                KeyState::OutOfPlay
            } else {
                KeyState::Available
            };
        }
        if self.game.word().contains(letter) {
            KeyState::Correct
        } else {
            KeyState::Wrong
        }
    }

    // ------------------------------------------------------------- rendering

    fn render_title_bar(&self, cx: &Context<Self>) -> impl IntoElement {
        let dark = cx.theme().is_dark();

        TitleBar::new()
            // The other route out: on Linux gpui-kit draws its own window
            // controls and its X calls `window.remove_window()` directly, which
            // never reaches the handler in `HangmanView::new`. This hook is
            // where that button ends up instead — and it is ignored outright on
            // Windows and macOS, where the control buttons are the system's.
            .on_close_window(cx.listener(|this, _, window, _| {
                this.save_window_frame(window);
                window.remove_window();
            }))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_sm().font_bold().child("Hangman!"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(self.subtitle()),
                    ),
            )
            .child(
                // `occlude` keeps this press away from the title bar behind it.
                // gpui-kit's `TitleBar` starts an interactive window move from
                // its own bubble-phase `on_mouse_down`/`on_mouse_move`, and
                // `Button` only stops propagation while it is loading. Without
                // this, a click that drifts even a pixel is taken by the window
                // manager as a window drag, the button never sees mouse-up, and
                // the toggle silently does nothing.
                h_flex().pr_2().occlude().child(
                    Button::new("theme-toggle")
                        .ghost()
                        .small()
                        .icon(if dark { IconName::Sun } else { IconName::Moon })
                        .accessibility_label("Toggle light and dark mode")
                        .tooltip(if dark {
                            "Switch to light mode"
                        } else {
                            "Switch to dark mode"
                        })
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_theme(window, cx))),
                ),
            )
    }

    fn render_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let current = self.game.difficulty();
        // What switching away would cost right now, or `None` when it is free.
        // The warning goes on the pills themselves rather than into a dialog:
        // gpui-kit ships a `Modal` and a `Dialog`, but this window has never
        // used either (see `render_stats_panel`), and a tooltip is the idiom
        // every other button in this toolbar already uses. You hover before you
        // click, and the notice after the switch says it a second time.
        // It says "this word" and never the word itself, unlike the notice
        // afterwards: a tooltip is readable *during* play, and `game.word()` is
        // the answer.
        let at_stake = self.game.has_word_to_lose();

        h_flex()
            .w_full()
            .flex_wrap()
            .gap_3()
            .px_5()
            .py_2p5()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex().gap_2p5().child(eyebrow("DIFFICULTY", cx)).child(
                    // A segmented control: one bar of joined buttons with
                    // exactly one of them selected.
                    ButtonGroup::new("difficulty")
                        .small()
                        .outline()
                        .children(Difficulty::ALL.map(|difficulty| {
                            let selected = current == Some(difficulty);
                            let pill = Button::new(SharedString::from(format!(
                                "difficulty-{difficulty:?}"
                            )))
                            .label(difficulty.label())
                            .selected(selected);
                            // Only the pills that would actually switch, and
                            // only while there is a word to lose.
                            if at_stake && !selected {
                                pill.tooltip("Switching now counts this word as a loss")
                            } else {
                                pill
                            }
                        }))
                        .on_click(cx.listener(|this, clicked: &Vec<usize>, _, cx| {
                            if let Some(difficulty) =
                                clicked.first().and_then(|ix| Difficulty::ALL.get(*ix))
                            {
                                this.set_difficulty(*difficulty, cx);
                            }
                        })),
                ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        // Outside the title bar, so — unlike the theme toggle —
                        // this needs no `.occlude()`: nothing behind it is
                        // waiting to turn the click into a window drag. The
                        // same goes for the three buttons after it.
                        Button::new("hint")
                            .small()
                            .ghost()
                            .icon(IconName::Eye)
                            .label("Hint")
                            .disabled(!self.game.can_hint())
                            .tooltip_with_action(self.hint_tooltip(), &Hint, Some(KEY_CONTEXT))
                            .on_click(cx.listener(|this, _, _, cx| this.hint(cx))),
                    )
                    .child(
                        Button::new("toggle-stats")
                            .small()
                            .ghost()
                            .icon(IconName::ChartPie)
                            .label("Stats")
                            .selected(self.show_stats)
                            .tooltip("Show the lifetime score, streak and tally")
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_stats(cx))),
                    )
                    .child(
                        Button::new("change-word")
                            .small()
                            .ghost()
                            .icon(IconName::RotateCw)
                            .label("Change Word")
                            .tooltip_with_action(
                                "Give up on this word — counts as a loss",
                                &ChangeWord,
                                Some(KEY_CONTEXT),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.give_up(cx))),
                    )
                    .child(
                        Button::new("open-word-list")
                            .small()
                            .ghost()
                            .icon(IconName::FileText)
                            .label("Open word list…")
                            .tooltip_with_action(
                                "Play a .txt of your own",
                                &OpenWordList,
                                Some(KEY_CONTEXT),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_open_word_list(&OpenWordList, window, cx)
                            })),
                    ),
            )
    }

    /// One number-over-caption tile of the scoreboard.
    fn render_stat(
        value: impl Into<SharedString>,
        label: impl Into<SharedString>,
        color: Hsla,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .flex_1()
            .gap_0p5()
            .child(
                div()
                    .text_xl()
                    .font_bold()
                    .text_color(color)
                    .child(value.into()),
            )
            .child(eyebrow(label.into(), cx))
    }

    fn render_scoreboard(&self, cx: &Context<Self>) -> impl IntoElement {
        let divider = || div().w(px(1.)).h(px(30.)).bg(cx.theme().border);
        let stats = self.session.stats();

        panel(cx)
            .flex_row()
            .items_center()
            .gap_4()
            .px_5()
            .py_3()
            .child(Self::render_stat(
                self.session.match_points().to_string(),
                "SCORE",
                cx.theme().foreground,
                cx,
            ))
            .child(divider())
            .child(Self::render_stat(
                stats.streak.to_string(),
                "STREAK",
                cx.theme().green,
                cx,
            ))
            .child(divider())
            .child(Self::render_stat(
                stats.best_streak.to_string(),
                "BEST",
                cx.theme().blue,
                cx,
            ))
            .child(divider())
            .child(Self::render_stat(
                format!("{} / {}", self.game.word_number(), self.game.total_words()),
                "WORD",
                cx.theme().muted_foreground,
                cx,
            ))
    }

    /// The lifetime stats panel, folded out under the board by the toolbar's
    /// Stats button.
    ///
    /// Inline and collapsible rather than a dialog on purpose: this window
    /// already has every piece it needs — `panel`, `eyebrow`, `render_stat` —
    /// and a panel of numbers you may want to read while you play is the wrong
    /// thing to put behind a backdrop that blocks the board. The `Reset stats`
    /// button inside it is the opposite case, and does open one; see
    /// [`HangmanView::confirm_reset_stats`].
    fn render_stats_panel(&self, cx: &Context<Self>) -> impl IntoElement {
        let stats = self.session.stats();

        panel(cx)
            .gap_3()
            .px_5()
            .py_3()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(eyebrow("LIFETIME STATS", cx))
                    .child(
                        // `.ghost()` and `.danger()` are the same slot — the
                        // last one wins — so this is an *outlined* danger
                        // button: unmistakably destructive without shouting
                        // from the middle of a panel of numbers.
                        Button::new("reset-stats")
                            .xsmall()
                            .danger()
                            .outline()
                            .icon(IconName::Delete)
                            .label("Reset stats")
                            .tooltip("Throw away every point, streak and tally")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_reset_stats(window, cx)
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_4()
                    .child(Self::render_stat(
                        stats.points.to_string(),
                        "POINTS",
                        cx.theme().foreground,
                        cx,
                    ))
                    .child(Self::render_stat(
                        stats.words_won.to_string(),
                        "WORDS WON",
                        cx.theme().green,
                        cx,
                    ))
                    .child(Self::render_stat(
                        stats.words_lost.to_string(),
                        "WORDS LOST",
                        cx.theme().red,
                        cx,
                    ))
                    .child(Self::render_stat(
                        percent(stats.win_rate()),
                        "WIN RATE",
                        cx.theme().foreground,
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .gap_4()
                    .child(Self::render_stat(
                        stats.streak.to_string(),
                        "STREAK",
                        cx.theme().green,
                        cx,
                    ))
                    .child(Self::render_stat(
                        stats.best_streak.to_string(),
                        "BEST STREAK",
                        cx.theme().blue,
                        cx,
                    ))
                    .child(Self::render_stat(
                        format!(
                            "{} / {} / {}",
                            stats.matches_won, stats.matches_lost, stats.matches_tied
                        ),
                        "MATCHES W / L / T",
                        cx.theme().foreground,
                        cx,
                    )),
            )
            .child(div().w_full().h(px(1.)).bg(cx.theme().border))
            .child(
                v_flex()
                    .gap_1p5()
                    .child(Self::render_breakdown_row(
                        eyebrow("DIFFICULTY", cx),
                        ["POINTS", "WON", "LOST", "MATCHES"]
                            .map(|heading| eyebrow(heading, cx).into_any_element()),
                    ))
                    .children(Difficulty::ALL.map(|difficulty| {
                        self.render_difficulty_row(difficulty, cx)
                            .into_any_element()
                    })),
            )
    }

    /// One line of the per-difficulty breakdown: a label, then four numbers on
    /// a shared grid so the columns line up down the table.
    fn render_breakdown_row(label: impl IntoElement, cells: [AnyElement; 4]) -> impl IntoElement {
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            // Wide enough for the "DIFFICULTY" heading itself, so the header
            // and every row below it start their numbers at the same x.
            .child(div().w(BREAKDOWN_LABEL_WIDTH).flex_none().child(label))
            .children(cells.map(|cell| div().flex_1().child(cell)))
    }

    /// The breakdown line for one difficulty. A difficulty never played reads
    /// as zeroes rather than being hidden: an empty row is information too.
    fn render_difficulty_row(
        &self,
        difficulty: Difficulty,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let bucket = self.session.stats().for_difficulty(difficulty);
        let DifficultyStats {
            points,
            words_won,
            words_lost,
            ..
        } = bucket;

        let number = |text: String, color: Hsla| {
            div()
                .text_sm()
                .font_semibold()
                .text_color(color)
                .child(text)
                .into_any_element()
        };

        Self::render_breakdown_row(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(difficulty.label()),
            [
                number(points.to_string(), cx.theme().foreground),
                number(words_won.to_string(), cx.theme().green),
                number(words_lost.to_string(), cx.theme().red),
                number(
                    bucket.matches_played().to_string(),
                    cx.theme().muted_foreground,
                ),
            ],
        )
    }

    /// One character of the word: the glyph, and the rule it sits on.
    ///
    /// `index` is the cell's place in the word, which is also its place in the
    /// stagger when the word is revealed on a win. `fresh` is set instead when
    /// the *last guess* turned this cell over, and carries the cell's place
    /// among that letter's own occurrences — see [`HangmanView::render_word_panel`].
    fn render_word_cell(
        &self,
        index: usize,
        cell: Cell,
        fresh: Option<usize>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        // On a loss every cell is revealed, so mark the ones the player never
        // actually guessed — that is the answer, not their work.
        let missed = self.game.game_result() == Some(GameResult::Lost)
            && cell.guessable
            && !self.game.guessed_letters().contains(&cell.value);

        let glyph_color = if missed {
            cx.theme().danger
        } else {
            cx.theme().foreground
        };
        let rule_color = if missed {
            cx.theme().danger.alpha(0.7)
        } else if cell.revealed {
            cx.theme().foreground.alpha(0.55)
        } else {
            cx.theme().muted_foreground.alpha(0.4)
        };

        let glyph = h_flex()
            .h(WORD_CELL_HEIGHT)
            .justify_center()
            .text_size(px(30.))
            .font_bold()
            .font_family(cx.theme().mono_font_family.clone())
            .text_color(glyph_color)
            .when(cell.revealed, |this| this.child(cell.value.to_string()));

        v_flex()
            .items_center()
            .gap_1p5()
            // A space or a slash is not a blank to fill in, so it takes half
            // the room and gets no rule under it.
            .w(if cell.guessable {
                WORD_CELL_WIDTH
            } else {
                WORD_CELL_WIDTH / 2.
            })
            .child(if self.game.is_won() {
                // Won: the letters resolve one after another instead of the
                // whole word landing at once. Two things arm this. The cells
                // only carry an animation while the game is won, so losing the
                // element between games is what lets it play again; and the id
                // carries the word's number in the match, so the next word's
                // win cannot inherit the finished state of this one.
                //
                // This branch comes first deliberately. The guess that wins is
                // also a guess that reveals letters, so both reveals are live
                // on the same frame; taking the win first means the row plays
                // one flourish rather than a cell running two fades at once.
                glyph
                    .with_animation(
                        ElementId::named_usize(
                            format!("word-reveal-{index}"),
                            self.game.word_number(),
                        ),
                        // Linear on purpose: the easing belongs to the letter's
                        // own fade, not to its place in the queue.
                        Animation::new(WIN_REVEAL.span(index)),
                        move |this, delta| WIN_REVEAL.draw(this, index, delta),
                    )
                    .into_any_element()
            } else if let Some(ordinal) = fresh {
                // Mid-game: the cells this guess just filled fade up into
                // place, the rest are already there and stay put. Same two
                // arming tricks as the win — the element exists only while
                // this letter is the last one guessed, and the id carries the
                // guess count so a later guess cannot inherit its state.
                glyph
                    .with_animation(
                        ElementId::named_usize(format!("word-guess-{index}"), self.guess_count()),
                        Animation::new(GUESS_REVEAL.span(ordinal)),
                        move |this, delta| GUESS_REVEAL.draw(this, ordinal, delta),
                    )
                    .into_any_element()
            } else {
                glyph.into_any_element()
            })
            .when(cell.guessable, |this| {
                this.child(
                    div()
                        .w_full()
                        .h(WORD_RULE_HEIGHT)
                        .rounded_full()
                        .bg(rule_color),
                )
            })
    }

    fn render_word_panel(&self, cx: &Context<Self>) -> impl IntoElement {
        let wrong = self.game.wrong_guesses();
        let cells = self.game.cells();
        // A letter's occurrences stagger against each other, not against the
        // word: guessing the only E in a long word should not wait out the
        // stagger of the eight cells in front of it, and four Es in a row
        // should still land one after another. So this counts the matches as
        // the row is built, and each cell is handed its place among them.
        let mut occurrences = 0usize;

        let row = h_flex()
            .w_full()
            .gap(WORD_GAP)
            .flex_wrap()
            .justify_center()
            // `cells()` already reveals the whole word once the game is over,
            // and marks ' ' and '/' as non-guessable so they show for free.
            .children(cells.into_iter().enumerate().map(|(index, cell)| {
                // The cells this guess turned over are exactly the revealed
                // ones holding the letter it guessed: every other revealed
                // cell came from an earlier guess and must not move again.
                // (' ' and '/' are free, but never equal an A-Z guess.)
                let fresh = (self.last_guess == Some(cell.value) && cell.revealed).then(|| {
                    let ordinal = occurrences;
                    occurrences += 1;
                    ordinal
                });
                self.render_word_cell(index, cell, fresh, cx)
            }));

        panel(cx)
            .gap_2()
            .px_5()
            .py_3()
            .child(eyebrow("THE WORD", cx))
            .child(if wrong == 0 {
                row.into_any_element()
            } else {
                // A wrong guess shakes the word. The wrong-guess count is *in
                // the element id*, which is what makes this fire once per
                // guess: `with_animation` keys its state on the id and starts
                // the clock the frame that id first appears, so a fixed id
                // would shake once and then sit still for the rest of the
                // match. Guessing wrong is a new count, so a new id, so a
                // fresh animation. Below zero wrong guesses there is no
                // animation at all, which also keeps the row from twitching at
                // startup and re-arms it after a new game.
                row.with_animation(
                    ElementId::named_usize("word-shake", wrong),
                    Animation::new(SHAKE),
                    // `left` on a relative element offsets the paint only: the
                    // row keeps its size and its neighbours keep their places.
                    |this, delta| this.left(px(shake_offset(delta))),
                )
                .into_any_element()
            })
    }

    /// One letter key, dressed for whichever state it is in.
    ///
    /// `Selectable::selected` is doing the work for a guessed key: it paints
    /// the variant's *pressed* surface and suppresses the hover and active
    /// styling, which is exactly "this key has been played" — where
    /// `disabled` would instead wash the colour out to a faint tint.
    ///
    /// The key a guess just played fades into that colour rather than snapping
    /// to it. The colours are `Button`'s own — there is no outer element to
    /// tint and no transform to scale with, since gpui's `Style` carries
    /// neither (`gpui-pre-0.3.3/src/style.rs`; `Transformation` exists, but
    /// only on `svg`). What makes a real tween possible instead is
    /// `ButtonVariant::Custom`, whose colours the animator can rebuild every
    /// frame.
    fn render_key(&self, letter: char, cx: &Context<Self>) -> impl IntoElement {
        let state = self.key_state(letter);
        let id = SharedString::from(format!("letter-{letter}"));

        let button = Button::new(id)
            .label(letter.to_string())
            .size(KEY_SIZE)
            .font_semibold()
            .map(|button| match state {
                KeyState::Available => button
                    .on_click(cx.listener(move |this, _, _, cx| this.guess(letter, cx)))
                    .tooltip(format!("Guess {letter}")),
                KeyState::Correct => button.success().selected(true).toggled(true),
                KeyState::Wrong => button.danger().selected(true).toggled(true),
                KeyState::OutOfPlay => button.disabled(true),
            });

        // Only the key the guess just played, and only into the two colours a
        // guess can produce. Every other key is already at rest.
        let settling =
            self.last_guess == Some(letter) && matches!(state, KeyState::Correct | KeyState::Wrong);
        if !settling {
            return button.into_any_element();
        }

        // Where the key is coming from: the three colours the unplayed
        // `Default` variant paints (`gpui-component-0.6.0/src/button/button.rs`
        // `:890` background, `:904` text, `:956` border).
        let from_bg = cx.theme().button;
        let from_border = cx.theme().input;
        let from_text = cx.theme().button_foreground;
        // …and where it is going: the same three for a *selected* `Success` or
        // `Danger`, which is what `.success().selected(true)` above resolves to
        // (`button.rs:1188`).
        let (to_bg, to_border, to_text) = if state == KeyState::Correct {
            (
                cx.theme().button_success_active,
                cx.theme().button_success,
                cx.theme().button_success_foreground,
            )
        } else {
            (
                cx.theme().button_danger_active,
                cx.theme().button_danger,
                cx.theme().button_danger_foreground,
            )
        };
        let custom = ButtonCustomVariant::new(cx);

        button
            .with_animation(
                // The guess count is *in the element id*, exactly as the pip
                // pulse carries the wrong-guess count: with a fixed id this
                // would play once at mount and every later key would inherit
                // the finished state.
                ElementId::named_usize(format!("key-settle-{letter}"), self.guess_count()),
                Animation::new(KEY_SETTLE).with_easing(ease_in_out),
                move |button, delta| {
                    if delta >= 1. {
                        // The resting frame is the button exactly as it was
                        // built. That matters: a theme may paint these tokens
                        // as gradients, which a flat colour cannot reproduce,
                        // so the tween is an approximation the key must not be
                        // left sitting on.
                        return button;
                    }
                    // A selected `Custom` reads `active` as its background,
                    // `color` as its border and `foreground` as its text, so
                    // handing it three blended colours per frame tweens the
                    // real button rather than faking one with an overlay.
                    button.custom(
                        custom
                            .active(blend(from_bg, to_bg, delta))
                            .hover(blend(from_bg, to_bg, delta))
                            .color(blend(from_border, to_border, delta))
                            .foreground(blend(from_text, to_text, delta)),
                    )
                },
            )
            .into_any_element()
    }

    fn render_keyboard(&self, cx: &Context<Self>) -> impl IntoElement {
        let letters: Vec<char> = ('A'..='Z').collect();

        panel(cx)
            .gap_3()
            .px_5()
            .py_3()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(eyebrow("LETTERS", cx))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} guess{} left",
                                self.game.remaining_guesses(),
                                if self.game.remaining_guesses() == 1 {
                                    ""
                                } else {
                                    "es"
                                }
                            )),
                    ),
            )
            .child(
                // The outer column centres the pad; the inner one sizes to the
                // widest row, so the rows inside stay left-aligned and the
                // original's indent still reads as an indent.
                v_flex().items_center().child(
                    v_flex().gap(KEY_GAP).children(
                        letters
                            .chunks(LETTERS_PER_ROW)
                            .enumerate()
                            .map(|(row, chunk)| {
                                h_flex()
                                    .gap(KEY_GAP)
                                    // The original's quirk: the V-Z row starts
                                    // one cell in.
                                    .when(row == INDENTED_ROW, |this| this.child(div().w(KEY_SIZE)))
                                    .children(
                                        chunk.iter().map(|&letter| self.render_key(letter, cx)),
                                    )
                            }),
                    ),
                ),
            )
    }

    /// The status line while a game is in progress.
    fn render_status_line(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex().h(px(28.)).px_1().child(match self.notice.clone() {
            Some(notice) => div()
                .text_sm()
                .italic()
                .text_color(if notice.good {
                    cx.theme().green
                } else {
                    cx.theme().red
                })
                .child(notice.text)
                .into_any_element(),
            None => div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(IDLE_HINT)
                .into_any_element(),
        })
    }

    /// The end-of-game panel: what happened, what the word was, what next.
    fn render_result(&self, cx: &Context<Self>) -> impl IntoElement {
        let won = self.game.is_won();
        // `notice` already holds the original's wording, including the
        // separate line for giving up.
        let headline: SharedString = match self.notice.clone() {
            Some(notice) => notice.text,
            None if won => GAME_WON.into(),
            None => GAME_LOST.into(),
        };
        let answer = format!("The word was {}", self.game.word());
        let summary = self.match_summary();

        let banner = if won {
            Alert::success("result", answer)
        } else {
            Alert::error("result", answer)
        }
        .title(headline)
        .large();

        panel(cx).gap_4().p_4().child(banner).child(match summary {
            // The match is over: `new_game()` would refuse, and the
            // original showed a dead button here. Show the score instead.
            Some(summary) => h_flex()
                .justify_between()
                .items_center()
                .gap_3()
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(
                            div()
                                .font_semibold()
                                .text_color(if summary.good {
                                    cx.theme().green
                                } else {
                                    cx.theme().red
                                })
                                .child(summary.text),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Pick a difficulty to start a new match."),
                        ),
                )
                .into_any_element(),
            None => h_flex()
                .justify_between()
                .items_center()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Press Enter for the next word."),
                )
                .child(
                    Button::new("new-game")
                        .primary()
                        .large()
                        .label("New Game")
                        .tooltip("Next word (Enter)")
                        .on_click(cx.listener(|this, _, _, cx| this.new_game(cx))),
                )
                .into_any_element(),
        })
    }

    /// The gallows stage: the drawing, plus the wrong-guess meter under it.
    fn render_stage(&self, cx: &Context<Self>) -> impl IntoElement {
        let wrong = self.game.wrong_guesses();
        // Six on Insane and on a word list of your own, up to ten on Easy —
        // the drawing and the pips both follow whatever this game allows.
        let budget = self.game.guess_budget();

        panel(cx)
            .w(STAGE_WIDTH)
            .flex_none()
            .items_center()
            .gap_3()
            .p_4()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .child(eyebrow("THE GALLOWS", cx))
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(if wrong == 0 {
                                cx.theme().muted_foreground
                            } else {
                                cx.theme().red
                            })
                            .child(format!("{wrong} / {budget}")),
                    ),
            )
            // The drawing sits on the panel itself: every line takes its
            // colour from the theme, so it reads on either one. The column
            // stretches to the body's height and the drawing takes the slack,
            // with the pips settling underneath it.
            //
            // `w_full` is load-bearing. The panel centres its children, so
            // without a width of its own this column is sized to its content —
            // and its only content with a width is the pip row, because a
            // percentage width (the drawing's `w_full`) measures as nothing
            // when the container's width is what is being measured. The
            // drawing was handed the 84px row's box for a 300px-wide panel and
            // came out at 28% of the size it should have been. The row is
            // wider now that the budget can reach ten (144px at most, still
            // half the panel), which would have hidden the bug, not fixed it.
            .child(
                v_flex()
                    .w_full()
                    .flex_1()
                    .items_center()
                    .gap_4()
                    .child(gallows(wrong, budget))
                    .child(
                        // One pip per wrong guess the budget allows: the score
                        // the drawing is keeping, in a form you can count at a
                        // glance. Six of them at `PIP_SIZE` with five
                        // `gap_1p5` gaps is 84px; ten with nine gaps is 144px,
                        // inside the 300px the panel has between its padding
                        // either way.
                        h_flex()
                            .flex_none()
                            .gap_1p5()
                            .children((0..budget).map(|step| Self::render_pip(step, wrong, cx))),
                    ),
            )
    }

    /// One wrong-guess pip, and — for the one this guess just filled — the
    /// pulse that marks it turning red.
    ///
    /// The pulse is a halo drawn *behind* the pip: it grows past it and fades
    /// out, so it finishes invisible, which is exactly how a pip that has been
    /// red for a while should look. It is positioned absolutely inside a
    /// pip-sized box, so growing it moves nothing else along the row.
    fn render_pip(step: usize, wrong: usize, cx: &Context<Self>) -> impl IntoElement {
        let filled = step < wrong;
        // The pip the last wrong guess filled, and only while it is the last.
        let fresh = filled && step + 1 == wrong;
        let danger = cx.theme().danger;

        div()
            .relative()
            .size(PIP_SIZE)
            .when(fresh, |this| {
                this.child(div().absolute().rounded_full().bg(danger).with_animation(
                    // The wrong-guess count is *in the element id*, so
                    // each new wrong guess mounts this fresh and plays
                    // it again; with a fixed id `with_animation` would
                    // run it once and treat every later pip as the same
                    // element, already finished.
                    ElementId::named_usize("pip-pulse", wrong),
                    Animation::new(PIP_PULSE).with_easing(ease_out_quint()),
                    |this, delta| {
                        let grow = PIP_HALO * delta;
                        this.size(PIP_SIZE + grow * 2.)
                            .left(-grow)
                            .top(-grow)
                            .opacity(PIP_HALO_ALPHA * (1. - delta))
                    },
                ))
            })
            .child(div().size_full().rounded_full().bg(if filled {
                danger
            } else {
                cx.theme().muted_foreground.alpha(0.25)
            }))
    }

    fn render_play_column(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .id("play-column")
            .flex_1()
            .h_full()
            .gap_3()
            // A real scrollbar rather than a bare `overflow_y_scroll`: this
            // column is the only thing in the window that can scroll, and
            // until it drew one there was nothing to say so — the content
            // simply stopped at the bottom edge. gpui-kit hides the bar
            // whenever the content fits, so it costs nothing in the states
            // that need no scrolling.
            //
            // The padding is what keeps the bar out of the panels rather than
            // over them. `Scrollable` copies only the size and flex styles onto
            // its wrapper (`gpui-component-0.6.0/src/scroll/scrollable.rs:238`),
            // so this stays on the scrolled content, and the overlay — which is
            // pinned to the wrapper's edges — lands in the strip it leaves.
            .pr(SCROLLBAR_GUTTER)
            .overflow_y_scrollbar()
            .child(self.render_scoreboard(cx))
            .child(self.render_word_panel(cx))
            .child(self.render_keyboard(cx))
            .child(if self.game.is_game_over() {
                self.render_result(cx).into_any_element()
            } else {
                self.render_status_line(cx).into_any_element()
            })
            .when(self.show_stats, |this| {
                this.child(self.render_stats_panel(cx))
            })
    }

    /// The keyboard legend along the bottom of the window.
    ///
    /// It sits outside the play column on purpose. That column scrolls, and a
    /// legend that scrolls away is a legend you have to go looking for; this
    /// is a strip under both columns, mirroring the toolbar at the top, so it
    /// is on screen whatever the window is doing. It also keeps it clear of
    /// the stage panel's flex arithmetic, which is more delicate than it looks
    /// (see `render_stage`).
    ///
    /// Every chip is read out of the keymap through
    /// [`Kbd::binding_for_action`], so this row cannot drift from the bindings
    /// `main.rs` registers, and it spells each chord the way the platform
    /// does.
    fn render_shortcut_bar(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;

        h_flex()
            .w_full()
            .flex_none()
            .flex_wrap()
            .gap_4()
            .px_5()
            .py_2()
            .border_t_1()
            .border_color(cx.theme().border)
            .children(shortcut_legend(&self.game).into_iter().filter_map(|hint| {
                // No binding in the keymap, no entry: an unlabelled promise is
                // worse than nothing. Unreachable while `main.rs` binds all
                // three, which is the point of asking rather than assuming.
                let kbd = shortcut_kbd(hint.shortcut, window)?;
                Some(
                    h_flex()
                        .items_center()
                        .gap_1p5()
                        // A key the game would currently refuse is dimmed
                        // rather than dropped, so the strip stays the same
                        // shape and the rule stays readable — the same call
                        // the disabled `Hint` button makes.
                        .opacity(if hint.live { 1. } else { 0.4 })
                        .child(kbd.outline())
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(hint.shortcut.label()),
                        ),
                )
            }))
    }
}

impl Focusable for HangmanView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for HangmanView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("hangman")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            // Key events bubble up the focus path, so this still fires when one
            // of the letter buttons has been tabbed to.
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(Self::on_open_word_list))
            .on_action(cx.listener(Self::on_change_word))
            .on_action(cx.listener(Self::on_hint))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_title_bar(cx))
            .child(self.render_toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .gap(COLUMN_GAP - SCROLLBAR_GUTTER)
                    .p_5()
                    .child(self.render_play_column(cx))
                    .child(self.render_stage(cx)),
            )
            .child(self.render_shortcut_bar(window, cx))
            // `Root` owns these overlays but does not render them for you.
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

#[cfg(test)]
mod tests {
    // Named one by one rather than with a `use super::*`, which would drag in
    // this module's `gpui_kit::*` glob — and with it gpui's own `test` macro,
    // which shadows the built-in attribute and blows the recursion limit.
    use super::{
        DIALOG_TOP_FRACTION, GPUI_KIT_DIALOG_TOP_FRACTION, RESET_NOTHING, Shortcut, Stats,
        dialog_top_margin, plural, points, reset_stats_summary, shortcut_legend,
    };
    use crate::game::{Difficulty, Game};

    // `Stats` has a private field, so it is filled in rather than built from a
    // literal — which is the right shape for this anyway: only the four
    // numbers the warning quotes matter here.
    fn stats(points: u64, won: u32, lost: u32, best_streak: u32) -> Stats {
        let mut stats = Stats::default();
        stats.points = points;
        stats.words_won = won;
        stats.words_lost = lost;
        stats.best_streak = best_streak;
        stats
    }

    #[test]
    fn plural_says_s_for_everything_but_one() {
        assert_eq!(plural(0, "word"), "0 words");
        assert_eq!(plural(1, "word"), "1 word");
        assert_eq!(plural(2, "word"), "2 words");
    }

    #[test]
    fn points_is_plural_of_point() {
        assert_eq!(points(1), "1 point");
        assert_eq!(points(840), "840 points");
    }

    #[test]
    fn the_reset_warning_names_what_is_lost() {
        let summary = reset_stats_summary(&stats(9210, 31, 9, 11));

        assert!(summary.starts_with("9210 points, 40 words and a best streak of 11"));
        assert!(summary.contains("no undo"));
    }

    #[test]
    fn the_reset_warning_counts_lost_words_too() {
        // Words played is wins *and* losses: resetting throws away the whole
        // record, not just the flattering half of it.
        assert!(reset_stats_summary(&stats(50, 0, 3, 0)).contains("3 words"));
    }

    #[test]
    fn the_reset_warning_does_not_threaten_an_empty_tally() {
        // A first launch with the stats panel open can reach the button, and
        // a dialog warning about losing nothing would be crying wolf.
        assert_eq!(reset_stats_summary(&Stats::default()), RESET_NOTHING);
    }

    #[test]
    fn a_streak_alone_is_still_worth_warning_about() {
        // Points and words can both be zero with a best streak behind them:
        // every word so far was won on a list of one, say. The tally is not
        // empty, so the warning is the real one.
        let summary = reset_stats_summary(&stats(0, 0, 0, 4));

        assert_ne!(summary, RESET_NOTHING);
        assert!(summary.contains("best streak of 4"));
    }

    /// Everything in here goes through [`shortcut_legend`], which takes a
    /// `&Game` and returns plain data — no GPUI type is constructed and no
    /// window is opened, which is what keeps these runnable with the rest.
    fn legend(game: &Game) -> Vec<(Shortcut, bool)> {
        shortcut_legend(game)
            .into_iter()
            .map(|hint| (hint.shortcut, hint.live))
            .collect()
    }

    /// Spend one guess on a letter the word does not contain.
    fn guess_wrong(game: &mut Game) {
        let wrong = ('A'..='Z')
            .find(|letter| {
                !game.word().contains(*letter) && !game.guessed_letters().contains(letter)
            })
            .expect("a word using all 26 letters would be a surprise");
        game.guess(wrong);
    }

    /// Guess letters the word does not contain until the word is lost.
    fn lose_the_word(game: &mut Game) {
        while !game.is_game_over() {
            guess_wrong(game);
        }
    }

    /// Play the whole list out, losing every word, until the match is scored.
    fn lose_the_match(game: &mut Game) {
        while !game.is_match_over() {
            lose_the_word(game);
            game.new_game();
        }
    }

    #[test]
    fn a_word_in_play_offers_the_three_standing_shortcuts() {
        let game = Game::with_seed(Difficulty::Easy, 7);

        assert_eq!(
            legend(&game),
            vec![
                (Shortcut::Hint, true),
                (Shortcut::ChangeWord, true),
                (Shortcut::OpenWordList, true),
            ]
        );
    }

    #[test]
    fn the_hint_entry_greys_out_on_the_last_guess_rather_than_vanishing() {
        let mut game = Game::with_seed(Difficulty::Easy, 7);
        while game.remaining_guesses() > 1 {
            guess_wrong(&mut game);
        }

        assert!(
            !game.can_hint(),
            "the last guess is not spendable on a hint"
        );
        assert_eq!(
            legend(&game),
            vec![
                (Shortcut::Hint, false),
                (Shortcut::ChangeWord, true),
                (Shortcut::OpenWordList, true),
            ]
        );
    }

    #[test]
    fn a_finished_word_offers_enter_and_dims_what_it_refuses() {
        let mut game = Game::with_seed(Difficulty::Easy, 7);
        lose_the_word(&mut game);

        assert!(!game.is_match_over(), "one lost word is not a whole match");
        assert_eq!(
            legend(&game),
            vec![
                (Shortcut::Hint, false),
                (Shortcut::ChangeWord, false),
                (Shortcut::OpenWordList, true),
                (Shortcut::NextWord, true),
            ]
        );
    }

    #[test]
    fn a_finished_match_stops_offering_enter() {
        let mut game = Game::with_seed(Difficulty::Easy, 7);
        lose_the_match(&mut game);

        // `Game::new_game` refuses once the list is exhausted, and the result
        // panel swaps its `New Game` button for the match summary — so a
        // legend still promising Enter would be promising nothing.
        assert!(!game.new_game());
        assert!(
            !legend(&game)
                .iter()
                .any(|(shortcut, _)| *shortcut == Shortcut::NextWord)
        );
    }

    #[test]
    fn loading_your_own_list_is_never_refused() {
        let mut game = Game::with_seed(Difficulty::Easy, 7);
        lose_the_match(&mut game);

        assert!(
            legend(&game).contains(&(Shortcut::OpenWordList, true)),
            "Ctrl+O works in every state the game can be in"
        );
    }

    #[test]
    fn the_dialog_margin_lands_its_top_edge_on_the_chosen_fraction() {
        // The margin is the gap between where gpui-kit puts the box and where
        // we want it, so the two added back together are the whole rule.
        for height in [660., 800., 1024., 1440.] {
            let top = height * GPUI_KIT_DIALOG_TOP_FRACTION + dialog_top_margin(height);
            assert!(
                (top - height * DIALOG_TOP_FRACTION).abs() < 0.01,
                "a {height}px window should open the dialog at {}px, not {top}px",
                height * DIALOG_TOP_FRACTION,
            );
        }
    }

    #[test]
    fn the_dialog_never_gets_a_negative_margin() {
        // Nothing can drive the window height below zero today; the clamp is
        // there so that a future `DIALOG_TOP_FRACTION` under a tenth moves the
        // dialog back to gpui-kit's own spot rather than off the top edge.
        assert_eq!(dialog_top_margin(0.), 0.);
        assert_eq!(dialog_top_margin(-10.), 0.);
    }
}
