//! Pure hangman game logic — no UI, no I/O, no globals.
//!
//! This is a port of the backend of Zack Rauen's 2015 Java hangman
//! (`com.zackrauen.hangman.backend.Hangman`). The rules are preserved; the
//! internal design is not, because a few of the Java tricks do not translate
//! into idiomatic Rust. Those deliberate deviations are called out in comments
//! where they occur.
//!
//! # Vocabulary
//!
//! * A **game** is one word: you guess letters until you reveal the word (a
//!   win) or run out of wrong guesses (a loss).
//! * A **match** is one pass through a whole word list — ten games for the
//!   bundled lists. Words are drawn at random *without replacement*, so a match
//!   never repeats a word. When the list runs out, the match is over and is
//!   scored as a [`MatchOutcome`] by comparing wins against losses.

use std::collections::BTreeSet;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

/// How many wrong guesses a player gets before losing the game.
///
/// The Java original had a `setMaximumWrongGuesses` setter, but nothing ever
/// called it — so difficulty changes the word list and nothing else. That
/// behaviour is preserved here by making the budget a constant.
pub const MAX_WRONG_GUESSES: usize = 6;

/// Which bundled word list a match is played from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Difficulty {
    /// The list the original game starts on.
    #[default]
    Easy,
    Medium,
    Hard,
    Insane,
}

impl Difficulty {
    /// Every difficulty, in menu order. Handy for building a UI picker.
    pub const ALL: [Difficulty; 4] = [
        Difficulty::Easy,
        Difficulty::Medium,
        Difficulty::Hard,
        Difficulty::Insane,
    ];

    /// A human-readable name, as shown in the original's Difficulty menu.
    pub fn label(self) -> &'static str {
        match self {
            Difficulty::Easy => "Easy",
            Difficulty::Medium => "Medium",
            Difficulty::Hard => "Hard",
            Difficulty::Insane => "Insane",
        }
    }

    /// How much this difficulty multiplies a solved word by.
    ///
    /// The rules themselves do not use this — the guess budget is the same on
    /// all four (see [`MAX_WRONG_GUESSES`]) — but which list you chose is a
    /// property of the difficulty rather than of the scoreboard, so the number
    /// lives here and [`crate::stats`] does the arithmetic with it.
    pub fn weight(self) -> u32 {
        match self {
            Difficulty::Easy => 1,
            Difficulty::Medium => 2,
            Difficulty::Hard => 3,
            Difficulty::Insane => 4,
        }
    }

    /// The raw contents of this difficulty's word list.
    ///
    /// The four lists are baked into the binary with `include_str!`, so there
    /// are no data files to ship next to the executable.
    fn raw_list(self) -> &'static str {
        match self {
            Difficulty::Easy => include_str!("../assets/words/easy.txt"),
            Difficulty::Medium => include_str!("../assets/words/med.txt"),
            Difficulty::Hard => include_str!("../assets/words/hard.txt"),
            Difficulty::Insane => include_str!("../assets/words/insane.txt"),
        }
    }

    /// This difficulty's words, one per line, already cleaned up.
    pub fn words(self) -> Vec<String> {
        sanitize_words(self.raw_list().lines().map(|line| line.to_string()))
    }
}

/// What happened when a letter was guessed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuessResult {
    /// The letter is in the word. No penalty.
    Correct,
    /// The letter is not in the word. Costs one wrong guess.
    Wrong,
    /// This letter was already guessed. No penalty, exactly as in the original.
    Duplicate,
    /// Not an ASCII letter (a digit, punctuation, `' '`, `'/'`, …). No penalty.
    Invalid,
    /// The game (or the whole match) was already over, so nothing happened.
    Ignored,
}

/// How a single game ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameResult {
    /// Every guessable character was revealed.
    Won,
    /// The player ran out of guesses, or gave up.
    Lost,
}

/// How a whole match ended, decided by comparing wins against losses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchOutcome {
    /// More wins than losses.
    Win,
    /// More losses than wins.
    Loss,
    /// Equal wins and losses.
    Tie,
}

