//! The handful of choices that outlive a launch.
//!
//! The game used to start identically every time: light/dark reset to dark, the
//! window reopened centred at its default size, the difficulty went back to
//! Easy however you had left it, and every point you had ever scored went with
//! it. This module is the file that remembers all of that, written as JSON to
//! the platform's own configuration directory:
//!
//! | Platform | Path |
//! | --- | --- |
//! | Windows | `%APPDATA%\hangman-gpui\settings.json` |
//! | macOS | `~/Library/Application Support/hangman-gpui/settings.json` |
//! | Linux | `$XDG_CONFIG_HOME/hangman-gpui/settings.json`, or `~/.config/…` |
//!
//! Two rules shape everything below.
//!
//! **Nothing here may stop the game from starting.** A missing file (which is
//! every first launch), an unreadable one, half a file, JSON of the wrong shape,
//! a config directory that cannot be created — all of them fall back to
//! [`Settings::default`], at worst with one line on stderr. There is no error
//! type for a caller to handle because there is nothing useful a caller could
//! do; the game is perfectly playable with no settings at all.
//!
//! **No UI types.** Like [`crate::game`], this module is plain data, so its
//! rules can be unit-tested without a window. The conversions to gpui-kit's
//! `ThemeMode` and to gpui's `Bounds<Pixels>` live in [`crate::ui`] instead.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::game::{Difficulty, GameResult, Snapshot};
use crate::stats::Stats;
use crate::words::Word;

/// The directory the file is written into, under the platform's config
/// directory. Named after the crate, like every other well-behaved app.
const APP_DIR: &str = "hangman-gpui";

/// The file itself. JSON, pretty-printed, so it can be read and edited by hand.
const FILE_NAME: &str = "settings.json";

/// Which palette the window starts in.
///
/// This mirrors gpui-kit's `ThemeMode`, rather than storing it, so that the
/// file format is ours: the game is dark first, and `ThemeMode`'s own default
/// is light.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
}

/// A rectangle in logical pixels, in the desktop's global coordinate space —
/// the space gpui's `Bounds<Pixels>` and `PlatformDisplay::bounds` both use, so
/// a second monitor to the left of the primary one has negative `x`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn right(&self) -> f32 {
        self.x + self.width
    }

    fn bottom(&self) -> f32 {
        self.y + self.height
    }

    /// How much area this rectangle shares with `other`, which is `0.` when
    /// they do not touch at all.
    fn overlap(&self, other: &Rect) -> f32 {
        let width = (self.right().min(other.right()) - self.x.max(other.x)).max(0.);
        let height = (self.bottom().min(other.bottom()) - self.y.max(other.y)).max(0.);
        width * height
    }

    /// Every number is an ordinary one: no NaN, no infinity.
    ///
    /// Worth checking explicitly, because NaN compares `false` against
    /// everything and would slip straight through the clamping below.
    fn is_finite(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
    }

    /// Where a saved window should actually open, given the displays that exist
    /// *now* and the smallest size the layout is usable at.
    ///
    /// A saved rectangle is never trusted as written. The monitor it was on may
    /// be unplugged, the resolution may have changed, and the file may have been
    /// edited by hand into nonsense — none of which may be allowed to open the
    /// window off-screen or smaller than it can be used at. So the rectangle is
    /// pinned to the display it overlapped most, resized to fit that display,
    /// and moved until it lies entirely inside it.
    ///
    /// Returns `None` when the saved rectangle overlaps no current display at
    /// all, or holds nonsense: that is the signal for the caller to fall back to
    /// its centred default rather than to guess.
    pub fn fit_onto(self, displays: &[Rect], min_width: f32, min_height: f32) -> Option<Rect> {
        if !self.is_finite() || self.width <= 0. || self.height <= 0. {
            return None;
        }

        // The display this window was mostly on. If it is gone, so is any
        // meaning the saved position had.
        let display = displays
            .iter()
            .filter(|display| self.overlap(display) > 0.)
            .max_by(|a, b| self.overlap(a).total_cmp(&self.overlap(b)))?;

        // Never smaller than the layout's minimum, never bigger than the screen
        // it has to fit on. The minimum wins on a display too small for it,
        // which is why this is not a single `clamp` — `clamp` panics when its
        // own bounds cross.
        let width = self.width.max(min_width).min(display.width.max(min_width));
        let height = self
            .height
            .max(min_height)
            .min(display.height.max(min_height));

        // And then far enough back onto the display that the whole window —
        // title bar included — is reachable with the mouse.
        let x = self
            .x
            .clamp(display.x, (display.right() - width).max(display.x));
        let y = self
            .y
            .clamp(display.y, (display.bottom() - height).max(display.y));

        Some(Rect::new(x, y, width, height))
    }
}

