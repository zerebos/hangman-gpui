//! The GPUI user interface: one dark-first window, laid out as a game board.
//!
//! The window is a title bar (wordmark plus a light/dark toggle), a toolbar
//! strip carrying the original's `Game` menu, and a body split into a left
//! play column — scoreboard, word, keyboard, result — and a right stage panel
//! holding the gallows artwork. Every colour comes from a gpui-kit theme
//! token, so both themes are usable and neither is hard-coded.

mod gallows;

use gpui_kit::component::alert::Alert;
use gpui_kit::component::button::{Button, ButtonGroup, ButtonVariants as _};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Root, Selectable as _, Sizable as _,
    StyledExt as _, Theme, ThemeMode, TitleBar, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::audio::Audio;
use crate::game::{
    Cell, Difficulty, Game, GameResult, GuessResult, MAX_WRONG_GUESSES, MatchOutcome,
};
use gallows::gallows;

/// The key context this view claims. Key bindings registered against it (see
/// `main.rs`) only fire while something inside the view has focus.
pub const KEY_CONTEXT: &str = "Hangman";

// The original's alert strings, verbatim.
const GAME_LOST: &str = "Bring Add/Drop Form!";
const GAME_WON: &str = "You WIN!";
const GAVE_UP: &str = "Giving up counts as a loss in my book.";
const INVALID_GUESS: &str = "You managed to guess an invalid character, congrats.";
const FILE_ERROR: &str = "Sorry, we couldnt read in your file.";

/// What the status line says when there is nothing else to report.
const IDLE_HINT: &str = "Type a letter, or click one above.";

// The original's letter grid: seven buttons per row, with V-Z indented by one
// cell because the Java wrap rule started the fourth row at column 1.
const LETTERS_PER_ROW: usize = 7;
const INDENTED_ROW: usize = 3;

/// One square letter key. Square, so the grid reads as a keyboard.
const KEY_SIZE: Pixels = px(42.);
/// The gap between keys, and between the cells of the word.
const KEY_GAP: Pixels = px(6.);

/// The stage column's width: the 300px artwork plus its panel padding.
const STAGE_WIDTH: Pixels = px(332.);

/// How wide one character of the word is, and how tall its glyph row is.
const WORD_CELL_WIDTH: Pixels = px(34.);
const WORD_CELL_HEIGHT: Pixels = px(38.);
/// The word is spaced wider than the keyboard: it should read as a puzzle.
const WORD_GAP: Pixels = px(10.);
/// The rule drawn under each guessable character.
const WORD_RULE_HEIGHT: Pixels = px(3.);

actions!(hangman, [OpenWordList, ChangeWord]);

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
    /// GPUI only delivers key events to elements on the focus path, so the root
    /// element has to own a focus handle and actually be focused before typing
    /// a letter can reach us.
    focus_handle: FocusHandle,
    /// The win/loss cues. Without the `sound` feature this is a zero-sized
    /// no-op, so the calls below need no `#[cfg]` of their own. It has to be
    /// owned here rather than created per clip: it holds the output device open.
    audio: Audio,
}

