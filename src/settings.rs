//! The handful of choices that outlive a launch.
//!
//! The game used to start identically every time: light/dark reset to dark, the
//! window reopened centred at its default size, and the difficulty went back to
//! Easy however you had left it. This module is the file that remembers those
//! three, written as JSON to the platform's own configuration directory:
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

use crate::game::Difficulty;

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

    // ------------------------------------------------------------ the file

    #[test]
    fn defaults_are_a_dark_window_with_nothing_restored() {
        let settings = Settings::default();

        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(settings.difficulty, None);
        assert_eq!(settings.window, None);
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
}