/// Everything one call to [`Game::guess`] changed.
///
/// `game` and `match_` are `Some` only on the guess that actually ended the
/// game (or the match), so a UI can use them directly as "fire this
/// announcement now" signals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuessOutcome {
    /// How the guess itself resolved.
    pub result: GuessResult,
    /// Set when this guess ended the current game.
    pub game: Option<GameResult>,
    /// Set when this guess also ended the match.
    ///
    /// NOTE: this is a deliberate deviation from the original, which never
    /// reported the per-game win/loss on the final word of a match — only the
    /// match summary — so the last game of every match silently showed no
    /// feedback. Here both `game` and `match_` are reported together.
    pub match_: Option<MatchOutcome>,
}

/// One character of the current word, ready to be laid out by a UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    /// The real character, always uppercase for letters.
    pub value: char,
    /// Whether the player has to guess this character at all.
    ///
    /// `' '` and `'/'` are not guessable: they show for free and are ignored by
    /// the win check.
    pub guessable: bool,
    /// Whether the character is currently shown rather than hidden as `_`.
    pub revealed: bool,
}

impl Cell {
    /// The character to draw: the value itself, or `'_'` while it is hidden.
    pub fn display(self) -> char {
        if self.revealed { self.value } else { '_' }
    }
}

/// Returned when a word list has no usable words in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyWordList;

impl std::fmt::Display for EmptyWordList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the word list contains no playable words")
    }
}

impl std::error::Error for EmptyWordList {}

/// Trim each line, drop the ones with nothing to guess, and uppercase the rest.
///
/// The bundled lists are already well behaved; this exists because the original
/// let the player load any `.txt` file as a word list, and blank or
/// punctuation-only lines would otherwise produce an unplayable "word".
fn sanitize_words(words: impl IntoIterator<Item = String>) -> Vec<String> {
    words
        .into_iter()
        .map(|word| word.trim().to_ascii_uppercase())
        .filter(|word| word.chars().any(|c| c.is_ascii_alphabetic()))
        .collect()
}

/// A hangman match in progress.
///
/// Construct one with [`Game::new`] (or [`Game::with_seed`] for reproducible
/// word order), then drive it with [`Game::guess`], [`Game::new_game`] and
/// [`Game::give_up`]. The first word is dealt by the constructor, so a freshly
/// built `Game` is immediately playable.
pub struct Game {
    /// `None` once a custom word list has been loaded from a file.
    difficulty: Option<Difficulty>,
    /// Words not yet played this match. A word is removed as it is dealt, which
    /// is what stops a match from repeating a word.
    remaining_words: Vec<String>,
    /// How many words the match started with, for "word 3 of 10" style UI.
    total_words: usize,
    /// The current word, uppercased.
    word: String,
    // The Java version stored shared `HangmanCharacter` objects in both the
    // alphabet and the word, so marking a letter guessed mutated both at once.
    // Rust makes that kind of aliasing deliberately awkward, and it is not
    // needed: keeping the word plus the set of guessed letters and *deriving*
    // the display is simpler, cheaper to reason about, and impossible to get
    // out of sync.
    guessed: BTreeSet<char>,
    wrong_guesses: usize,
    result: Option<GameResult>,
    // Per-match, and reset by `reset` along with the word pool: these exist so
    // `finish_match` can compare them, which is a *rule*. Points, streaks and
    // anything that outlives a match are `crate::stats`' business, not this
    // module's.
    words_won: usize,
    words_lost: usize,
    match_outcome: Option<MatchOutcome>,
    rng: StdRng,
}

impl Game {
    /// Start a match on `difficulty`, seeded from the operating system.
    pub fn new(difficulty: Difficulty) -> Self {
        Self::start(Some(difficulty), difficulty.words(), rand::make_rng())
    }