impl HangmanView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            game: Game::new(Difficulty::default()),
            notice: None,
            focus_handle: cx.focus_handle(),
            audio: Audio::new(),
        }
    }

    // ---------------------------------------------------------------- actions

    fn guess(&mut self, letter: char, cx: &mut Context<Self>) {
        let outcome = self.game.guess(letter);
        if outcome.result == GuessResult::Ignored {
            return;
        }

        self.notice = match outcome.result {
            GuessResult::Invalid => Some(Notice::bad(INVALID_GUESS)),
            // The original cleared the line on any ordinary guess, and only the
            // guess that ends the game leaves a message behind.
            _ => match outcome.game {
                // The two moments the Java played a cue. Giving up is handled
                // in `give_up`, which stays silent just as the original did.
                Some(GameResult::Won) => {
                    self.audio.play_win();
                    Some(Notice::good(GAME_WON))
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
        if self.game.is_game_over() {
            return;
        }
        self.game.give_up();
        self.notice = Some(Notice::bad(GAVE_UP));
        cx.notify();
    }

    fn new_game(&mut self, cx: &mut Context<Self>) {
        // Returns false once the word list is exhausted; the footer shows the
        // match summary in that case rather than a button that does nothing.
        if self.game.new_game() {
            self.notice = None;
            cx.notify();
        }
    }

    fn set_difficulty(&mut self, difficulty: Difficulty, cx: &mut Context<Self>) {
        self.game.set_difficulty(difficulty);
        self.notice = None;
        cx.notify();
    }

    /// Flip the whole application between the light and dark palettes.
    ///
    /// `Theme` is a GPUI *global* — one value owned by the app rather than by
    /// any view — so swapping it restyles every gpui-kit component at once.
    /// Handing `Theme::change` the window makes it repaint immediately.
    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = if cx.theme().is_dark() {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        Theme::change(next, Some(window), cx);
        cx.notify();
    }

    fn on_change_word(&mut self, _: &ChangeWord, _: &mut Window, cx: &mut Context<Self>) {
        self.give_up(cx);
    }

    /// The original's "Game > Open File...": pick a `.txt` with one word per line.
    fn on_open_word_list(&mut self, _: &OpenWordList, window: &mut Window, cx: &mut Context<Self>) {
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
        let loaded = contents.ok().and_then(|text| {
            let words = text.lines().map(str::to_owned).collect();
            self.game.set_word_list(words).ok()
        });

        match loaded {
            Some(()) => {
                self.notice = None;
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
        let wins = self.game.wins();
        let total = wins + self.game.losses();

        Some(match outcome {
            MatchOutcome::Win => Notice::good(format!("Good job, you got {wins} out of {total}")),
            MatchOutcome::Loss => {
                Notice::bad(format!("Nice try, you only got {wins} out of {total}"))
            }
            MatchOutcome::Tie => {
                Notice::good(format!("A tie, not bad, you got {wins} out of {total}"))
            }
        })
    }

    /// What the title bar shows beside the wordmark.
    fn subtitle(&self) -> SharedString {
        match self.game.difficulty() {
            Some(difficulty) => difficulty.label().into(),
            None => "Custom word list".into(),
        }
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
                h_flex().pr_2().child(
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
                            Button::new(SharedString::from(format!("difficulty-{difficulty:?}")))
                                .label(difficulty.label())
                                .selected(current == Some(difficulty))
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
                        Button::new("change-word")
                            .small()
                            .ghost()
                            .icon(IconName::RotateCw)
                            .label("Change Word")
                            .tooltip("Give up on this word (Ctrl+N) — counts as a loss")
                            .on_click(cx.listener(|this, _, _, cx| this.give_up(cx))),
                    )
                    .child(
                        Button::new("open-word-list")
                            .small()
                            .ghost()
                            .icon(IconName::FileText)
                            .label("Open word list…")
                            .tooltip("Play a .txt of your own (Ctrl+O)")
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

        panel(cx)
            .flex_row()
            .items_center()
            .gap_4()
            .px_5()
            .py_3()
            .child(Self::render_stat(
                self.game.wins().to_string(),
                "WINS",
                cx.theme().green,
                cx,
            ))
            .child(divider())
            .child(Self::render_stat(
                self.game.losses().to_string(),
                "LOSSES",
                cx.theme().red,
                cx,
            ))
            .child(divider())
            .child(Self::render_stat(
                format!("{} / {}", self.game.word_number(), self.game.total_words()),
                "WORD",
                cx.theme().foreground,
                cx,
            ))
    }

    /// One character of the word: the glyph, and the rule it sits on.
    fn render_word_cell(&self, cell: Cell, cx: &Context<Self>) -> impl IntoElement {
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
            .child(
                h_flex()
                    .h(WORD_CELL_HEIGHT)
                    .justify_center()
                    .text_size(px(30.))
                    .font_bold()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(glyph_color)
                    .when(cell.revealed, |this| this.child(cell.value.to_string())),
            )
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
        panel(cx)
            .gap_2()
            .px_5()
            .py_3()
            .child(eyebrow("THE WORD", cx))
            .child(
                h_flex()
                    .w_full()
                    .gap(WORD_GAP)
                    .flex_wrap()
                    .justify_center()
                    // `cells()` already reveals the whole word once the game is
                    // over, and marks ' ' and '/' as non-guessable so they show
                    // for free.
                    .children(
                        self.game
                            .cells()
                            .into_iter()
                            .map(|cell| self.render_word_cell(cell, cx)),
                    ),
            )
    }

    /// One letter key, dressed for whichever state it is in.
    ///
    /// `Selectable::selected` is doing the work for a guessed key: it paints
    /// the variant's *pressed* surface and suppresses the hover and active
    /// styling, which is exactly "this key has been played" — where
    /// `disabled` would instead wash the colour out to a faint tint.
    fn render_key(&self, letter: char, cx: &Context<Self>) -> impl IntoElement {
        let state = self.key_state(letter);
        let id = SharedString::from(format!("letter-{letter}"));

        Button::new(id)
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
            })
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

    /// The gallows stage: the artwork, plus the wrong-guess meter under it.
    fn render_stage(&self, cx: &Context<Self>) -> impl IntoElement {
        let wrong = self.game.wrong_guesses();

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
                            .child(format!("{wrong} / {MAX_WRONG_GUESSES}")),
                    ),
            )
            // The artwork sits on the panel itself: the gallows is drawn in
            // wood browns with a cartoon keyline, which reads on either theme.
            // The column stretches to the body's height, so this group takes
            // the slack and stays centred rather than clinging to the top.
            .child(
                v_flex()
                    .flex_1()
                    .justify_center()
                    .items_center()
                    .gap_4()
                    .child(gallows(wrong))
                    .child(
                        // Six pips, one per wrong guess: the score the drawing
                        // is keeping, in a form you can count at a glance.
                        h_flex()
                            .gap_1p5()
                            .children((0..MAX_WRONG_GUESSES).map(|step| {
                                div().size(px(9.)).rounded_full().bg(if step < wrong {
                                    cx.theme().danger
                                } else {
                                    cx.theme().muted_foreground.alpha(0.25)
                                })
                            })),
                    ),
            )
    }

    fn render_play_column(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .id("play-column")
            .flex_1()
            .h_full()
            .gap_3()
            .overflow_y_scroll()
            .child(self.render_scoreboard(cx))
            .child(self.render_word_panel(cx))
            .child(self.render_keyboard(cx))
            .child(if self.game.is_game_over() {
                self.render_result(cx).into_any_element()
            } else {
                self.render_status_line(cx).into_any_element()
            })
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
                    .gap_5()
                    .p_5()
                    .child(self.render_play_column(cx))
                    .child(self.render_stage(cx)),
            )
            // `Root` owns these overlays but does not render them for you.
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}