/// The window as it was last left: where it was, how big, and whether it was
/// filling the screen.
///
/// `rect` is the *restore* rectangle in the maximized case — the size the window
/// springs back to — which is exactly what gpui's `WindowBounds::Maximized`
/// carries, so the two map onto each other without any bookkeeping here.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowFrame {
    pub rect: Rect,
    /// Maximized, or full-screen. The two are not told apart on purpose:
    /// reopening full-screen because a session once ended that way is a rude
    /// surprise, and maximized is the polite version of the same intent.
    #[serde(default)]
    pub maximized: bool,
}

/// How a word ended, as the file spells it.
///
/// A mirror of [`GameResult`] rather than the thing itself, for the reason
/// [`ThemeChoice`] mirrors gpui-kit's `ThemeMode`: the file format is ours, and
/// [`crate::game`] is kept free of the derives that would make it serde's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SavedResult {
    Won,
    Lost,
}

/// The match that was being played when the game last closed.
///
/// The file's shape for a [`Snapshot`], and the answer to the last silent way
/// out of a word you are losing: closing the window used to throw the word
/// away for nothing, so quitting and relaunching was a free reroll that kept
/// your streak. Saving the match instead means quitting is not an escape,
/// because you come back to it.
///
/// It is written from the moment anything about the match changes — every
/// guess, every hint, every word — rather than as the window closes, and that
/// is the point rather than an accident: a save that only happened on a clean
/// close would still hand a free reroll to anyone who killed the process.
///
/// The words are stored whole, as [`Word`]s, rather than as indices into a
/// pack. A pack rewritten between launches, or one that has been deleted from
/// the disk, therefore costs nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedMatch {
    /// The word on the board, with whatever its pack knew about it.
    ///
    /// The one field with no `#[serde(default)]`: a saved match without a word
    /// in it is not a saved match, and failing the parse here is how it is
    /// refused — `in_flight_or_default` turns that into "nothing to resume"
    /// rather than into a lost settings file.
    pub word: Word,
    /// Every letter guessed against it, as one string: `"AEL"`.
    ///
    /// A string rather than an array of characters because this file is meant
    /// to be readable, and `["A", "E", "L"]` is three lines of JSON saying one
    /// word's worth of nothing.
    #[serde(default)]
    pub guessed: String,
    /// How many of those guesses were wrong.
    #[serde(default)]
    pub wrong_guesses: usize,
    /// How the word ended, or absent while it is still being played.
    #[serde(default)]
    pub result: Option<SavedResult>,
    /// The words of the match still to be dealt.
    #[serde(default)]
    pub remaining: Vec<Word>,
    /// The difficulty it is being played on, or absent for a word list of your
    /// own.
    #[serde(default, with = "difficulty_by_name")]
    pub difficulty: Option<Difficulty>,
    /// What the pack called itself, or empty for one that did not say.
    #[serde(default)]
    pub pack: String,
    /// How many playable words that pack held.
    #[serde(default)]
    pub pack_words: usize,
    /// The guess budget the match is being played to. Advisory: a difficulty's
    /// own budget wins over it on the way back in, and a loaded pack's is
    /// clamped — see [`crate::game::Game::resume`].
    #[serde(default)]
    pub guess_budget: usize,
    /// Words won so far this match.
    #[serde(default)]
    pub words_won: usize,
    /// Words lost so far this match.
    #[serde(default)]
    pub words_lost: usize,
    /// What the match has scored so far.
    ///
    /// The one field here that is not the game's: the match score lives in
    /// [`crate::stats::Session`], and unlike the lifetime tally beside it in
    /// this file it would otherwise not survive the launch.
    #[serde(default)]
    pub match_points: u32,
}