    /// Start a match on `difficulty` with a fixed RNG seed.
    ///
    /// Same seed, same word order — which is what makes the tests (and any
    /// "daily puzzle" feature) deterministic.
    pub fn with_seed(difficulty: Difficulty, seed: u64) -> Self {
        Self::start(
            Some(difficulty),
            difficulty.words(),
            StdRng::seed_from_u64(seed),
        )
    }

    /// Start a match on an arbitrary word list, e.g. one loaded from a file.
    ///
    /// Blank and punctuation-only lines are dropped; the rest are uppercased.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyWordList`] if nothing playable survives that cleanup.
    pub fn from_words(words: Vec<String>) -> Result<Self, EmptyWordList> {
        Self::from_words_with_rng(words, rand::make_rng())
    }

    /// [`Game::from_words`] with a fixed RNG seed.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyWordList`] if the list has no playable words.
    pub fn from_words_with_seed(words: Vec<String>, seed: u64) -> Result<Self, EmptyWordList> {
        Self::from_words_with_rng(words, StdRng::seed_from_u64(seed))
    }

    fn from_words_with_rng(words: Vec<String>, rng: StdRng) -> Result<Self, EmptyWordList> {
        let words = sanitize_words(words);
        if words.is_empty() {
            return Err(EmptyWordList);
        }
        Ok(Self::start(None, words, rng))
    }

    /// Shared constructor body. `words` must already be sanitized and non-empty
    /// for the game to be playable; an empty list yields an immediately-over
    /// match, which only the bundled lists could never produce.
    fn start(difficulty: Option<Difficulty>, words: Vec<String>, rng: StdRng) -> Self {
        let total_words = words.len();
        let mut game = Game {
            difficulty,
            remaining_words: words,
            total_words,
            word: String::new(),
            guessed: BTreeSet::new(),
            wrong_guesses: 0,
            result: None,
            words_won: 0,
            words_lost: 0,
            match_outcome: None,
            rng,
        };
        // Deal the first word so the caller gets a playable game.
        if !game.deal_word() {
            game.finish_match();
        }
        game
    }

    /// Take a random word out of the remaining pool. Returns `false` if the
    /// pool was empty.
    fn deal_word(&mut self) -> bool {
        if self.remaining_words.is_empty() {
            return false;
        }
        let index = self.rng.random_range(0..self.remaining_words.len());
        // `swap_remove` is O(1) and order does not matter — we draw at random.
        self.word = self.remaining_words.swap_remove(index);
        self.guessed.clear();
        self.wrong_guesses = 0;
        self.result = None;
        true
    }

    // ---------------------------------------------------------------- actions

    /// Guess a letter.
    ///
    /// Case does not matter. Non-letters, repeats, and guesses made after the
    /// game is over cost nothing — see [`GuessResult`].
    pub fn guess(&mut self, letter: char) -> GuessOutcome {
        let ignored = GuessOutcome {
            result: GuessResult::Ignored,
            game: None,
            match_: None,
        };
        if self.is_game_over() {
            return ignored;
        }

        let letter = letter.to_ascii_uppercase();
        // The original detected non-guessable characters by catching the
        // `ArrayIndexOutOfBoundsException` that `ArrayList.get(-1)` threw on a
        // failed `indexOf`; an explicit check says the same thing out loud.
        if !letter.is_ascii_alphabetic() {
            return GuessOutcome {
                result: GuessResult::Invalid,
                game: None,
                match_: None,
            };
        }
        if !self.guessed.insert(letter) {
            return GuessOutcome {
                result: GuessResult::Duplicate,
                game: None,
                match_: None,
            };
        }

        if self.word.contains(letter) {
            if self.is_word_complete() {
                self.end_game(GameResult::Won, GuessResult::Correct)
            } else {
                GuessOutcome {
                    result: GuessResult::Correct,
                    game: None,
                    match_: None,
                }
            }
        } else {
            self.wrong_guesses += 1;
            if self.wrong_guesses >= MAX_WRONG_GUESSES {
                self.end_game(GameResult::Lost, GuessResult::Wrong)
            } else {
                GuessOutcome {
                    result: GuessResult::Wrong,
                    game: None,
                    match_: None,
                }
            }
        }
    }

