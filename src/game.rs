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
//! * A **match** is [`MATCH_WORDS`] games, drawn at random *without
//!   replacement* from the pack's words, so a match never repeats a word and a
//!   pack bigger than a match is not played out in one sitting. When the drawn
//!   words run out, the match is over and is scored as a [`MatchOutcome`] by
//!   comparing wins against losses.

use std::collections::BTreeSet;

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};

use crate::gallows;
use crate::words;
use crate::words::{Pack, Word};

/// The classic guess budget: six wrong guesses, a whole hangman and no more.
///
/// The Java original had a `setMaximumWrongGuesses` setter that nothing ever
/// called, so its budget was a hard six whatever you were playing. It is no
/// longer a maximum here — [`Difficulty::guess_budget`] hands out up to ten —
/// but it stays as the fallback for a game with no difficulty behind it: a
/// word list loaded from a file is played by the original's rules, because
/// nothing about the file says how hard it is meant to be.
pub const DEFAULT_GUESS_BUDGET: usize = 6;

/// How many words one match is played over.
///
/// Every list used to be played to the end, which was fine while a list was ten
/// words: the match *was* the list. Packs are bigger than that now, and playing
/// thirty words before a match can be scored is a different game — so a match
/// draws this many and leaves the rest of the pack for the next one. Ten is the
/// number the original's lists happened to hold, so a match is exactly as long
/// as it has always been.
///
/// A pack with fewer words than this is played in full, which is what keeps a
/// short list someone typed out by hand playable.
pub const MATCH_WORDS: usize = 10;

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
    /// The rules themselves do not use this — [`Difficulty::guess_budget`] is
    /// difficulty's only grip on the rules — but which list you chose is a
    /// property of the difficulty rather than of the scoreboard, so the number
    /// lives here and [`crate::stats`] does the arithmetic with it.
    ///
    /// The weight climbs as the budget falls, and the weight wins: a clean win
    /// is worth 150 on Easy, 260 on Medium, 360 on Hard and 440 on Insane, so
    /// playing up always pays even though it buys fewer guesses to spend.
    /// `a_clean_win_pays_more_the_harder_the_list` in [`crate::stats`] guards
    /// that, because it is the pair of numbers that could quietly stop being
    /// true if either were tuned on its own.
    pub fn weight(self) -> u32 {
        match self {
            Difficulty::Easy => 1,
            Difficulty::Medium => 2,
            Difficulty::Hard => 3,
            Difficulty::Insane => 4,
        }
    }

    /// How many wrong guesses this difficulty allows before the word is lost.
    ///
    /// This is the one thing difficulty changes about the *rules*; the Java
    /// original changed nothing but the word list. The four numbers are chosen
    /// to stay inside what [`crate::gallows`] can draw: it has
    /// [`PARTS`](crate::gallows::PARTS)`.len()` = 10 body parts, of which
    /// [`CORE_PARTS`](crate::gallows::CORE_PARTS) = 6 make the classic figure,
    /// and `part_count` clamps a budget into `CORE_PARTS..=PARTS.len()`. A
    /// budget anywhere in 6..=10 therefore buys exactly one new body part per
    /// wrong guess — no stage drawing two at once, none repeating — which is
    /// what the four extra parts were added for.
    ///
    /// Insane keeps the classic six, so the hardest setting is the game the
    /// original shipped; each step down the ladder adds slack instead.
    pub fn guess_budget(self) -> usize {
        match self {
            Difficulty::Easy => 10,
            Difficulty::Medium => 8,
            Difficulty::Hard => 7,
            Difficulty::Insane => DEFAULT_GUESS_BUDGET,
        }
    }

    /// The raw contents of this difficulty's word pack.
    ///
    /// The four packs are baked into the binary with `include_str!`, so there
    /// are no data files to ship next to the executable.
    fn raw_pack(self) -> &'static str {
        match self {
            Difficulty::Easy => include_str!("../assets/words/easy.json"),
            Difficulty::Medium => include_str!("../assets/words/med.json"),
            Difficulty::Hard => include_str!("../assets/words/hard.json"),
            Difficulty::Insane => include_str!("../assets/words/insane.json"),
        }
    }

    /// This difficulty's pack, parsed.
    ///
    /// # Panics
    ///
    /// If the pack does not parse — see [`Pack::bundled`], and the test that
    /// stops a broken one reaching a release.
    pub fn pack(self) -> Pack {
        Pack::bundled(self.raw_pack())
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

/// What happened when a hint was asked for.
///
/// The mirror of [`GuessResult`]: one variant per way the call can land, with
/// the two refusals spelled out separately so a UI can say *why* it will not
/// give you one rather than just going quiet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintResult {
    /// This letter was revealed, and it cost one wrong guess.
    Revealed(char),
    /// Refused: one guess is left, and a hint costs one.
    ///
    /// A hint priced in guesses must not be allowed to spend the last one. It
    /// would reveal a letter and lose the word in the same breath — a trap
    /// rather than a choice — and it would force an answer to "is a word
    /// completed by the hint that killed you a win or a loss?". Stopping one
    /// guess short makes the question moot.
    NoGuessToSpare,
    /// Refused: the game (or the whole match) was already over, or the word
    /// holds nothing that is still hidden. Nothing happened, exactly as
    /// [`GuessResult::Ignored`] means for a guess.
    Ignored,
}

/// Everything one call to [`Game::hint`] changed.
///
/// Shaped like [`GuessOutcome`], and for the same reason: `game` and `match_`
/// are `Some` only when the hint ended the game (or the match), so a UI can
/// use them directly as "announce this now" signals. A hint can only ever end
/// a game by *winning* it — see [`HintResult::NoGuessToSpare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HintOutcome {
    /// How the hint itself resolved.
    pub result: HintResult,
    /// Set when the revealed letter completed the word.
    pub game: Option<GameResult>,
    /// Set when that win also ended the match.
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

/// A match in flight, in the detail it takes to pick it up again.
///
/// Everything in here is state that cannot be worked out from anything else:
/// the word on the board and what has been guessed of it, the pool still to
/// play, the budget being played to and the per-match tally. Everything a
/// [`Game`] *derives* — the cells, the display string, the letters still
/// available, which word of the match this is — is deliberately absent, because
/// derived state written down twice is derived state that can come back
/// disagreeing with itself. [`Game::resume`] recomputes all of it, and
/// `total_words` with it.
///
/// The words are stored **by value**, not as indices into a pack: a pack edited
/// between launches, or gone from the disk entirely, then costs nothing — you
/// come back to the word you were actually on, category, clue and all.
///
/// It carries no serde derives, like everything else in this module. The file
/// format for it lives in [`crate::settings`], which is the same split
/// [`Word`] and [`crate::stats::Stats`] have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The difficulty being played, or `None` for a word list of the player's.
    pub difficulty: Option<Difficulty>,
    /// What the pack called itself, or empty for one that did not say.
    pub pack_name: String,
    /// How many playable words the pack held, for the "playing ten of them"
    /// half of the loaded-list summary.
    pub pack_words: usize,
    /// The budget the match is being played to. Re-derived on the way back in
    /// rather than trusted — see [`Game::resume`].
    pub guess_budget: usize,
    /// The word on the board.
    pub current: Word,
    /// Every letter guessed against it, hints included.
    pub guessed: BTreeSet<char>,
    /// How many of those were wrong.
    pub wrong_guesses: usize,
    /// How the current word ended, or `None` while it is still being played.
    /// A word given up on is the reason this cannot simply be derived: it is
    /// lost with guesses still in hand and letters still hidden, which is
    /// indistinguishable from a word in progress.
    pub result: Option<GameResult>,
    /// The words of the match still to be dealt.
    pub remaining_words: Vec<Word>,
    /// Words won so far this match.
    pub words_won: usize,
    /// Words lost so far this match.
    pub words_lost: usize,
}

/// The guess budget a match is played with.
///
/// A bundled difficulty's own [`Difficulty::guess_budget`] is the rule for it,
/// and a pack reached *through* a difficulty does not get to argue: only a pack
/// the player loaded is asked. That one may state a `guess_budget`, and
/// otherwise gets the original's [`DEFAULT_GUESS_BUDGET`], since nothing else
/// about a file says how hard it is meant to be.
///
/// A stated budget is clamped into [`gallows::CORE_PARTS`]`..=`[`gallows::PARTS`]`.len()`
/// — 6..=10 — because outside that range a wrong guess stops being exactly one
/// new body part, which is the constraint [`Difficulty::guess_budget`]'s own
/// four numbers are chosen inside. A pack asking for 3, or for 40, is not
/// refused: it is pulled to the nearest number the gallows can draw.
fn budget_for(difficulty: Option<Difficulty>, pack_budget: Option<usize>) -> usize {
    if let Some(difficulty) = difficulty {
        return difficulty.guess_budget();
    }
    match pack_budget {
        Some(budget) => budget.clamp(gallows::CORE_PARTS, gallows::PARTS.len()),
        None => DEFAULT_GUESS_BUDGET,
    }
}

