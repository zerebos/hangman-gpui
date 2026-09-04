//! The GPUI user interface: one window, laid out like the Swing original.
//!
//! The window is a left game column (title, alert, score, word, letter grid)
//! next to a right panel holding the gallows picture. The original's `Game`
//! menu becomes a toolbar strip under the title bar, because gpui-kit has no
//! menu-bar component.

mod gallows;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::label::Label;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Root, Selectable as _, Sizable as _, StyledExt as _,
    TitleBar, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::game::{Difficulty, Game, GameResult, GuessResult, MatchOutcome};
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

// The original's letter grid: seven buttons per row, with V-Z indented by one
// cell because the Java wrap rule started the fourth row at column 1.
const LETTERS_PER_ROW: usize = 7;
const INDENTED_ROW: usize = 3;
const LETTER_WIDTH: Pixels = px(44.);

actions!(hangman, [OpenWordList, ChangeWord]);

/// A line of feedback, styled after the original's `alertMessage` label:
/// italic, and green for good news or red for bad.
#[derive(Debug, Clone)]
struct Alert {
    text: SharedString,
    good: bool,
}

impl Alert {
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
    alert: Option<Alert>,
    /// GPUI only delivers key events to elements on the focus path, so the root
    /// element has to own a focus handle and actually be focused before typing
    /// a letter can reach us.
    focus_handle: FocusHandle,
}