    /// Give up on the current word: an instant loss that counts in the tally.
    ///
    /// Returns the match outcome if this was the last word of the match. Does
    /// nothing if the game is already over.
    pub fn give_up(&mut self) -> Option<MatchOutcome> {
        if self.is_game_over() {
            return None;
        }
        self.end_game(GameResult::Lost, GuessResult::Ignored).match_
    }

    /// Record the end of a game and, if the word pool is exhausted, the match.
    fn end_game(&mut self, result: GameResult, guess_result: GuessResult) -> GuessOutcome {
        self.result = Some(result);
        match result {
            GameResult::Won => self.words_won += 1,
            GameResult::Lost => self.words_lost += 1,
        }
        // The match ends when the last word has been played out.
        let match_ = if self.remaining_words.is_empty() {
            self.finish_match();
            self.match_outcome
        } else {
            None
        };
        GuessOutcome {
            result: guess_result,
            game: Some(result),
            match_,
        }
    }

    fn finish_match(&mut self) {
        self.match_outcome = Some(match self.words_won.cmp(&self.words_lost) {
            std::cmp::Ordering::Greater => MatchOutcome::Win,
            std::cmp::Ordering::Less => MatchOutcome::Loss,
            std::cmp::Ordering::Equal => MatchOutcome::Tie,
        });
    }

    /// Deal the next word of the match.
    ///
    /// Returns `false` (and changes nothing) if the match is over, or if the
    /// current game is still in progress — matching the original, where a new
    /// word could only be requested once the current one had been resolved.
    pub fn new_game(&mut self) -> bool {
        if self.is_match_over() || !self.is_game_over() {
            return false;
        }
        self.deal_word()
    }

    /// Abandon the current match and start a fresh one on `difficulty`.
    ///
    /// The per-match word tally and the word pool both reset, and the first
    /// word is dealt. The lifetime score and the streak in [`crate::stats`] are
    /// untouched — a streak spans matches on purpose.
    pub fn set_difficulty(&mut self, difficulty: Difficulty) {
        self.reset(Some(difficulty), difficulty.words());
    }

    /// Abandon the current match and start a fresh one on a custom word list.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyWordList`] if the list has no playable words; the current
    /// match is left untouched in that case.
    pub fn set_word_list(&mut self, words: Vec<String>) -> Result<(), EmptyWordList> {
        let words = sanitize_words(words);
        if words.is_empty() {
            return Err(EmptyWordList);
        }
        self.reset(None, words);
        Ok(())
    }

    fn reset(&mut self, difficulty: Option<Difficulty>, words: Vec<String>) {
        self.difficulty = difficulty;
        self.total_words = words.len();
        self.remaining_words = words;
        self.words_won = 0;
        self.words_lost = 0;
        self.match_outcome = None;
        if !self.deal_word() {
            self.finish_match();
        }
    }

    // ---------------------------------------------------------------- queries

    /// The word being guessed, uppercased. Use this for the game-over reveal.
    pub fn word(&self) -> &str {
        &self.word
    }

    /// The current word as display cells, one per character.
    ///
    /// Once the game is over every cell is revealed, so the UI can render the
    /// answer without special-casing anything.
    pub fn cells(&self) -> Vec<Cell> {
        let over = self.is_game_over();
        self.word
            .chars()
            .map(|value| {
                let guessable = value.is_ascii_alphabetic();
                Cell {
                    value,
                    guessable,
                    revealed: over || !guessable || self.guessed.contains(&value),
                }
            })
            .collect()
    }

    /// The current word as a string of revealed letters and `_` placeholders,
    /// e.g. `"A__LES_UCE"`.
    pub fn display(&self) -> String {
        self.cells().into_iter().map(Cell::display).collect()
    }

    /// Every letter guessed so far this game, in alphabetical order.
    pub fn guessed_letters(&self) -> &BTreeSet<char> {
        &self.guessed
    }