impl SavedMatch {
    /// The file's version of a snapshot the game handed over, plus the match
    /// score that goes with it.
    pub fn new(snapshot: Snapshot, match_points: u32) -> Self {
        Self {
            word: snapshot.current,
            // `BTreeSet` iterates in order, so the string is alphabetical and a
            // file written twice from the same state is the same file.
            guessed: snapshot.guessed.into_iter().collect(),
            wrong_guesses: snapshot.wrong_guesses,
            result: snapshot.result.map(|result| match result {
                GameResult::Won => SavedResult::Won,
                GameResult::Lost => SavedResult::Lost,
            }),
            remaining: snapshot.remaining_words,
            difficulty: snapshot.difficulty,
            pack: snapshot.pack_name,
            pack_words: snapshot.pack_words,
            guess_budget: snapshot.guess_budget,
            words_won: snapshot.words_won,
            words_lost: snapshot.words_lost,
            match_points,
        }
    }

    /// The snapshot back out again, reshaped and nothing more.
    ///
    /// Deliberately not the place the values are checked: this file is one
    /// anyone may edit, and what a plausible match in flight *is* is a rule,
    /// so it belongs with the rules. [`crate::game::Game::resume`] is what
    /// refuses a snapshot, and it refuses it the same way whether it came from
    /// here or from a test.
    pub fn snapshot(self) -> Snapshot {
        Snapshot {
            difficulty: self.difficulty,
            pack_name: self.pack,
            pack_words: self.pack_words,
            guess_budget: self.guess_budget,
            current: self.word,
            guessed: self.guessed.chars().collect(),
            wrong_guesses: self.wrong_guesses,
            result: self.result.map(|result| match result {
                SavedResult::Won => GameResult::Won,
                SavedResult::Lost => GameResult::Lost,
            }),
            remaining_words: self.remaining,
            words_won: self.words_won,
            words_lost: self.words_lost,
        }
    }
}

/// Everything the game remembers between launches.
///
/// `#[serde(default)]` is what makes an older or hand-trimmed file work: any
/// field that is missing takes its default instead of failing the whole parse,
/// and any field a later version adds is simply absent here.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The palette the theme toggle was last left on.
    pub theme: ThemeChoice,
    /// The difficulty last chosen from the toolbar. `None` until one is picked.
    #[serde(with = "difficulty_by_name")]
    pub difficulty: Option<Difficulty>,
    /// Where the window was. `None` until it has been opened once.
    pub window: Option<WindowFrame>,
    /// The lifetime score, streak and tally — see [`crate::stats`].
    ///
    /// Added after the first released format, and needing no version bump to
    /// be so: the container-level `#[serde(default)]` above means a file
    /// written before this field existed simply reads as
    /// [`Stats::default`]. `stats_or_default` then keeps a *malformed* value
    /// from costing the rest of the file, which is the same
    /// never-fail-loudly rule the module doc states.
    #[serde(deserialize_with = "stats_or_default")]
    pub stats: Stats,
    /// The match that was still being played when the window closed, if there
    /// was one — see [`SavedMatch`].
    ///
    /// Forgiving in the same two ways `stats` is, and for the same reason: a
    /// key that is missing (every settings file written before this existed)
    /// is simply no match to resume, and one that holds nonsense is thrown
    /// away on its own rather than taking the theme, the window and the
    /// lifetime score down with it. The worst a broken value can do is cost
    /// you the word you were on.
    #[serde(deserialize_with = "in_flight_or_default")]
    pub in_flight: Option<SavedMatch>,
}