/// Draw the words one match is played over, at most [`MATCH_WORDS`] of them.
///
/// A pack no bigger than a match is returned whole, in a new order that does
/// not matter — [`Game::deal_word`] draws at random anyway. A bigger one is
/// sampled without replacement, which is what stops a match being the whole
/// pack and what makes the *next* match on the same pack a different ten
/// words.
///
/// It draws from `rng`, so a seeded game picks the same ten every time, exactly
/// as it picks the same order.
fn draw_match(mut words: Vec<Word>, rng: &mut StdRng) -> Vec<Word> {
    let take = words.len().min(MATCH_WORDS);
    let mut drawn = Vec::with_capacity(take);
    for _ in 0..take {
        let index = rng.random_range(0..words.len());
        drawn.push(words.swap_remove(index));
    }
    drawn
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
    /// Words not yet played this match — at most [`MATCH_WORDS`] of them, drawn
    /// from the pack when the match started. A word is removed as it is dealt,
    /// which is what stops a match from repeating a word.
    remaining_words: Vec<Word>,
    /// How many words the match started with, for "word 3 of 10" style UI.
    total_words: usize,
    /// How many playable words the pack held, before the match drew from it.
    /// Only interesting when it is larger than `total_words`, which is the
    /// whole point of a pack bigger than a match.
    pack_words: usize,
    /// What the pack called itself, or empty for one that did not say. The
    /// view shows it in place of its own "Custom word list" wording.
    pack_name: String,
    /// The current word, uppercased, with whatever its pack knows about it.
    current: Word,
    // The Java version stored shared `HangmanCharacter` objects in both the
    // alphabet and the word, so marking a letter guessed mutated both at once.
    // Rust makes that kind of aliasing deliberately awkward, and it is not
    // needed: keeping the word plus the set of guessed letters and *deriving*
    // the display is simpler, cheaper to reason about, and impossible to get
    // out of sync.
    guessed: BTreeSet<char>,
    wrong_guesses: usize,
    /// How many wrong guesses this game allows, from [`budget_for`]. It is
    /// fixed for the whole match — only `reset` moves it — but it is read on
    /// every guess, so it lives next to the counter it is compared against.
    guess_budget: usize,
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
        Self::start(Some(difficulty), difficulty.pack(), rand::make_rng())
    }

    /// Start a match on `difficulty` with a fixed RNG seed.
    ///
    /// Same seed, same word order — which is what makes the tests (and any
    /// "daily puzzle" feature) deterministic.
    pub fn with_seed(difficulty: Difficulty, seed: u64) -> Self {
        Self::start(
            Some(difficulty),
            difficulty.pack(),
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
        Self::from_pack_with_rng(Pack::from_words(words), rand::make_rng())
    }

    /// Start a match on a word pack, e.g. one the player opened from disk.
    ///
    /// The pack-shaped half of [`Game::from_words`]: same rules, but the words
    /// keep their categories and clues, and the pack may state its own guess
    /// budget.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyWordList`] if nothing playable survives the cleanup.
    pub fn from_pack(pack: Pack) -> Result<Self, EmptyWordList> {
        Self::from_pack_with_rng(pack, rand::make_rng())
    }

    /// [`Game::from_pack`] with a fixed RNG seed.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyWordList`] if the pack has no playable words.
    pub fn from_pack_with_seed(pack: Pack, seed: u64) -> Result<Self, EmptyWordList> {
        Self::from_pack_with_rng(pack, StdRng::seed_from_u64(seed))
    }

    /// [`Game::from_words`] with a fixed RNG seed.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyWordList`] if the list has no playable words.
    pub fn from_words_with_seed(words: Vec<String>, seed: u64) -> Result<Self, EmptyWordList> {
        Self::from_pack_with_rng(Pack::from_words(words), StdRng::seed_from_u64(seed))
    }

    /// Pick the match back up from a [`Snapshot`], seeded from the operating
    /// system.
    ///
    /// Returns `None` for a snapshot that does not describe a match anyone
    /// could be in the middle of, which is the whole of the validation: a
    /// saved match is read back out of a file the player is invited to edit,
    /// so it is checked the way a word pack is rather than trusted, and the
    /// caller falls back to dealing a fresh match exactly as
    /// [`crate::settings`] falls back to its defaults. Nothing here is worth
    /// failing loudly over — the cost of a snapshot thrown away is one word.
    ///
    /// What is checked, and why each one matters:
    ///
    /// * The word and the pool go through [`crate::words::sanitize`], the same
    ///   cleanup a pack off disk gets. A current word with nothing guessable
    ///   left in it is the one unrecoverable case, so it is the one `None`
    ///   that is about the word itself.
    /// * **The budget is re-derived, not restored.** It goes back through
    ///   `budget_for`, so a difficulty's budget is its own however the file
    ///   reads, and a loaded pack's is clamped into what the gallows can draw.
    ///   Trusting the saved number would let a hand-edited file play Insane
    ///   with ten guesses — at Insane's weight.
    /// * `result` has to be one the rest of the state could have produced: a
    ///   completed word is a win, a spent budget is a loss, and anything else
    ///   is either still in progress or was given up on. That last pair is
    ///   why `result` is stored rather than derived.
    /// * A resolved word with an empty pool behind it is a match that is
    ///   *over*, which is not a match in flight — [`Game::snapshot`] never
    ///   writes one, and resuming into it would leave a window with no next
    ///   word and no summary.
    ///
    /// The RNG is a fresh one. Word order is already decided — the pool is
    /// restored as it stood — so the only thing a seed still buys is which
    /// letter a hint reveals, and nothing promises that across launches.
    pub fn resume(snapshot: Snapshot) -> Option<Self> {
        Self::resume_with_rng(snapshot, rand::make_rng())
    }

    /// [`Game::resume`] with a fixed RNG seed.
    pub fn resume_with_seed(snapshot: Snapshot, seed: u64) -> Option<Self> {
        Self::resume_with_rng(snapshot, StdRng::seed_from_u64(seed))
    }

    fn resume_with_rng(snapshot: Snapshot, rng: StdRng) -> Option<Self> {
        let current = words::sanitize(vec![snapshot.current]).pop()?;
        let remaining_words = words::sanitize(snapshot.remaining_words);

        let guess_budget = budget_for(snapshot.difficulty, Some(snapshot.guess_budget));
        if snapshot.wrong_guesses > guess_budget {
            return None;
        }

        let guessed: BTreeSet<char> = snapshot
            .guessed
            .into_iter()
            .filter(|letter| letter.is_ascii_alphabetic())
            .map(|letter| letter.to_ascii_uppercase())
            .collect();

        let complete = word_complete(&current.word, &guessed);
        let spent = snapshot.wrong_guesses >= guess_budget;
        // A spent budget is asked about *first*, and it rules the word out as
        // well as the result. The guess that empties the budget ends the game
        // as a loss — `guess` checks the win before it charges, and `hint`
        // refuses at one guess left precisely so it can never be the guess that
        // loses — so nothing can complete a word after that, and a completed
        // word with nothing left to spend is a state no play could reach. Ask
        // `complete` first and a hand edit resumes as a win with a full gallows
        // drawn behind it.
        let plausible = if spent {
            !complete && snapshot.result == Some(GameResult::Lost)
        } else if complete {
            snapshot.result == Some(GameResult::Won)
        } else {
            matches!(snapshot.result, None | Some(GameResult::Lost))
        };
        if !plausible {
            return None;
        }

        // A resolved word is already on the per-match tally — `end_game` counts
        // it in the same breath as it sets the result — so the counter matching
        // that result cannot be zero. Unchecked, the word on the board falls out
        // of the count altogether: `word_number` reports 0, so the panel reads
        // "word 0 of 3"; the end-of-match line is short by one; and
        // `finish_match` compares a tally the word never reached, which can
        // hand the match to the wrong side. It is the same rule as `result`
        // itself — a state the play could not have produced — one field over.
        let counted = match snapshot.result {
            Some(GameResult::Won) => snapshot.words_won > 0,
            Some(GameResult::Lost) => snapshot.words_lost > 0,
            None => true,
        };
        if !counted {
            return None;
        }

        // A resolved word with nothing behind it is a match that is *over*,
        // which is not a match in flight: there is no next word to deal and no
        // summary to show, so there is nothing here to resume into.
        if snapshot.result.is_some() && remaining_words.is_empty() {
            return None;
        }

        // The length of the match, recounted rather than restored. A word
        // still being played is not on the tally yet, so it is the one that
        // has to be counted in on top of it; a resolved one already is.
        // `saturating_add` because these two numbers come out of a file: the
        // sum of two hand-written `usize`s can wrap, and a wrapped total would
        // pass the bound below by being absurdly small rather than absurdly
        // large.
        let unplayed = usize::from(snapshot.result.is_none());
        let total_words = snapshot
            .words_won
            .saturating_add(snapshot.words_lost)
            .saturating_add(remaining_words.len())
            .saturating_add(unplayed);
        // No zero case: the check above leaves at least one word here either
        // way. A match longer than a match is a file that has been edited into
        // something `draw_match` could never have dealt.
        if total_words > MATCH_WORDS {
            return None;
        }

        Some(Game {
            difficulty: snapshot.difficulty,
            // A pack cannot have held fewer words than the match drew from it;
            // a file saying otherwise is only ever wrong about a sentence in a
            // notification, so it is corrected rather than refused.
            pack_words: snapshot.pack_words.max(total_words),
            pack_name: snapshot.pack_name.trim().to_owned(),
            remaining_words,
            total_words,
            current,
            guessed,
            wrong_guesses: snapshot.wrong_guesses,
            guess_budget,
            result: snapshot.result,
            words_won: snapshot.words_won,
            words_lost: snapshot.words_lost,
            // Always `None`: a match with an outcome is over, and the checks
            // above have already refused every snapshot that describes one.
            match_outcome: None,
            rng,
        })
    }

    fn from_pack_with_rng(pack: Pack, rng: StdRng) -> Result<Self, EmptyWordList> {
        let budget = budget_for(None, pack.guess_budget);
        let name = pack.display_name().to_owned();
        let words = pack.into_words();
        if words.is_empty() {
            return Err(EmptyWordList);
        }
        Ok(Self::start_with(None, budget, name, words, rng))
    }

    /// Shared constructor body for a pack: sanitize it, ask it for its budget,
    /// and start a match on it.
    fn start(difficulty: Option<Difficulty>, pack: Pack, rng: StdRng) -> Self {
        let budget = budget_for(difficulty, pack.guess_budget);
        let name = pack.display_name().to_owned();
        Self::start_with(difficulty, budget, name, pack.into_words(), rng)
    }

    /// Shared constructor body. `words` must already be sanitized and non-empty
    /// for the game to be playable; an empty list yields an immediately-over
    /// match, which only the bundled packs could never produce.
    fn start_with(
        difficulty: Option<Difficulty>,
        guess_budget: usize,
        pack_name: String,
        words: Vec<Word>,
        mut rng: StdRng,
    ) -> Self {
        let pack_words = words.len();
        let words = draw_match(words, &mut rng);
        let total_words = words.len();
        let mut game = Game {
            difficulty,
            remaining_words: words,
            total_words,
            pack_words,
            pack_name,
            current: Word::bare(String::new()),
            guessed: BTreeSet::new(),
            wrong_guesses: 0,
            guess_budget,
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
        self.current = self.remaining_words.swap_remove(index);
        self.guessed.clear();
        self.wrong_guesses = 0;
        self.result = None;
        true
    }

    /// The word being guessed, uppercased.
    fn text(&self) -> &str {
        &self.current.word
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

        if self.text().contains(letter) {
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
            if self.wrong_guesses >= self.guess_budget {
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

    /// Reveal one letter of the word, at the price of one wrong guess.
    ///
    /// The letter goes into the guessed set exactly as a correct guess would —
    /// so the word row, the keyboard and the win check all see it as one — and
    /// `wrong_guesses` goes up by one, so the wrong-guess counter, the pips
    /// and the gallows all charge for it without any of them knowing that
    /// hints exist. Pricing a hint in guesses is what keeps it that cheap:
    /// [`crate::stats`] already pays a solved word by the budget it left
    /// unspent, so a hint needs no penalty of its own, and
    /// [`Difficulty::guess_budget`] already sizes the price per difficulty —
    /// a tenth of Easy, a sixth of Insane.
    ///
    /// *Which* letter is drawn from the game's own RNG, so a seeded game
    /// ([`Game::with_seed`], [`Game::from_words_with_seed`]) hints
    /// reproducibly.
    ///
    /// Changes nothing and reveals nothing whenever [`Game::can_hint`] is
    /// false; the [`HintResult`] says which of the reasons it was.
    pub fn hint(&mut self) -> HintOutcome {
        let refused = |result| HintOutcome {
            result,
            game: None,
            match_: None,
        };

        if self.is_game_over() {
            return refused(HintResult::Ignored);
        }
        // Deliberately `<= 1` rather than `== 0`: see `HintResult::NoGuessToSpare`.
        if self.remaining_guesses() <= 1 {
            return refused(HintResult::NoGuessToSpare);
        }
        let hidden = self.hidden_letters();
        if hidden.is_empty() {
            // Unreachable while a game is live — revealing the last hidden
            // letter wins the word, so a game still in progress always has one
            // — but the draw below needs a non-empty pool, and a guard says
            // that out loud instead of leaving an index to panic on.
            return refused(HintResult::Ignored);
        }

        let letter = hidden[self.rng.random_range(0..hidden.len())];
        self.guessed.insert(letter);
        self.wrong_guesses += 1;

        // The win is checked first, and the loss is not checked at all: the
        // budget cannot run out here, because `remaining_guesses` was at least
        // two before the increment above. That is the whole point of refusing
        // at one guess left.
        if self.is_word_complete() {
            let ended = self.end_game(GameResult::Won, GuessResult::Correct);
            HintOutcome {
                result: HintResult::Revealed(letter),
                game: ended.game,
                match_: ended.match_,
            }
        } else {
            HintOutcome {
                result: HintResult::Revealed(letter),
                game: None,
                match_: None,
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
    ///
    /// Returns `false` (and changes nothing) when `difficulty` is the one
    /// already being played and the match is still running: the UI's pills
    /// are always clickable, including the selected one, and throwing away
    /// the word in hand for a click that picked no new difficulty is not what
    /// anyone means by it. Once the match *is* over that same click is the
    /// only way to play the list again — the footer says so in as many words
    /// — so it restarts as usual and returns `true`. A custom word list has
    /// no difficulty at all, so every pill restarts out of one.
    pub fn set_difficulty(&mut self, difficulty: Difficulty) -> bool {
        if !self.would_switch_to(difficulty) {
            return false;
        }
        self.reset(Some(difficulty), difficulty.pack());
        true
    }

    /// Would [`Game::set_difficulty`] actually do anything for `difficulty`?
    ///
    /// The predicate behind that method's `false`, split out so the UI can ask
    /// *before* it acts: a switch throws the current word away, and whether
    /// that word has to be paid for depends on whether the switch happens at
    /// all. Keeping the rule in one place is the point — a copy of it in the
    /// view would be one to get out of step.
    pub fn would_switch_to(&self, difficulty: Difficulty) -> bool {
        self.difficulty != Some(difficulty) || self.is_match_over()
    }

    /// Is there a started-but-unfinished word here — one that walking away
    /// would cost you?
    ///
    /// True once a guess or a hint has landed on a word that is not yet won or
    /// lost. This is the line between abandoning a word and simply choosing
    /// where to start: picking a difficulty before you have played a letter
    /// costs nothing, and every letter after that is a word in progress. Both
    /// guesses and hints count, because `hint` records the letter it reveals in
    /// the same set `guess` does.
    ///
    /// The UI charges a loss for one of these when the word is thrown away —
    /// by a difficulty switch or a new word list — exactly as [`Game::give_up`]
    /// does. Without it the streak has a free escape hatch, which is the one
    /// thing the streak is not supposed to have.
    pub fn has_word_to_lose(&self) -> bool {
        !self.is_game_over() && !self.guessed.is_empty()
    }

    /// Abandon the current match and start a fresh one on a custom word list.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyWordList`] if the list has no playable words; the current
    /// match is left untouched in that case.
    pub fn set_word_list(&mut self, words: Vec<String>) -> Result<(), EmptyWordList> {
        self.set_pack(Pack::from_words(words))
    }

    /// Abandon the current match and start a fresh one on a word pack.
    ///
    /// The pack-shaped half of [`Game::set_word_list`], and the one the view
    /// actually calls: the words keep their categories and clues, and the pack
    /// may state its own guess budget.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyWordList`] if the pack has no playable words; the current
    /// match is left untouched in that case, which is what lets the view leave
    /// the word on the board and charge nothing for a file that would not load.
    pub fn set_pack(&mut self, pack: Pack) -> Result<(), EmptyWordList> {
        let budget = budget_for(None, pack.guess_budget);
        let name = pack.display_name().to_owned();
        let words = pack.into_words();
        if words.is_empty() {
            return Err(EmptyWordList);
        }
        self.reset_with(None, budget, name, words);
        Ok(())
    }

    fn reset(&mut self, difficulty: Option<Difficulty>, pack: Pack) {
        let budget = budget_for(difficulty, pack.guess_budget);
        let name = pack.display_name().to_owned();
        self.reset_with(difficulty, budget, name, pack.into_words());
    }

    fn reset_with(
        &mut self,
        difficulty: Option<Difficulty>,
        guess_budget: usize,
        pack_name: String,
        words: Vec<Word>,
    ) {
        self.pack_words = words.len();
        self.pack_name = pack_name;
        let words = draw_match(words, &mut self.rng);
        self.difficulty = difficulty;
        self.guess_budget = guess_budget;
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
        self.text()
    }

    /// What kind of thing the current word is — `"Food"`, `"College life"` —
    /// or `None` for a pack that does not say.
    ///
    /// Safe to show *during* play: a category names the neighbourhood, not the
    /// word. A clue is the same, and the answer itself never is.
    pub fn category(&self) -> Option<&str> {
        self.current.category.as_deref()
    }

    /// A sentence about what the current word means, or `None` for a pack that
    /// does not say.
    ///
    /// Unlike [`Game::hint`] this costs nothing and this module does not track
    /// whether it has been read: it reveals no letter, so there is no rule
    /// here for it to be part of.
    pub fn clue(&self) -> Option<&str> {
        self.current.clue.as_deref()
    }

    /// The current word as display cells, one per character.
    ///
    /// Once the game is over every cell is revealed, so the UI can render the
    /// answer without special-casing anything.
    pub fn cells(&self) -> Vec<Cell> {
        let over = self.is_game_over();
        self.text()
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

    /// How many wrong guesses have been made this game
    /// (0..=[`guess_budget`](Game::guess_budget)).
    ///
    /// This doubles as the index of the gallows drawing stage.
    pub fn wrong_guesses(&self) -> usize {
        self.wrong_guesses
    }

    /// How many wrong guesses this game allows in total.
    ///
    /// [`Difficulty::guess_budget`] for a bundled pack, or what `budget_for`
    /// works out for a loaded one. The UI needs it
    /// for the wrong-guess counter, for the row of pips and for the gallows,
    /// which spreads its body parts over whatever budget it is handed.
    pub fn guess_budget(&self) -> usize {
        self.guess_budget
    }

    /// How many wrong guesses are still affordable.
    pub fn remaining_guesses(&self) -> usize {
        self.guess_budget.saturating_sub(self.wrong_guesses)
    }

    /// Whether [`Game::hint`] would actually reveal something.
    ///
    /// The UI disables its Hint button on this, so the refusals inside
    /// [`Game::hint`] are a backstop rather than the usual path. Note the
    /// middle term: a hint is unavailable one guess *before* the last, not on
    /// it.
    pub fn can_hint(&self) -> bool {
        !self.is_game_over() && self.remaining_guesses() > 1 && !self.hidden_letters().is_empty()
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

    /// The match in flight, or `None` when there is nothing to come back to.
    ///
    /// `None` means the match is over: every word of it has been played and
    /// the summary is on screen, so the next launch has a fresh match to deal
    /// rather than a finished one to restore. Every other state is worth
    /// saving, including a word that has just been resolved and one that has
    /// not been guessed at yet — "where you were" is one rule with no
    /// exceptions in it, and the alternative hands back the rest of the match
    /// as the price of closing the window on a word you had just won.
    ///
    /// See [`Game::resume`] for the way back, and [`Snapshot`] for what is
    /// left out of one.
    pub fn snapshot(&self) -> Option<Snapshot> {
        if self.is_match_over() {
            return None;
        }
        Some(Snapshot {
            difficulty: self.difficulty,
            pack_name: self.pack_name.clone(),
            pack_words: self.pack_words,
            guess_budget: self.guess_budget,
            current: self.current.clone(),
            guessed: self.guessed.clone(),
            wrong_guesses: self.wrong_guesses,
            result: self.result,
            remaining_words: self.remaining_words.clone(),
            words_won: self.words_won,
            words_lost: self.words_lost,
        })
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

    /// How many playable words the pack held.
    ///
    /// At least [`Game::total_words`], and more than it whenever the pack is
    /// bigger than a match — which every bundled pack now is. The view says so
    /// when a list is loaded, because "loaded two hundred words" and "playing
    /// ten of them" are both true and only one of them is obvious.
    pub fn pack_words(&self) -> usize {
        self.pack_words
    }

    /// What the pack called itself, or `None` for one that did not say.
    ///
    /// Empty for a plain `.txt`, which has nowhere to put a name, so the view
    /// has its own wording to fall back on.
    ///
    /// Testing emptiness is enough because the name arrives through
    /// [`Pack::display_name`] and is trimmed before it is ever stored — a pack
    /// calling itself `"   "` is stored as `""` and answered `None` here. Keep
    /// that true at the call sites rather than trimming again in this method:
    /// a blank name is meant to be indistinguishable from an absent one by the
    /// time anything reads it.
    pub fn pack_name(&self) -> Option<&str> {
        Some(self.pack_name.as_str()).filter(|name| !name.is_empty())
    }

    /// Which word of the match is on screen, 1-based — the "3" in "word 3 of 10".
    pub fn word_number(&self) -> usize {
        self.total_words - self.remaining_words.len()
    }

    /// The distinct guessable letters of the word that are still hidden — the
    /// pool a hint is drawn from.
    ///
    /// Sorted and de-duplicated, so which letter a seeded game hints depends
    /// on the seed alone and not on where the letters happen to sit in the
    /// word, and so a letter that appears three times is no likelier to come
    /// up than one that appears once.
    fn hidden_letters(&self) -> Vec<char> {
        let mut letters: Vec<char> = self
            .text()
            .chars()
            .filter(|c| c.is_ascii_alphabetic() && !self.guessed.contains(c))
            .collect();
        letters.sort_unstable();
        letters.dedup();
        letters
    }

    /// Whether every guessable character of the current word has been guessed.
    fn is_word_complete(&self) -> bool {
        word_complete(self.text(), &self.guessed)
    }
}

/// Whether every guessable character of `word` is in `guessed`.
///
/// Free rather than a method because [`Game::resume`] has to ask it of a word
/// and a letter set that are not a `Game` yet — and asking the same question
/// twice, two ways, is how the win check and the resume check would come to
/// disagree about what a finished word is.
fn word_complete(word: &str, guessed: &BTreeSet<char>) -> bool {
    word.chars()
        .filter(|c| c.is_ascii_alphabetic())
        .all(|c| guessed.contains(&c))
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

    /// Burn the whole guess budget on letters that are not in the current word.
    fn lose_current_game(game: &mut Game) {
        for letter in wrong_letters(game) {
            game.guess(letter);
        }
    }

    /// As many letters as the budget allows, none of them in the current word.
    ///
    /// The longest bundled word has eleven distinct letters, so there are
    /// always at least fifteen to choose from — comfortably more than the ten
    /// the most generous budget spends.
    fn wrong_letters(game: &Game) -> Vec<char> {
        let word = game.word().to_string();
        ('A'..='Z')
            .filter(|c| !word.contains(*c))
            .take(game.guess_budget())
            .collect()
    }

    /// A difficulty's pack as plain uppercase strings, which is what most of
    /// these tests want to compare a dealt word against.
    fn pack_words(difficulty: Difficulty) -> Vec<String> {
        difficulty
            .pack()
            .into_words()
            .into_iter()
            .map(|word| word.word)
            .collect()
    }

    #[test]
    fn every_bundled_pack_parses() {
        // `Difficulty::pack` panics on a pack that does not, which is the rule
        // for a file we ship rather than one the player wrote. This is the test
        // that stops a broken one reaching a release, so it is deliberately
        // cheap and deliberately here.
        for difficulty in Difficulty::ALL {
            let pack = difficulty.pack();
            assert_eq!(pack.name, difficulty.label(), "{}", difficulty.label());
            assert_eq!(pack.guess_budget, None, "{}", difficulty.label());
        }
    }

    #[test]
    fn every_bundled_pack_can_fill_a_whole_match() {
        // A pack shorter than `MATCH_WORDS` is legal — a list someone typed out
        // by hand is often shorter — but a *bundled* one being short would mean
        // a match that quietly ends early on that difficulty and nowhere else.
        for difficulty in Difficulty::ALL {
            assert!(
                pack_words(difficulty).len() >= MATCH_WORDS,
                "{} has only {} words",
                difficulty.label(),
                pack_words(difficulty).len()
            );
        }
    }

    #[test]
    fn a_match_is_drawn_from_the_pack_rather_than_being_all_of_it() {
        // The point of a pack bigger than a match: the same difficulty played
        // twice is not the same words in a different order. Two seeds rather
        // than two matches, so the assertion is about the draw and not about
        // what the first match happened to remove.
        let pack = pack_words(Difficulty::Easy);
        assert!(
            pack.len() > MATCH_WORDS,
            "this test only says anything while the pack is bigger than a match"
        );
        let drawn = |seed| {
            let mut game = Game::with_seed(Difficulty::Easy, seed);
            let mut words = vec![game.word().to_string()];
            while !game.is_match_over() {
                game.give_up();
                if game.new_game() {
                    words.push(game.word().to_string());
                }
            }
            words.sort();
            words
        };
        assert_ne!(drawn(1), drawn(2), "two seeds drew the same ten words");
    }

    #[test]
    fn no_bundled_clue_gives_its_own_word_away() {
        // The standing rule from the abandon-tooltip near-miss: nothing shown
        // *during* play may leak the answer, and a category and a clue are both
        // shown during play. A clue naming its own word is the obvious way to
        // break it, and a clue built on the same stem — "immeable" under
        // `Immeability` — is the way it actually happened while these were
        // being written. Six letters is where that stops being a coincidence
        // and starts being most of a short word.
        let letters_only = |text: &str| -> String {
            text.chars()
                .filter(char::is_ascii_alphabetic)
                .collect::<String>()
                .to_ascii_lowercase()
        };
        for difficulty in Difficulty::ALL {
            for word in difficulty.pack().words {
                let needle = letters_only(&word.word);
                let haystack = letters_only(&format!(
                    "{}{}",
                    word.category.as_deref().unwrap_or_default(),
                    word.clue.as_deref().unwrap_or_default()
                ));
                assert!(
                    !haystack.contains(&needle),
                    "{}'s clue names the word itself",
                    word.word
                );
                if needle.len() >= 6 {
                    assert!(
                        !haystack.contains(&needle[..6]),
                        "{}'s clue shares its first six letters",
                        word.word
                    );
                }
            }
        }
    }

    #[test]
    fn every_bundled_word_carries_a_category_and_a_clue() {
        // Both are optional in the *format*, because a list typed out by hand
        // has no reason to fill them in. The packs we ship are held to more
        // than that: a word with no clue is a `Clue` button that does nothing
        // for reasons the player cannot see.
        for difficulty in Difficulty::ALL {
            for word in difficulty.pack().words {
                assert!(word.category.is_some(), "{} has no category", word.word);
                assert!(word.clue.is_some(), "{} has no clue", word.word);
            }
        }
    }

    #[test]
    fn no_bundled_pack_repeats_a_word() {
        for difficulty in Difficulty::ALL {
            let mut words = pack_words(difficulty);
            let before = words.len();
            words.sort();
            words.dedup();
            assert_eq!(words.len(), before, "{} repeats a word", difficulty.label());
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

    // --------------------------------------------------- the guess budget

    #[test]
    fn each_difficulty_hands_out_its_own_guess_budget() {
        assert_eq!(
            Difficulty::ALL.map(Difficulty::guess_budget),
            [10, 8, 7, 6],
            "easy is the most forgiving; insane is the original's six"
        );
    }

    #[test]
    fn the_hardest_difficulty_is_the_original_game() {
        assert_eq!(Difficulty::Insane.guess_budget(), DEFAULT_GUESS_BUDGET);
        for difficulty in Difficulty::ALL {
            assert!(
                difficulty.guess_budget() >= Difficulty::Insane.guess_budget(),
                "{} is stingier than Insane",
                difficulty.label()
            );
        }
    }

    #[test]
    fn every_budget_buys_exactly_one_body_part_per_wrong_guess() {
        // Why these four numbers and not any others: `crate::gallows` has ten
        // parts and clamps a budget into `CORE_PARTS..=PARTS.len()`, so only a
        // budget in 6..=10 draws one new part per guess. Outside it the
        // drawing still works, but a stage would draw two parts or repeat one.
        use crate::gallows::{CORE_PARTS, PARTS, parts_drawn};
        for difficulty in Difficulty::ALL {
            let budget = difficulty.guess_budget();
            let label = difficulty.label();
            assert!(
                (CORE_PARTS..=PARTS.len()).contains(&budget),
                "{label}'s budget of {budget} is outside what the gallows draws"
            );
            for wrong in 0..=budget {
                assert_eq!(parts_drawn(budget, wrong), wrong, "{label} at {wrong}");
            }
        }
    }

    #[test]
    fn a_game_lasts_exactly_its_difficultys_budget_and_not_a_guess_less() {
        for difficulty in Difficulty::ALL {
            let label = difficulty.label();
            let budget = difficulty.guess_budget();
            let mut game = Game::with_seed(difficulty, 7);
            assert_eq!(game.guess_budget(), budget, "{label}");
            assert_eq!(game.remaining_guesses(), budget, "{label}");

            for (spent, letter) in wrong_letters(&game).into_iter().enumerate() {
                assert!(!game.is_game_over(), "{label} was over after {spent}");
                let outcome = game.guess(letter);
                assert_eq!(outcome.result, GuessResult::Wrong, "{label}");
                assert_eq!(game.wrong_guesses(), spent + 1, "{label}");
                assert_eq!(game.remaining_guesses(), budget - spent - 1, "{label}");
            }

            assert!(game.is_game_over(), "{label} survived its whole budget");
            assert_eq!(game.game_result(), Some(GameResult::Lost), "{label}");
            assert_eq!(game.remaining_guesses(), 0, "{label}");
        }
    }

    #[test]
    fn a_word_list_of_your_own_is_played_by_the_originals_rules() {
        let game = game_with_word("BANANA");
        assert_eq!(game.difficulty(), None);
        assert_eq!(game.guess_budget(), DEFAULT_GUESS_BUDGET);
        assert_eq!(game.remaining_guesses(), DEFAULT_GUESS_BUDGET);
    }

    #[test]
    fn changing_difficulty_mid_match_changes_the_budget_with_it() {
        let mut game = Game::with_seed(Difficulty::Insane, 5);
        assert_eq!(game.guess_budget(), 6);

        // Half way through a word, with a wrong guess already spent.
        let spend = wrong_letters(&game)[0];
        game.guess(spend);
        assert_eq!(game.remaining_guesses(), 5);

        game.set_difficulty(Difficulty::Easy);
        assert_eq!(game.guess_budget(), 10);
        assert_eq!(game.remaining_guesses(), 10, "the new word starts fresh");

        game.set_difficulty(Difficulty::Hard);
        assert_eq!(game.guess_budget(), 7);

        // And a word list of the player's own gives the classic six back.
        game.set_word_list(vec!["BANANA".to_string()])
            .expect("word list is not empty");
        assert_eq!(game.guess_budget(), DEFAULT_GUESS_BUDGET);
    }

    #[test]
    fn a_rejected_word_list_leaves_the_budget_alone() {
        let mut game = Game::with_seed(Difficulty::Easy, 2);
        assert!(game.set_word_list(vec!["   ".to_string()]).is_err());
        assert_eq!(game.guess_budget(), Difficulty::Easy.guess_budget());
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
        assert_eq!(game.remaining_guesses(), DEFAULT_GUESS_BUDGET - 1);
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
        assert_eq!(game.wrong_guesses(), DEFAULT_GUESS_BUDGET);
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
    fn a_pack_that_names_itself_nothing_is_a_pack_with_no_name() {
        // The same rule as a blank category or clue, one field over: blank and
        // absent have to be the same state by the time anything reads it, or
        // the view's fallback wording never gets its turn and the title bar
        // shows three spaces.
        for name in ["", "   ", "\t\n"] {
            let pack = Pack::parse(&format!(
                r#"{{ "name": {name:?}, "words": [{{ "word": "Alpha" }}] }}"#
            ))
            .expect("valid JSON");
            let game = Game::from_pack(pack).expect("one word is not an empty pack");
            assert_eq!(game.pack_name(), None, "{name:?} was taken as a name");
        }
        // And a name with something in it survives, trimmed.
        let pack = Pack::parse(r#"{ "name": "  Pets  ", "words": [{ "word": "Alpha" }] }"#)
            .expect("valid JSON");
        let game = Game::from_pack(pack).expect("one word is not an empty pack");
        assert_eq!(game.pack_name(), Some("Pets"));
    }

    #[test]
    fn a_match_never_repeats_a_word() {
        let mut game = Game::with_seed(Difficulty::Easy, 42);
        let pack = pack_words(Difficulty::Easy);
        let mut seen = vec![game.word().to_string()];
        while !game.is_match_over() {
            game.give_up();
            if game.new_game() {
                seen.push(game.word().to_string());
            }
        }
        assert_eq!(seen.len(), MATCH_WORDS);
        let mut sorted = seen.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), seen.len(), "a word was dealt twice: {seen:?}");
        for word in &seen {
            assert!(pack.contains(word), "{word} is not in the pack");
        }
    }

    #[test]
    fn the_same_seed_draws_the_same_match() {
        // The other half of the above: the draw has to be reproducible, or a
        // seeded game stops being seeded. All ten words rather than the first,
        // because the first is dealt by `draw_match` and the other nine by the
        // `deal_word` sequence after it — a regression in the second half
        // would sail past a check on the first word alone.
        let words = |seed| {
            let mut game = Game::with_seed(Difficulty::Easy, seed);
            let mut seen = vec![game.word().to_string()];
            while !game.is_match_over() {
                game.give_up();
                if game.new_game() {
                    seen.push(game.word().to_string());
                }
            }
            seen
        };
        let first = words(7);
        assert_eq!(first.len(), MATCH_WORDS);
        assert_eq!(first, words(7));
        // And not by dealing the same match to everyone: a seed that produced
        // the same ten words as any other seed would satisfy the line above
        // while proving nothing about the seed.
        assert_ne!(first, words(8));
    }

    #[test]
    fn a_pack_smaller_than_a_match_is_played_in_full() {
        let game =
            Game::from_words(vec!["ALPHA".into(), "OMEGA".into()]).expect("two words is not empty");
        assert_eq!(game.total_words(), 2);
    }

    #[test]
    fn a_loaded_pack_may_ask_for_its_own_guess_budget() {
        let pack = Pack::parse(r#"{ "guess_budget": 9, "words": [{ "word": "Alpha" }] }"#)
            .expect("valid JSON");
        let game = Game::from_pack(pack).expect("one word is not empty");
        assert_eq!(game.guess_budget(), 9);
    }

    #[test]
    fn a_budget_the_gallows_cannot_draw_is_pulled_to_one_it_can() {
        // Not refused, pulled: a pack asking for 2 or for 40 is asking for a
        // wrong guess that is not exactly one new body part, and the nearest
        // playable answer is better than an error the player cannot act on.
        for (asked, expected) in [
            (0, gallows::CORE_PARTS),
            (2, gallows::CORE_PARTS),
            (40, gallows::PARTS.len()),
        ] {
            let pack = Pack {
                guess_budget: Some(asked),
                ..Pack::from_words(vec!["ALPHA".into()])
            };
            let game = Game::from_pack(pack).expect("one word is not empty");
            assert_eq!(game.guess_budget(), expected, "asked for {asked}");
        }
    }

    #[test]
    fn a_bundled_difficulty_does_not_let_a_pack_argue_about_the_budget() {
        // `budget_for` asks the pack only when there is no difficulty. The
        // bundled packs state no budget, so this is guarding the rule rather
        // than any file we ship — the four numbers on `Difficulty` are the
        // difficulty ladder, and a pack reached through a pill cannot bend it.
        assert_eq!(
            budget_for(Some(Difficulty::Insane), Some(10)),
            Difficulty::Insane.guess_budget()
        );
        assert_eq!(budget_for(None, Some(10)), 10);
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
    fn an_untouched_word_is_free_to_walk_away_from() {
        // Picking a difficulty before you have played a letter is choosing
        // where to start, not abandoning anything.
        let game = Game::with_seed(Difficulty::Insane, 3);
        assert!(!game.has_word_to_lose());
    }

    #[test]
    fn one_guess_is_enough_to_make_a_word_worth_losing() {
        let mut game = Game::with_seed(Difficulty::Insane, 3);
        let spend = wrong_letters(&game)[0];
        game.guess(spend);
        assert!(game.has_word_to_lose());

        // A right guess counts just the same as a wrong one.
        let mut game = Game::with_seed(Difficulty::Insane, 3);
        let hit = game.word().chars().next().expect("the word is not empty");
        assert_eq!(game.guess(hit).result, GuessResult::Correct);
        assert!(game.has_word_to_lose());
    }

    #[test]
    fn a_hint_makes_a_word_worth_losing_too() {
        // `hint` records the letter it reveals in the same set `guess` does, so
        // a word you have only ever hinted at is still a word in progress.
        let mut game = Game::with_seed(Difficulty::Easy, 4);
        assert!(!game.has_word_to_lose());
        assert!(matches!(game.hint().result, HintResult::Revealed(_)));
        assert!(game.has_word_to_lose());
    }

    #[test]
    fn a_finished_word_is_not_a_word_to_lose() {
        // Won and lost words are already scored; walking away from one after
        // the fact must not charge for it twice.
        let mut game = game_with_word("BANANA");
        win_current_game(&mut game);
        assert_eq!(game.game_result(), Some(GameResult::Won));
        assert!(!game.has_word_to_lose());

        let mut game = Game::with_seed(Difficulty::Insane, 3);
        lose_current_game(&mut game);
        assert_eq!(game.game_result(), Some(GameResult::Lost));
        assert!(!game.has_word_to_lose());
    }

    #[test]
    fn would_switch_to_answers_for_set_difficulty() {
        // The two have to agree, because the UI asks the predicate and then
        // acts on the method.
        let mut game = Game::with_seed(Difficulty::Hard, 6);
        assert!(!game.would_switch_to(Difficulty::Hard));
        for difficulty in Difficulty::ALL {
            let mut copy = Game::with_seed(Difficulty::Hard, 6);
            assert_eq!(
                copy.would_switch_to(difficulty),
                copy.set_difficulty(difficulty),
                "{} disagreed",
                difficulty.label()
            );
        }

        // Over: every pill switches, the one just played included.
        while game.give_up().is_none() {
            assert!(game.new_game(), "the match still had words left");
        }
        for difficulty in Difficulty::ALL {
            assert!(
                game.would_switch_to(difficulty),
                "{} should restart a finished match",
                difficulty.label()
            );
        }

        // A custom list belongs to no difficulty, so all four switch.
        let game = game_with_word("BANANA");
        for difficulty in Difficulty::ALL {
            assert!(game.would_switch_to(difficulty), "{}", difficulty.label());
        }
    }

    #[test]
    fn reselecting_the_current_difficulty_leaves_the_game_alone() {
        // The pills fire even when they are already selected, so the one in
        // play has to be a no-op — otherwise a stray click on the difficulty
        // you are already playing costs you the word in hand.
        let mut game = Game::with_seed(Difficulty::Insane, 3);
        let word = game.word().to_string();
        let spend = wrong_letters(&game)[0];
        game.guess(spend);
        assert_eq!(game.wrong_guesses(), 1);

        assert!(
            !game.set_difficulty(Difficulty::Insane),
            "re-selecting the difficulty in play should refuse"
        );
        assert_eq!(game.word(), word, "the word in hand survived");
        assert_eq!(game.wrong_guesses(), 1, "the wrong guess survived");
        assert!(game.guessed_letters().contains(&spend));
        assert_eq!(game.word_number(), 1);
    }

    #[test]
    fn picking_a_different_difficulty_still_starts_a_new_game() {
        let mut game = Game::with_seed(Difficulty::Insane, 3);
        let spend = wrong_letters(&game)[0];
        game.guess(spend);

        assert!(
            game.set_difficulty(Difficulty::Easy),
            "a different difficulty should restart"
        );
        assert_eq!(game.difficulty(), Some(Difficulty::Easy));
        assert_eq!(game.wrong_guesses(), 0);
        assert!(game.guessed_letters().is_empty());
        assert_eq!(game.guess_budget(), Difficulty::Easy.guess_budget());
        assert!(pack_words(Difficulty::Easy).contains(&game.word().to_string()));
    }

    #[test]
    fn reselecting_the_current_difficulty_restarts_once_the_match_is_over() {
        // The footer at the end of a match reads "Pick a difficulty to start a
        // new match", and the difficulty just played is one of the four on
        // offer. The no-op above must not eat that click.
        let mut game = Game::with_seed(Difficulty::Insane, 3);
        while game.give_up().is_none() {
            assert!(game.new_game(), "the match still had words left");
        }
        assert!(game.is_match_over());

        assert!(
            game.set_difficulty(Difficulty::Insane),
            "the same difficulty should replay a finished match"
        );
        assert!(!game.is_match_over());
        assert_eq!((game.words_won(), game.words_lost()), (0, 0));
        assert_eq!(game.word_number(), 1);
    }

    #[test]
    fn every_pill_restarts_out_of_a_custom_word_list() {
        // A loaded list has no difficulty, so none of the four is "current"
        // and all four have to restart.
        for difficulty in Difficulty::ALL {
            let mut game = game_with_word("BANANA");
            assert_eq!(game.difficulty(), None);
            assert!(
                game.set_difficulty(difficulty),
                "{} should restart out of a custom list",
                difficulty.label()
            );
            assert_eq!(game.difficulty(), Some(difficulty));
        }
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
        assert!(pack_words(Difficulty::Insane).contains(&game.word().to_string()));
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

    // --------------------------------------------------------------- hints

    #[test]
    fn a_hint_reveals_exactly_one_new_letter() {
        let mut game = game_with_word("ALPHABET");
        game.guess('A');
        assert_eq!(game.display(), "A___A___");

        let HintResult::Revealed(letter) = game.hint().result else {
            panic!("a fresh game should have a hint to give");
        };
        assert!(game.word().contains(letter), "{letter} is not in the word");
        assert_ne!(letter, 'A', "a hint may not re-reveal a guessed letter");
        assert_eq!(
            game.guessed_letters().len(),
            2,
            "a hint adds one letter and no more"
        );
        assert!(game.guessed_letters().contains(&letter));
        assert!(game.display().contains(letter));
    }

    #[test]
    fn a_hint_costs_exactly_one_wrong_guess() {
        let mut game = game_with_word("ALPHABET");
        let budget = game.guess_budget();
        assert_eq!(game.wrong_guesses(), 0);

        assert!(matches!(game.hint().result, HintResult::Revealed(_)));
        assert_eq!(game.wrong_guesses(), 1);
        assert_eq!(game.remaining_guesses(), budget - 1);
        assert!(!game.is_game_over(), "one hint cannot end a fresh game");
    }

    #[test]
    fn a_hint_ends_neither_the_game_nor_the_match() {
        let mut game = game_with_word("ALPHABET");
        assert!(matches!(game.hint().result, HintResult::Revealed(_)));
        assert_eq!(game.words_won(), 0);
        assert_eq!(game.words_lost(), 0);
        assert_eq!(game.match_outcome(), None);
        assert_eq!(game.game_result(), None);
    }

    #[test]
    fn a_seeded_game_hints_the_same_letter_twice() {
        let hint_of = |seed| {
            let mut game = Game::from_words_with_seed(vec!["ALPHABET".into()], seed)
                .expect("word list is not empty");
            game.hint().result
        };
        assert_eq!(hint_of(7), hint_of(7));
    }

    #[test]
    fn a_hint_is_refused_with_one_guess_left() {
        let mut game = game_with_word("ALPHABET");
        // Everything but the last guess, so `remaining_guesses` is exactly 1.
        for letter in wrong_letters(&game)
            .into_iter()
            .take(game.guess_budget() - 1)
        {
            game.guess(letter);
        }
        assert_eq!(game.remaining_guesses(), 1);
        assert!(!game.is_game_over());

        let guessed = game.guessed_letters().len();
        let wrong = game.wrong_guesses();
        assert!(!game.can_hint(), "the last guess is not a hint to spend");
        assert_eq!(game.hint().result, HintResult::NoGuessToSpare);
        assert_eq!(game.guessed_letters().len(), guessed, "nothing revealed");
        assert_eq!(game.wrong_guesses(), wrong, "and nothing charged");
        assert!(!game.is_game_over(), "a refused hint cannot lose the word");
    }

    #[test]
    fn a_hint_is_available_right_up_to_that_point() {
        let mut game = game_with_word("ALPHABET");
        for letter in wrong_letters(&game)
            .into_iter()
            .take(game.guess_budget() - 2)
        {
            game.guess(letter);
        }
        assert_eq!(game.remaining_guesses(), 2);
        assert!(game.can_hint());
        assert!(matches!(game.hint().result, HintResult::Revealed(_)));
        assert_eq!(game.remaining_guesses(), 1);
    }

    #[test]
    fn a_hint_is_refused_once_the_game_is_over() {
        let mut lost = game_with_word("ALPHABET");
        lose_current_game(&mut lost);
        assert!(!lost.can_hint());
        assert_eq!(lost.hint().result, HintResult::Ignored);

        let mut won = game_with_word("ALPHABET");
        win_current_game(&mut won);
        assert!(!won.can_hint());
        assert_eq!(won.hint().result, HintResult::Ignored);
    }

    #[test]
    fn a_hint_is_refused_when_the_word_is_fully_revealed() {
        let mut game = game_with_word("ALPHABET");
        win_current_game(&mut game);
        // Every guessable letter is showing, so there is nothing left to give.
        assert!(game.cells().iter().all(|cell| cell.revealed));
        assert!(!game.can_hint());
        assert_eq!(game.hint().result, HintResult::Ignored);
        assert_eq!(game.wrong_guesses(), 0, "a refused hint charges nothing");
    }

    #[test]
    fn a_hint_that_completes_the_word_wins_it() {
        let mut game = game_with_word("CAT");
        game.guess('C');
        game.guess('A');
        assert_eq!(game.display(), "CA_");

        let outcome = game.hint();
        assert_eq!(outcome.result, HintResult::Revealed('T'));
        assert_eq!(outcome.game, Some(GameResult::Won));
        // The single-word list is exhausted, so the match ends with it.
        assert_eq!(outcome.match_, Some(MatchOutcome::Win));
        assert!(game.is_won());
        assert_eq!(game.words_won(), 1);
        assert_eq!(game.words_lost(), 0);
        // It still cost a guess: the win is scored on what is left of the
        // budget, which is one less than it would have been.
        assert_eq!(game.wrong_guesses(), 1);
        assert_eq!(game.remaining_guesses(), game.guess_budget() - 1);
    }

    #[test]
    fn a_winning_hint_wins_even_with_the_budget_nearly_gone() {
        let mut game = game_with_word("CAT");
        game.guess('C');
        game.guess('A');
        // Down to two guesses: the hint spends one and must still win rather
        // than lose, whichever check runs first.
        for letter in wrong_letters(&game)
            .into_iter()
            .take(game.guess_budget() - 2)
        {
            game.guess(letter);
        }
        assert_eq!(game.remaining_guesses(), 2);

        let outcome = game.hint();
        assert_eq!(outcome.result, HintResult::Revealed('T'));
        assert_eq!(outcome.game, Some(GameResult::Won));
        assert!(game.is_won());
        assert_eq!(game.remaining_guesses(), 1);
    }

    // ------------------------------------------- the match in flight (item 11)

    /// A three-word match on a known difficulty, with a guess and a hint spent
    /// on the word in hand — a match, in other words, that is genuinely in the
    /// middle of something.
    fn match_in_flight() -> Game {
        let mut game = Game::from_pack_with_seed(
            Pack {
                name: "Fixture".into(),
                guess_budget: Some(8),
                // All three carry the same category and clue, so a test
                // about what survives a round trip does not also depend on
                // which of them the seed happens to deal first.
                words: ["Laptop", "Bagel", "Kayak"]
                    .map(|word| Word {
                        word: word.into(),
                        category: Some("Technology".into()),
                        clue: Some("A computer you can close.".into()),
                    })
                    .to_vec(),
            },
            7,
        )
        .expect("the fixture pack has words in it");
        game.guess('Z');
        game.hint();
        game
    }

    /// The same match, put through the trip a settings file would give it.
    fn round_trip(game: &Game) -> Game {
        Game::resume(game.snapshot().expect("the match is still running"))
            .expect("a snapshot straight off a live game is resumable")
    }

    /// Everything a player can see about a game, for comparing the two sides
    /// of a round trip without reaching into private fields.
    fn visible(game: &Game) -> String {
        format!(
            "{:?} {:?} {:?} {} {} {} {} {} {} {} {} {:?} {:?} {:?}",
            game.difficulty(),
            game.pack_name(),
            game.pack_words(),
            game.display(),
            game.word(),
            game.wrong_guesses(),
            game.guess_budget(),
            game.words_won(),
            game.words_lost(),
            game.total_words(),
            game.word_number(),
            game.game_result(),
            game.guessed_letters(),
            (game.category(), game.clue()),
        )
    }

    #[test]
    fn a_match_in_flight_comes_back_exactly_as_it_was_left() {
        let game = match_in_flight();

        assert_eq!(visible(&round_trip(&game)), visible(&game));
    }

    #[test]
    fn a_resumed_word_keeps_its_category_and_its_clue() {
        let resumed = round_trip(&match_in_flight());

        assert_eq!(resumed.category(), Some("Technology"));
        assert_eq!(resumed.clue(), Some("A computer you can close."));
    }

    #[test]
    fn a_resumed_match_plays_on_from_where_it_was() {
        let mut resumed = round_trip(&match_in_flight());
        let word = resumed.word().to_string();

        win_current_game(&mut resumed);

        assert!(resumed.is_won());
        assert_eq!(resumed.words_won(), 1);
        assert!(resumed.new_game());
        assert_ne!(resumed.word(), word, "the pool moved on to the next word");
    }

    #[test]
    fn a_finished_match_is_not_worth_coming_back_to() {
        let mut game = game_with_word("LAPTOP");
        win_current_game(&mut game);

        assert!(game.is_match_over());
        assert_eq!(game.snapshot(), None);
    }

    #[test]
    fn a_word_that_has_just_been_resolved_is_still_saved() {
        let mut game = match_in_flight();
        lose_current_game(&mut game);

        // The word is over but the match is not, and coming back to the next
        // word of it beats coming back to a match that never happened.
        let resumed = round_trip(&game);
        assert_eq!(resumed.game_result(), Some(GameResult::Lost));
        assert_eq!(resumed.words_lost(), 1);
        assert_eq!(resumed.word_number(), 1);
    }

    #[test]
    fn a_word_given_up_on_does_not_come_back_playable() {
        let mut game = match_in_flight();
        game.give_up();

        // The reason `result` is stored rather than derived: this word has
        // guesses in hand and letters still hidden, which is exactly what a
        // word still being played looks like.
        let resumed = round_trip(&game);
        assert!(resumed.is_game_over());
        assert_eq!(resumed.game_result(), Some(GameResult::Lost));
    }

    #[test]
    fn a_word_list_of_your_own_resumes_with_its_own_budget() {
        let pack = Pack {
            guess_budget: Some(9),
            words: vec![Word::bare("Laptop"), Word::bare("Bagel")],
            ..Pack::default()
        };
        let game = Game::from_pack_with_seed(pack, 3).expect("the pack has words");

        let resumed = round_trip(&game);

        assert_eq!(resumed.difficulty(), None);
        assert_eq!(resumed.guess_budget(), 9);
    }

    #[test]
    fn a_difficulty_gets_its_own_budget_back_whatever_the_file_says() {
        let game = Game::with_seed(Difficulty::Insane, 4);
        let snapshot = Snapshot {
            // The hand edit the re-derivation exists to refuse: Insane's
            // weight with Easy's slack.
            guess_budget: 10,
            ..game.snapshot().expect("the match has just started")
        };

        let resumed = Game::resume(snapshot).expect("only the budget was wrong");

        assert_eq!(resumed.guess_budget(), Difficulty::Insane.guess_budget());
    }

    #[test]
    fn a_loaded_pack_asking_for_a_budget_the_gallows_cannot_draw_is_clamped() {
        let game = game_with_word("LAPTOP");
        for (asked, expected) in [(2, gallows::CORE_PARTS), (40, gallows::PARTS.len())] {
            let snapshot = Snapshot {
                guess_budget: asked,
                ..game.snapshot().expect("the match is running")
            };

            let resumed = Game::resume(snapshot).expect("a budget is clamped, not refused");

            assert_eq!(resumed.guess_budget(), expected, "asked for {asked}");
        }
    }

    #[test]
    fn a_word_with_nothing_left_to_guess_is_not_resumed() {
        let game = game_with_word("LAPTOP");
        for word in ["", "   ", "1234", "!!!"] {
            let snapshot = Snapshot {
                current: Word::bare(word),
                ..game.snapshot().expect("the match is running")
            };

            assert!(Game::resume(snapshot).is_none(), "{word:?}");
        }
    }

    #[test]
    fn a_resumed_word_is_trimmed_and_uppercased_like_any_other() {
        let game = game_with_word("LAPTOP");
        let snapshot = Snapshot {
            current: Word::bare("  laptop  "),
            ..game.snapshot().expect("the match is running")
        };

        let resumed = Game::resume(snapshot).expect("it is a playable word");

        assert_eq!(resumed.word(), "LAPTOP");
    }

    #[test]
    fn a_result_the_rest_of_the_state_could_not_have_produced_is_refused() {
        let mut game = match_in_flight();
        game.guess('Z');
        let live = game.snapshot().expect("the match is running");

        for (result, why) in [
            (Some(GameResult::Won), "the word is not complete"),
            (None, "a spent budget is a loss"),
            (Some(GameResult::Lost), "a completed word is a win"),
        ] {
            let snapshot = match why {
                "a spent budget is a loss" => Snapshot {
                    wrong_guesses: live.guess_budget,
                    result,
                    ..live.clone()
                },
                "a completed word is a win" => Snapshot {
                    guessed: live.current.word.chars().collect(),
                    result,
                    ..live.clone()
                },
                _ => Snapshot {
                    result,
                    ..live.clone()
                },
            };

            assert!(Game::resume(snapshot).is_none(), "{why}");
        }
    }

    #[test]
    fn a_completed_word_with_a_spent_budget_is_not_resumed() {
        // Two words, not one: with an empty pool behind it a resolved word is
        // refused by the finished-match check instead, and this test would
        // pass without ever reaching the rule it is about.
        let game = Game::from_words_with_seed(["Laptop", "Bagel"].map(str::to_string).to_vec(), 9)
            .expect("the fixture list has words in it");
        let live = game.snapshot().expect("the match is running");

        // Unreachable by playing: the guess that empties the budget loses the
        // word before any later one could finish it. Reachable by editing the
        // file, where it would resume as a win under a full gallows.
        for result in [Some(GameResult::Won), Some(GameResult::Lost), None] {
            let snapshot = Snapshot {
                guessed: live.current.word.chars().collect(),
                wrong_guesses: live.guess_budget,
                result,
                ..live.clone()
            };

            assert!(Game::resume(snapshot).is_none(), "{result:?}");
        }
    }

    #[test]
    fn a_resolved_word_missing_from_the_tally_is_not_resumed() {
        let mut won = match_in_flight();
        win_current_game(&mut won);
        let mut lost = match_in_flight();
        lost.give_up();

        // Each is the *matching* counter zeroed, which is the only half that
        // matters: a word that ended is on the tally, so the count for the way
        // it ended cannot be nothing. Left in, the word falls out of
        // `word_number` and out of the summary.
        for (game, zeroed) in [(won, "words_won"), (lost, "words_lost")] {
            let live = game.snapshot().expect("the match is still running");
            let snapshot = Snapshot {
                words_won: 0,
                words_lost: 0,
                ..live
            };

            assert!(Game::resume(snapshot).is_none(), "{zeroed} was zero");
        }
    }

    #[test]
    fn the_word_on_the_board_is_always_one_of_the_words_played() {
        let mut lost = match_in_flight();
        lost.give_up();

        // The invariant the check above buys, on both sides of it: whatever a
        // resumed match looks like, the word on the board is numbered, and it
        // is numbered inside the match.
        for game in [match_in_flight(), lost] {
            let resumed = round_trip(&game);

            assert!(resumed.word_number() >= 1);
            assert!(resumed.word_number() <= resumed.total_words());
        }
    }

    #[test]
    fn more_wrong_guesses_than_the_budget_allows_is_not_resumed() {
        let mut game = match_in_flight();
        // Given up on, so the snapshot is a *plausible* loss in every other
        // respect: without the budget check the count would sail through the
        // "a spent budget is a loss" branch and resume a word carrying more
        // wrong guesses than the gallows has stages to spend them on.
        game.give_up();
        let snapshot = Snapshot {
            wrong_guesses: 99,
            ..game.snapshot().expect("the match is still running")
        };

        assert!(Game::resume(snapshot).is_none());
    }

    #[test]
    fn a_resolved_word_with_nothing_behind_it_is_a_finished_match_not_a_saved_one() {
        let mut game = match_in_flight();
        lose_current_game(&mut game);
        let snapshot = Snapshot {
            // The pool emptied out from under a word that is already over:
            // there is no next word and no summary, so there is nothing here
            // to resume into.
            remaining_words: Vec::new(),
            ..game.snapshot().expect("the match is running")
        };

        assert!(Game::resume(snapshot).is_none());
    }

    #[test]
    fn the_match_length_is_recounted_rather_than_restored() {
        let game = Game::from_words_with_seed(
            ["Laptop", "Bagel", "Kayak", "Violin", "Anchor"]
                .map(str::to_string)
                .to_vec(),
            11,
        )
        .expect("the fixture list has words in it");
        let snapshot = game.snapshot().expect("the match has just started");
        let remaining = snapshot.remaining_words.len();

        let resumed = Game::resume(Snapshot {
            words_won: 2,
            words_lost: 1,
            ..snapshot
        })
        .expect("three words played and the rest to come is an ordinary match");

        // Three finished, one on the board, the rest still to deal.
        assert_eq!(resumed.total_words(), remaining + 4);
        assert_eq!(resumed.word_number(), 4);
    }

    #[test]
    fn a_match_longer_than_a_match_is_not_resumed() {
        let game = Game::with_seed(Difficulty::Easy, 12);
        let snapshot = Snapshot {
            words_won: MATCH_WORDS,
            ..game.snapshot().expect("the match has just started")
        };

        assert!(Game::resume(snapshot).is_none());
    }

    #[test]
    fn junk_in_the_guessed_letters_is_dropped_rather_than_refused() {
        let game = game_with_word("LAPTOP");
        let snapshot = Snapshot {
            guessed: "l4 z!".chars().collect(),
            ..game.snapshot().expect("the match is running")
        };

        let resumed = Game::resume(snapshot).expect("the word is still playable");

        assert_eq!(
            resumed.guessed_letters().iter().collect::<String>(),
            "LZ",
            "uppercased, with everything unguessable dropped"
        );
        assert_eq!(resumed.display(), "L_____");
    }
}