    /// The letters `'A'..='Z'` that have not been guessed yet.
    pub fn available_letters(&self) -> Vec<char> {
        ('A'..='Z')
            .filter(|letter| !self.guessed.contains(letter))
            .collect()
    }

    /// How many wrong guesses have been made this game (0..=[`MAX_WRONG_GUESSES`]).
    ///
    /// This doubles as the index of the gallows drawing stage.
    pub fn wrong_guesses(&self) -> usize {
        self.wrong_guesses
    }

    /// How many wrong guesses are still affordable.
    pub fn remaining_guesses(&self) -> usize {
        MAX_WRONG_GUESSES.saturating_sub(self.wrong_guesses)
    }

    /// Whether the current game has been resolved, one way or another.
    pub fn is_game_over(&self) -> bool {
        self.result.is_some()
    }

    /// How the current game ended, or `None` while it is still in progress.
    pub fn game_result(&self) -> Option<GameResult> {
        self.result
    }

    /// Whether the current game was won.
    pub fn is_won(&self) -> bool {
        self.result == Some(GameResult::Won)
    }

    /// Words won so far this match.
    pub fn words_won(&self) -> usize {
        self.words_won
    }

    /// Words lost so far this match.
    pub fn words_lost(&self) -> usize {
        self.words_lost
    }

    /// The difficulty being played, or `None` for a custom word list.
    pub fn difficulty(&self) -> Option<Difficulty> {
        self.difficulty
    }

    /// Whether every word in the list has been played.
    pub fn is_match_over(&self) -> bool {
        self.match_outcome.is_some()
    }

    /// How the match ended, or `None` while it is still running.
    pub fn match_outcome(&self) -> Option<MatchOutcome> {
        self.match_outcome
    }

    /// How many words the match started with.
    pub fn total_words(&self) -> usize {
        self.total_words
    }

    /// Which word of the match is on screen, 1-based — the "3" in "word 3 of 10".
    pub fn word_number(&self) -> usize {
        self.total_words - self.remaining_words.len()
    }

    /// Whether every guessable character of the current word has been guessed.
    fn is_word_complete(&self) -> bool {
        self.word
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .all(|c| self.guessed.contains(&c))
    }
}