impl Settings {
    /// Where the file lives, or `None` on a system with no config directory of
    /// its own (a Linux account with no `$HOME`, say). Nothing is created here.
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join(APP_DIR).join(FILE_NAME))
    }

    /// The saved settings, or the defaults if there is any reason at all not to
    /// have them. Called once, at startup, before the window is opened.
    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };

        match fs::read_to_string(&path) {
            Ok(text) => Self::parse(&text),
            // The first launch, every time. Not worth a word.
            Err(err) if err.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(err) => {
                eprintln!(
                    "hangman: could not read settings from {}: {err}",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Write the file, reporting a failure on stderr and carrying on.
    ///
    /// Called from the moment a setting changes rather than on a timer, so the
    /// last write always wins and there is no state to flush at exit.
    pub fn save(&self) {
        let Some(path) = Self::path() else {
            return;
        };
        if let Err(err) = self.write_to(&path) {
            eprintln!(
                "hangman: could not save settings to {}: {err}",
                path.display()
            );
        }
    }

    /// Parse a settings file, falling back to the defaults for anything that is
    /// not one. Split out from [`Settings::load`] so the fallback can be tested
    /// without a disk.
    fn parse(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_else(|err| {
            eprintln!("hangman: ignoring unreadable settings ({err}), using the defaults");
            Self::default()
        })
    }

    /// The fallible half of [`Settings::save`].
    ///
    /// The write goes to a temporary file that is then renamed over the real
    /// one, because `fs::rename` replaces the destination in one step on all
    /// three platforms: a save interrupted half way leaves the previous
    /// settings intact instead of a truncated file the next launch has to
    /// throw away.
    fn write_to(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, json)?;
        fs::rename(&temp, path)
    }
}

/// Read the stats, or hand back an empty tally if they are unreadable.
///
/// A wrong-typed `"stats"` — a number, a string, an object whose fields are
/// the wrong shape — would otherwise fail the whole parse and throw away the
/// theme, the window and the difficulty along with it. Losing a score nobody
/// can read is bad enough; losing the rest of the file with it is worse.
///
/// The value is read as raw JSON first because that is what makes "try, and
/// fall back" possible at all: a `Deserializer` cannot be attempted twice.
fn stats_or_default<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Stats, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// Read the match in flight, or hand back `None` if it is unreadable.
///
/// [`stats_or_default`] for the other forgiving key, and the same mechanism: a
/// `Deserializer` cannot be attempted twice, so the value is taken as raw JSON
/// first and only then shaped.
fn in_flight_or_default<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<SavedMatch>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

/// Serde for `Option<Difficulty>`, stored as the name from the original's
/// Difficulty menu: `"difficulty": "Insane"`.
///
/// [`Difficulty`] lives in [`crate::game`], which is deliberately free of
/// everything but the rules — no derives from this module get to leak into it —
/// so the glue is here. Storing the name rather than an index also means a
/// value this version does not know simply reads back as `None`, which is the
/// same as never having picked one.
mod difficulty_by_name {
    use serde::{Deserialize as _, Deserializer, Serializer};

    use crate::game::Difficulty;

    pub fn serialize<S: Serializer>(
        difficulty: &Option<Difficulty>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match difficulty {
            Some(difficulty) => serializer.serialize_some(difficulty.label()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Difficulty>, D::Error> {
        let name = Option::<String>::deserialize(deserializer)?;
        Ok(name.and_then(|name| {
            Difficulty::ALL
                .into_iter()
                .find(|difficulty| difficulty.label().eq_ignore_ascii_case(&name))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::game::{GameResult, MatchOutcome};

    /// A single 1920x1080 display with its top left corner at the origin, which
    /// is what one ordinary monitor looks like to gpui.
    const PRIMARY: Rect = Rect {
        x: 0.,
        y: 0.,
        width: 1920.,
        height: 1080.,
    };

    /// The window's minimum, as `ui::MIN_WINDOW_SIZE` has it.
    const MIN: (f32, f32) = (880., 660.);

    fn fit(rect: Rect, displays: &[Rect]) -> Option<Rect> {
        rect.fit_onto(displays, MIN.0, MIN.1)
    }

    /// A match in flight with every field of it saying something, so a round
    /// trip that drops one is a round trip that fails.
    fn saved_match() -> SavedMatch {
        SavedMatch {
            word: Word {
                word: "LAPTOP".into(),
                category: Some("Technology".into()),
                clue: Some("A computer you can close.".into()),
            },
            guessed: "AOPT".into(),
            wrong_guesses: 1,
            result: None,
            remaining: vec![Word::bare("BAGEL"), Word::bare("KAYAK")],
            difficulty: Some(Difficulty::Medium),
            pack: "Medium".into(),
            pack_words: 30,
            guess_budget: 8,
            words_won: 2,
            words_lost: 1,
            match_points: 520,
        }
    }

    // ------------------------------------------------------------ the file

    #[test]
    fn defaults_are_a_dark_window_with_nothing_restored() {
        let settings = Settings::default();

        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(settings.difficulty, None);
        assert_eq!(settings.window, None);
        assert_eq!(settings.stats, Stats::default());
    }

    #[test]
    fn settings_survive_a_round_trip() {
        let settings = Settings {
            theme: ThemeChoice::Light,
            difficulty: Some(Difficulty::Insane),
            window: Some(WindowFrame {
                rect: Rect::new(120., 64., 1000., 760.),
                maximized: true,
            }),
            stats: Stats::default(),
            in_flight: Some(saved_match()),
        };

        let json = serde_json::to_string_pretty(&settings).expect("settings should serialize");

        assert_eq!(Settings::parse(&json), settings);
    }

    #[test]
    fn the_file_stores_readable_names() {
        let settings = Settings {
            theme: ThemeChoice::Light,
            difficulty: Some(Difficulty::Medium),
            window: None,
            stats: Stats::default(),
            in_flight: None,
        };

        let json = serde_json::to_string(&settings).expect("settings should serialize");

        assert!(json.contains(r#""theme":"light""#), "{json}");
        assert!(json.contains(r#""difficulty":"Medium""#), "{json}");
    }

    #[test]
    fn corrupt_contents_fall_back_to_the_defaults() {
        for text in ["", "{", "not json at all", r#"{"theme": 7}"#, "[]"] {
            assert_eq!(Settings::parse(text), Settings::default(), "{text:?}");
        }
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        let settings = Settings::parse(r#"{"theme": "light"}"#);

        assert_eq!(settings.theme, ThemeChoice::Light);
        assert_eq!(settings.difficulty, None);
        assert_eq!(settings.window, None);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let settings = Settings::parse(r#"{"theme": "light", "hints": true}"#);

        assert_eq!(settings.theme, ThemeChoice::Light);
    }

    #[test]
    fn an_unknown_difficulty_reads_as_none() {
        assert_eq!(
            Settings::parse(r#"{"difficulty": "Trivial"}"#).difficulty,
            None
        );
    }

    #[test]
    fn difficulty_names_are_matched_case_insensitively() {
        assert_eq!(
            Settings::parse(r#"{"difficulty": "hard"}"#).difficulty,
            Some(Difficulty::Hard)
        );
    }

    #[test]
    fn every_difficulty_round_trips_by_name() {
        for difficulty in Difficulty::ALL {
            let settings = Settings {
                difficulty: Some(difficulty),
                ..Settings::default()
            };
            let json = serde_json::to_string(&settings).expect("settings should serialize");

            assert_eq!(Settings::parse(&json).difficulty, Some(difficulty));
        }
    }

    #[test]
    fn a_frame_with_no_maximized_flag_is_not_maximized() {
        let settings = Settings::parse(
            r#"{"window": {"rect": {"x": 0, "y": 0, "width": 900, "height": 700}}}"#,
        );

        assert_eq!(
            settings.window,
            Some(WindowFrame {
                rect: Rect::new(0., 0., 900., 700.),
                maximized: false,
            })
        );
    }

    // ----------------------------------------------------------- the stats

    /// A tally with something in every part of it, per-difficulty buckets
    /// included, so a round trip has something to lose.
    fn played_stats() -> Stats {
        let mut stats = Stats::default();
        stats.record_word(Some(Difficulty::Medium), GameResult::Lost, 0);
        // A clean Insane win, then a scrappy Easy one, then a custom list —
        // three in a row, so the streak and its bonus are in there too.
        stats.record_word(Some(Difficulty::Insane), GameResult::Won, 6);
        stats.record_word(Some(Difficulty::Easy), GameResult::Won, 2);
        stats.record_word(None, GameResult::Won, 4);
        stats.record_match(Some(Difficulty::Insane), MatchOutcome::Win);
        stats
    }

    #[test]
    fn a_file_from_before_the_stats_existed_reads_as_an_empty_tally() {
        let settings = Settings::parse(r#"{"theme": "light", "difficulty": "Hard"}"#);

        assert_eq!(settings.theme, ThemeChoice::Light);
        assert_eq!(settings.difficulty, Some(Difficulty::Hard));
        assert_eq!(settings.stats, Stats::default());
    }

    #[test]
    fn stats_survive_a_round_trip() {
        let settings = Settings {
            stats: played_stats(),
            ..Settings::default()
        };

        let json = serde_json::to_string_pretty(&settings).expect("settings should serialize");
        let parsed = Settings::parse(&json);

        assert_eq!(parsed, settings, "{json}");
        // Spelled out, because a bucket read back as an empty default would
        // still compare equal if the whole tally had been lost.
        assert_eq!(
            parsed.stats.for_difficulty(Difficulty::Insane).points,
            settings.stats.for_difficulty(Difficulty::Insane).points
        );
        assert_eq!(parsed.stats.streak, 3);
    }

    #[test]
    fn the_file_stores_the_stats_by_name() {
        let settings = Settings {
            stats: played_stats(),
            ..Settings::default()
        };

        let json = serde_json::to_string(&settings).expect("settings should serialize");

        assert!(json.contains(r#""stats":{"points":"#), "{json}");
        assert!(json.contains(r#""best_streak":3"#), "{json}");
        assert!(json.contains(r#""by_difficulty":{"Easy":"#), "{json}");
        assert!(json.contains(r#""Insane":{"points":440"#), "{json}");
    }

    #[test]
    fn an_unknown_difficulty_in_the_stats_is_ignored() {
        let settings = Settings::parse(
            r#"{"stats": {"points": 12, "by_difficulty": {"Trivial": {"points": 9}}}}"#,
        );

        assert_eq!(settings.stats.points, 12);
        for difficulty in Difficulty::ALL {
            assert_eq!(
                settings.stats.for_difficulty(difficulty).points,
                0,
                "{difficulty:?}"
            );
        }
    }

    #[test]
    fn garbage_stats_fall_back_to_an_empty_tally_without_losing_the_rest() {
        for text in [
            r#"{"theme": "light", "stats": 7}"#,
            r#"{"theme": "light", "stats": "none"}"#,
            r#"{"theme": "light", "stats": {"points": "lots"}}"#,
            r#"{"theme": "light", "stats": []}"#,
        ] {
            let settings = Settings::parse(text);

            assert_eq!(settings.stats, Stats::default(), "{text}");
            assert_eq!(settings.theme, ThemeChoice::Light, "{text}");
        }
    }

    // -------------------------------------------------------- the geometry

    #[test]
    fn a_frame_on_screen_is_restored_as_it_was() {
        let saved = Rect::new(200., 100., 1000., 760.);

        assert_eq!(fit(saved, &[PRIMARY]), Some(saved));
    }

    #[test]
    fn a_frame_hanging_off_an_edge_is_pushed_back_on() {
        // Dragged most of the way off the right edge, and below the bottom.
        let saved = Rect::new(1800., 900., 1000., 760.);

        assert_eq!(
            fit(saved, &[PRIMARY]),
            Some(Rect::new(920., 320., 1000., 760.))
        );
    }

    #[test]
    fn a_frame_smaller_than_the_minimum_grows_to_it() {
        let saved = Rect::new(0., 0., 300., 200.);

        assert_eq!(
            fit(saved, &[PRIMARY]),
            Some(Rect::new(0., 0., MIN.0, MIN.1))
        );
    }

    #[test]
    fn a_frame_bigger_than_the_display_shrinks_to_it() {
        // Saved on a 4K monitor, restored on a laptop.
        let saved = Rect::new(0., 0., 3840., 2160.);

        assert_eq!(fit(saved, &[PRIMARY]), Some(PRIMARY));
    }

    #[test]
    fn a_display_smaller_than_the_minimum_still_gives_a_usable_size() {
        let tiny = Rect::new(0., 0., 640., 480.);

        // The minimum wins: better a window that runs off a screen this small
        // than a layout that cannot draw itself.
        assert_eq!(
            fit(Rect::new(0., 0., 1000., 760.), &[tiny]),
            Some(Rect::new(0., 0., MIN.0, MIN.1))
        );
    }

    #[test]
    fn a_frame_on_a_second_monitor_keeps_its_place() {
        // A monitor to the left of the primary one, so its x is negative.
        let left = Rect::new(-1920., 0., 1920., 1080.);
        let saved = Rect::new(-1500., 200., 1000., 760.);

        assert_eq!(fit(saved, &[PRIMARY, left]), Some(saved));
    }

    #[test]
    fn a_frame_on_a_monitor_that_is_gone_is_not_restored() {
        // Saved on that second monitor, reopened with only the primary one.
        let saved = Rect::new(-1500., 200., 1000., 760.);

        assert_eq!(fit(saved, &[PRIMARY]), None);
    }

    #[test]
    fn a_frame_is_pinned_to_the_display_it_was_mostly_on() {
        let right = Rect::new(1920., 0., 1920., 1080.);
        // Straddling the seam, with the larger part on the right-hand monitor.
        let saved = Rect::new(1700., 100., 1000., 760.);

        // Pushed left until it fits on the right-hand monitor, not on the primary.
        assert_eq!(
            fit(saved, &[PRIMARY, right]),
            Some(Rect::new(1920., 100., 1000., 760.))
        );
    }

    #[test]
    fn nonsense_is_not_restored() {
        for saved in [
            Rect::new(f32::NAN, 0., 1000., 760.),
            Rect::new(0., 0., f32::INFINITY, 760.),
            Rect::new(0., 0., 0., 0.),
            Rect::new(0., 0., -1000., -760.),
        ] {
            assert_eq!(fit(saved, &[PRIMARY]), None, "{saved:?}");
        }
    }

    #[test]
    fn nothing_is_restored_without_a_display() {
        assert_eq!(fit(Rect::new(0., 0., 1000., 760.), &[]), None);
    }

    // -------------------------------------------------- the match in flight

    #[test]
    fn a_match_in_flight_survives_the_trip_through_a_snapshot() {
        let saved = saved_match();

        let round_tripped = SavedMatch::new(saved.clone().snapshot(), saved.match_points);

        assert_eq!(round_tripped, saved);
    }

    #[test]
    fn a_resolved_word_keeps_how_it_ended() {
        for (saved, expected) in [
            (SavedResult::Won, GameResult::Won),
            (SavedResult::Lost, GameResult::Lost),
        ] {
            let in_flight = SavedMatch {
                result: Some(saved),
                ..saved_match()
            };

            assert_eq!(in_flight.snapshot().result, Some(expected));
        }
    }

    #[test]
    fn the_guessed_letters_are_stored_as_one_readable_string() {
        let json = serde_json::to_string(&saved_match()).expect("a saved match should serialize");

        assert!(json.contains(r#""guessed":"AOPT""#), "{json}");
    }

    #[test]
    fn an_unreadable_match_costs_only_itself() {
        // The theme is written after `in_flight` on purpose: a key that failed
        // the whole parse would take everything, before it and after it alike.
        let text = r#"{
            "theme": "light",
            "in_flight": { "word": 7 },
            "difficulty": "Insane"
        }"#;

        let settings = Settings::parse(text);

        assert_eq!(settings.in_flight, None);
        assert_eq!(settings.theme, ThemeChoice::Light);
        assert_eq!(settings.difficulty, Some(Difficulty::Insane));
    }

    #[test]
    fn a_match_needs_nothing_but_a_word() {
        let text = r#"{ "in_flight": { "word": { "word": "LAPTOP" } } }"#;

        let in_flight = Settings::parse(text).in_flight.expect("a word is enough");

        assert_eq!(in_flight.word.word, "LAPTOP");
        assert_eq!(in_flight.guessed, "");
        assert_eq!(in_flight.result, None);
        assert_eq!(in_flight.difficulty, None);
    }

    #[test]
    fn a_match_with_no_word_in_it_is_no_match() {
        for text in [
            r#"{ "in_flight": {} }"#,
            r#"{ "in_flight": { "guessed": "AE" } }"#,
            r#"{ "in_flight": [] }"#,
        ] {
            assert_eq!(Settings::parse(text).in_flight, None, "{text:?}");
        }
    }

    #[test]
    fn a_settings_file_written_before_this_existed_simply_has_no_match() {
        let text = r#"{ "theme": "dark", "difficulty": "Easy" }"#;

        assert_eq!(Settings::parse(text).in_flight, None);
    }
}