impl HangmanView {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            game: Game::new(Difficulty::default()),
            alert: None,
            focus_handle: cx.focus_handle(),
        }
    }

    // ---------------------------------------------------------------- actions

    fn guess(&mut self, letter: char, cx: &mut Context<Self>) {
        let outcome = self.game.guess(letter);
        if outcome.result == GuessResult::Ignored {
            return;
        }

        self.alert = match outcome.result {
            GuessResult::Invalid => Some(Alert::bad(INVALID_GUESS)),
            // The original cleared the line on any ordinary guess, and only the
            // guess that ends the game leaves a message behind.
            _ => match outcome.game {
                Some(GameResult::Won) => Some(Alert::good(GAME_WON)),
                Some(GameResult::Lost) => Some(Alert::bad(GAME_LOST)),
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
        self.alert = Some(Alert::bad(GAVE_UP));
        cx.notify();
    }

    fn new_game(&mut self, cx: &mut Context<Self>) {
        // Returns false once the word list is exhausted; the footer shows the
        // match summary in that case rather than a button that does nothing.
        if self.game.new_game() {
            self.alert = None;
            cx.notify();
        }
    }

    fn set_difficulty(&mut self, difficulty: Difficulty, cx: &mut Context<Self>) {
        self.game.set_difficulty(difficulty);
        self.alert = None;
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
                self.alert = None;
                window.push_notification(
                    format!("Loaded {} words. New match!", self.game.total_words()),
                    cx,
                );
            }
            None => self.alert = Some(Alert::bad(FILE_ERROR)),
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

    // ------------------------------------------------------------- rendering

    /// The end-of-match message, derived rather than stored so it cannot drift
    /// out of sync with the score.
    fn match_summary(&self) -> Option<Alert> {
        let outcome = self.game.match_outcome()?;
        let wins = self.game.wins();
        let total = wins + self.game.losses();

        Some(match outcome {
            MatchOutcome::Win => Alert::good(format!("Good job, you got {wins} out of {total}")),
            MatchOutcome::Loss => {
                Alert::bad(format!("Nice try, you only got {wins} out of {total}"))
            }
            MatchOutcome::Tie => {
                Alert::good(format!("A tie, not bad, you got {wins} out of {total}"))
            }
        })
    }

    fn render_alert(alert: Option<Alert>, cx: &Context<Self>) -> impl IntoElement {
        // Fixed height so the layout below does not jump when the line appears.
        div().h(px(20.)).italic().when_some(alert, |this, alert| {
            let color = if alert.good {
                cx.theme().green
            } else {
                cx.theme().red
            };
            this.text_color(color).child(alert.text)
        })
    }

    fn render_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        let current = self.game.difficulty();

        h_flex()
            .w_full()
            .gap_2()
            .px_4()
            .py_2()
            .justify_between()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .gap_1()
                    .child(Label::new("Difficulty:").text_color(cx.theme().muted_foreground))
                    .children(Difficulty::ALL.map(|difficulty| {
                        Button::new(SharedString::from(format!("difficulty-{difficulty:?}")))
                            .small()
                            .label(difficulty.label())
                            .selected(current == Some(difficulty))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_difficulty(difficulty, cx)
                            }))
                    })),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("change-word")
                            .small()
                            .label("Change Word")
                            .tooltip("Give up on this word (Ctrl+N) — counts as a loss")
                            .on_click(cx.listener(|this, _, _, cx| this.give_up(cx))),
                    )
                    .child(
                        Button::new("open-word-list")
                            .small()
                            .label("Open word list…")
                            .tooltip("Play a .txt of your own (Ctrl+O)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.on_open_word_list(&OpenWordList, window, cx)
                            })),
                    ),
            )
    }

    fn render_record(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_5()
            .font_bold()
            .child(
                div()
                    .text_color(cx.theme().green)
                    .child(format!("Wins: {}", self.game.wins())),
            )
            .child(
                div()
                    .text_color(cx.theme().red)
                    .child(format!("Losses: {}", self.game.losses())),
            )
            .child(
                div()
                    .font_normal()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "Word {} of {}",
                        self.game.word_number(),
                        self.game.total_words()
                    )),
            )
    }

    fn render_word(&self, cx: &Context<Self>) -> impl IntoElement {
        // `cells()` already reveals the whole word once the game is over, and
        // marks ' ' and '/' as non-guessable so they show for free.
        h_flex()
            .gap_1()
            .flex_wrap()
            .children(self.game.cells().into_iter().map(|cell| {
                div()
                    .min_w(px(18.))
                    .text_center()
                    .text_xl()
                    .font_bold()
                    .font_family(cx.theme().mono_font_family.clone())
                    .when(!cell.revealed, |this| {
                        this.text_color(cx.theme().muted_foreground)
                    })
                    .child(cell.display().to_string())
            }))
    }

    fn render_letters(&self, cx: &Context<Self>) -> impl IntoElement {
        let letters: Vec<char> = ('A'..='Z').collect();
        let guessed = self.game.guessed_letters();
        let over = self.game.is_game_over();

        v_flex()
            .gap_1()
            .children(
                letters
                    .chunks(LETTERS_PER_ROW)
                    .enumerate()
                    .map(|(row, chunk)| {
                        h_flex()
                            .gap_1()
                            // The original's quirk: the V-Z row starts one cell in.
                            .when(row == INDENTED_ROW, |this| {
                                this.child(div().w(LETTER_WIDTH))
                            })
                            .children(chunk.iter().map(|&letter| {
                                Button::new(SharedString::from(format!("letter-{letter}")))
                                    .small()
                                    .w(LETTER_WIDTH)
                                    .label(letter.to_string())
                                    .disabled(over || guessed.contains(&letter))
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.guess(letter, cx)),
                                    )
                            }))
                    }),
            )
    }

    fn render_footer(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex().gap_3().h(px(32.)).map(|this| {
            if !self.game.is_game_over() {
                return this;
            }

            match self.match_summary() {
                // The match is over: `new_game()` would refuse, and the original
                // showed a dead button here. Show the score instead.
                Some(summary) => this.child(Self::render_alert(Some(summary), cx)).child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("Pick a difficulty to start a new match."),
                ),
                None => this.child(
                    Button::new("new-game")
                        .primary()
                        .label("New Game?")
                        .tooltip("Next word (Enter)")
                        .on_click(cx.listener(|this, _, _, cx| this.new_game(cx))),
                ),
            }
        })
    }

    fn render_game_column(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .id("game-column")
            .flex_1()
            .h_full()
            .gap_3()
            .overflow_y_scroll()
            .child(div().text_xl().font_bold().child("Hangman!"))
            .child(Self::render_alert(self.alert.clone(), cx))
            .child(self.render_record(cx))
            .child(Label::new("Word:").text_color(cx.theme().muted_foreground))
            .child(self.render_word(cx))
            .child(Label::new("Letters Available:").text_color(cx.theme().muted_foreground))
            .child(self.render_letters(cx))
            .child(self.render_footer(cx))
    }

    fn render_gallows_panel(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            // Wide enough for the 300px artwork plus the padding on both sides.
            .w(px(324.))
            .h_full()
            .p_3()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            // The gallows art is an opaque JPEG, so a themed background would
            // only show as a border around a white rectangle. Framing it in
            // white instead makes it read as a picture; drop this line once the
            // gallows is a transparent PNG.
            .bg(white())
            .child(gallows(self.game.wrong_guesses()))
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
            .child(TitleBar::new().child("Hangman!"))
            .child(self.render_toolbar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .items_stretch()
                    .gap_4()
                    .p_4()
                    .child(self.render_game_column(cx))
                    .child(self.render_gallows_panel(cx)),
            )
            // `Root` owns these overlays but does not render them for you.
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}