impl Default for Game {
    fn default() -> Self {
        Game::new(Difficulty::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A game on a single fixed word, so tests never depend on the RNG.
    fn game_with_word(word: &str) -> Game {
        Game::from_words_with_seed(vec![word.to_string()], 1).expect("word list is not empty")
    }

    /// Guess every distinct letter of the current word, winning the game.
    fn win_current_game(game: &mut Game) {
        let letters: Vec<char> = game
            .word()
            .chars()
            .filter(|c| c.is_ascii_alphabetic())
            .collect();
        for letter in letters {
            game.guess(letter);
        }
    }

    /// Burn all six wrong guesses on letters that are not in the current word.
    fn lose_current_game(game: &mut Game) {
        let word = game.word().to_string();
        let wrong: Vec<char> = ('A'..='Z').filter(|c| !word.contains(*c)).collect();
        for letter in wrong.into_iter().take(MAX_WRONG_GUESSES) {
            game.guess(letter);
        }
    }

    #[test]
    fn bundled_lists_all_have_ten_words() {
        for difficulty in Difficulty::ALL {
            assert_eq!(difficulty.words().len(), 10, "{}", difficulty.label());
        }
    }

    #[test]
    fn default_difficulty_is_easy() {
        assert_eq!(Difficulty::default(), Difficulty::Easy);
        assert_eq!(Game::default().difficulty(), Some(Difficulty::Easy));
    }

    #[test]
    fn difficulty_weights_climb_one_step_per_level() {
        assert_eq!(
            Difficulty::ALL.map(Difficulty::weight),
            [1, 2, 3, 4],
            "the four lists are worth 1x to 4x"
        );
    }

    #[test]
    fn the_easiest_difficulty_is_worth_the_least() {
        assert_eq!(Difficulty::default().weight(), 1);
        assert!(Difficulty::Insane.weight() > Difficulty::Easy.weight());
    }

    #[test]
    fn correct_guess_reveals_every_occurrence() {
        let mut game = game_with_word("BANANA");
        assert_eq!(game.display(), "______");
        assert_eq!(game.guess('A').result, GuessResult::Correct);
        assert_eq!(game.display(), "_A_A_A");
        assert_eq!(game.wrong_guesses(), 0);
    }

    #[test]
    fn wrong_guess_costs_one_guess() {
        let mut game = game_with_word("BANANA");
        assert_eq!(game.guess('Z').result, GuessResult::Wrong);
        assert_eq!(game.wrong_guesses(), 1);
        assert_eq!(game.remaining_guesses(), MAX_WRONG_GUESSES - 1);
        assert_eq!(game.display(), "______");
    }

    #[test]
    fn duplicate_guess_is_free() {
        let mut game = game_with_word("BANANA");
        game.guess('Z');
        assert_eq!(game.guess('Z').result, GuessResult::Duplicate);
        assert_eq!(game.wrong_guesses(), 1);

        game.guess('A');
        assert_eq!(game.guess('A').result, GuessResult::Duplicate);
        assert_eq!(game.wrong_guesses(), 1);
    }

    #[test]
    fn invalid_guess_is_free() {
        let mut game = game_with_word("ADD/DROP FORM");
        for bad in ['1', '/', ' ', '!', 'é'] {
            assert_eq!(game.guess(bad).result, GuessResult::Invalid, "{bad:?}");
        }
        assert_eq!(game.wrong_guesses(), 0);
        assert!(game.guessed_letters().is_empty());
    }

    #[test]
    fn guesses_are_case_insensitive() {
        let mut game = game_with_word("Banana");
        assert_eq!(game.word(), "BANANA");
        assert_eq!(game.guess('a').result, GuessResult::Correct);
        assert_eq!(game.guess('A').result, GuessResult::Duplicate);
        assert_eq!(game.display(), "_A_A_A");
    }

    #[test]
    fn spaces_and_slashes_are_free() {
        let mut game = game_with_word("Add/Drop Form");
        // Non-letters show immediately and never need guessing.
        assert_eq!(game.display(), "___/____ ____");
        for letter in ['A', 'D', 'R', 'O', 'P', 'F', 'M'] {
            game.guess(letter);
        }
        assert!(game.is_won(), "display was {}", game.display());
        assert_eq!(game.wrong_guesses(), 0);
    }

    #[test]
    fn six_wrong_guesses_lose_the_game() {
        let mut game = game_with_word("BANANA");
        for letter in ['C', 'D', 'E', 'F', 'G'] {
            let outcome = game.guess(letter);
            assert_eq!(outcome.result, GuessResult::Wrong);
            assert_eq!(outcome.game, None);
            assert!(!game.is_game_over());
        }
        let outcome = game.guess('H');
        assert_eq!(game.wrong_guesses(), MAX_WRONG_GUESSES);
        assert_eq!(outcome.game, Some(GameResult::Lost));
        assert!(game.is_game_over());
        assert!(!game.is_won());
        assert_eq!(game.words_lost(), 1);
        assert_eq!(game.words_won(), 0);
        // The whole word is revealed once the game is over.
        assert_eq!(game.display(), "BANANA");
    }

    #[test]
    fn guessing_all_letters_wins_the_game() {
        let mut game = game_with_word("BANANA");
        assert_eq!(game.guess('B').game, None);
        assert_eq!(game.guess('A').game, None);
        let outcome = game.guess('N');
        assert_eq!(outcome.result, GuessResult::Correct);
        assert_eq!(outcome.game, Some(GameResult::Won));
        assert!(game.is_won());
        assert_eq!(game.words_won(), 1);
        assert_eq!(game.words_lost(), 0);
        assert_eq!(game.display(), "BANANA");
    }

    #[test]
    fn guessing_after_game_over_is_a_no_op() {
        let mut game = game_with_word("BANANA");
        win_current_game(&mut game);
        assert!(game.is_game_over());
        let outcome = game.guess('Z');
        assert_eq!(outcome.result, GuessResult::Ignored);
        assert_eq!(game.wrong_guesses(), 0);
        assert_eq!(game.words_won(), 1);
    }

    #[test]
    fn give_up_counts_as_a_loss() {
        let mut game = Game::from_words_with_seed(vec!["BANANA".into(), "APPLE".into()], 7)
            .expect("word list is not empty");
        assert_eq!(game.give_up(), None); // not the last word yet
        assert!(game.is_game_over());
        assert!(!game.is_won());
        assert_eq!(game.words_lost(), 1);
        assert_eq!(game.game_result(), Some(GameResult::Lost));
        // Giving up twice must not double-count.
        assert_eq!(game.give_up(), None);
        assert_eq!(game.words_lost(), 1);
    }

    #[test]
    fn available_letters_shrink_as_letters_are_guessed() {
        let mut game = game_with_word("BANANA");
        assert_eq!(game.available_letters().len(), 26);
        game.guess('A');
        game.guess('Z');
        assert_eq!(game.available_letters().len(), 24);
        assert!(!game.available_letters().contains(&'A'));
        assert_eq!(
            game.guessed_letters().iter().copied().collect::<Vec<_>>(),
            vec!['A', 'Z']
        );
    }

    #[test]
    fn new_game_deals_a_fresh_word_and_clears_state() {
        let mut game = Game::from_words_with_seed(vec!["BANANA".into(), "APPLE".into()], 3)
            .expect("word list is not empty");
        game.guess('Z');
        assert!(!game.new_game(), "cannot skip a game that is still running");
        game.give_up();
        assert!(game.new_game());
        assert_eq!(game.wrong_guesses(), 0);
        assert!(game.guessed_letters().is_empty());
        assert!(!game.is_game_over());
        assert_eq!(game.word_number(), 2);
    }

    #[test]
    fn a_match_never_repeats_a_word() {
        let mut game = Game::with_seed(Difficulty::Easy, 42);
        let expected = Difficulty::Easy.words();
        let mut seen = vec![game.word().to_string()];
        while !game.is_match_over() {
            game.give_up();
            if game.new_game() {
                seen.push(game.word().to_string());
            }
        }
        assert_eq!(seen.len(), expected.len());
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), seen.len(), "a word was dealt twice: {seen:?}");
        let mut expected_sorted = expected;
        expected_sorted.sort();
        assert_eq!(sorted, expected_sorted);
    }

    #[test]
    fn match_ends_after_the_last_word() {
        let mut game = Game::with_seed(Difficulty::Hard, 99);
        assert_eq!(game.total_words(), 10);
        for word in 1..=10 {
            assert_eq!(game.word_number(), word);
            assert!(!game.is_match_over(), "match ended early at word {word}");
            win_current_game(&mut game);
            game.new_game();
        }
        assert!(game.is_match_over());
        assert_eq!(game.words_won(), 10);
        // Once the match is over, no more words are dealt.
        assert!(!game.new_game());
    }

    #[test]
    fn final_game_reports_both_the_game_and_the_match_result() {
        // The Java original skipped the per-game event on the last word; the
        // port reports both, and this test locks that in.
        let mut game =
            Game::from_words_with_seed(vec!["CAT".into()], 5).expect("word list is not empty");
        game.guess('C');
        game.guess('A');
        let outcome = game.guess('T');
        assert_eq!(outcome.result, GuessResult::Correct);
        assert_eq!(outcome.game, Some(GameResult::Won));
        assert_eq!(outcome.match_, Some(MatchOutcome::Win));
        assert!(game.is_match_over());
    }

    #[test]
    fn match_outcome_is_win_when_wins_exceed_losses() {
        let mut game =
            Game::from_words_with_seed(vec!["ONE".into(), "TWO".into(), "SIX".into()], 11)
                .expect("word list is not empty");
        win_current_game(&mut game);
        game.new_game();
        win_current_game(&mut game);
        game.new_game();
        lose_current_game(&mut game);
        assert_eq!(game.match_outcome(), Some(MatchOutcome::Win));
        assert_eq!((game.words_won(), game.words_lost()), (2, 1));
    }

    #[test]
    fn match_outcome_is_loss_when_losses_exceed_wins() {
        let mut game =
            Game::from_words_with_seed(vec!["ONE".into(), "TWO".into(), "SIX".into()], 11)
                .expect("word list is not empty");
        lose_current_game(&mut game);
        game.new_game();
        lose_current_game(&mut game);
        game.new_game();
        win_current_game(&mut game);
        assert_eq!(game.match_outcome(), Some(MatchOutcome::Loss));
        assert_eq!((game.words_won(), game.words_lost()), (1, 2));
    }

    #[test]
    fn match_outcome_is_tie_when_wins_equal_losses() {
        let mut game = Game::from_words_with_seed(vec!["ONE".into(), "TWO".into()], 11)
            .expect("word list is not empty");
        win_current_game(&mut game);
        game.new_game();
        let outcome = game.give_up();
        assert_eq!(outcome, Some(MatchOutcome::Tie));
        assert_eq!(game.match_outcome(), Some(MatchOutcome::Tie));
    }

    #[test]
    fn giving_up_on_the_last_word_ends_the_match() {
        let mut game =
            Game::from_words_with_seed(vec!["CAT".into()], 5).expect("word list is not empty");
        assert_eq!(game.give_up(), Some(MatchOutcome::Loss));
        assert!(game.is_match_over());
    }

    #[test]
    fn switching_difficulty_starts_a_fresh_match() {
        let mut game = Game::with_seed(Difficulty::Easy, 1);
        lose_current_game(&mut game);
        assert_eq!(game.words_lost(), 1);

        game.set_difficulty(Difficulty::Insane);
        assert_eq!(game.difficulty(), Some(Difficulty::Insane));
        assert_eq!((game.words_won(), game.words_lost()), (0, 0));
        assert!(!game.is_game_over());
        assert!(!game.is_match_over());
        assert_eq!(game.word_number(), 1);
        assert!(
            Difficulty::Insane
                .words()
                .contains(&game.word().to_string())
        );
    }

    #[test]
    fn a_custom_word_list_has_no_difficulty() {
        let game = Game::from_words(vec!["Hello World".into()]).expect("word list is not empty");
        assert_eq!(game.difficulty(), None);
        assert_eq!(game.word(), "HELLO WORLD");
        assert_eq!(game.display(), "_____ _____");
    }

    #[test]
    fn unplayable_word_lists_are_rejected() {
        assert!(Game::from_words(Vec::new()).is_err());
        // Blank and punctuation-only lines are dropped, leaving nothing to play.
        let junk = vec!["".to_string(), "   ".to_string(), "42 / 7".to_string()];
        assert!(Game::from_words(junk).is_err());
    }

    #[test]
    fn set_word_list_rejects_empty_lists_without_disturbing_the_match() {
        let mut game = Game::with_seed(Difficulty::Easy, 1);
        let word = game.word().to_string();
        assert_eq!(
            game.set_word_list(vec!["  ".into(), "123".into()]),
            Err(EmptyWordList)
        );
        assert_eq!(game.word(), word);
        assert_eq!(game.difficulty(), Some(Difficulty::Easy));

        assert!(game.set_word_list(vec!["Rustacean".into()]).is_ok());
        assert_eq!(game.word(), "RUSTACEAN");
        assert_eq!(game.difficulty(), None);
    }

    #[test]
    fn seeded_games_are_reproducible() {
        let a = Game::with_seed(Difficulty::Medium, 2024);
        let b = Game::with_seed(Difficulty::Medium, 2024);
        assert_eq!(a.word(), b.word());
    }
}
