//! Points, streaks and the lifetime tally — the scoring model, with no UI in it.
//!
//! [`crate::game`] knows the *rules*: whether a word was won, whether the match
//! is over. It deliberately keeps no score beyond the per-match win/loss counts
//! it needs to decide a [`MatchOutcome`]. Everything a player would call a
//! score lives here instead, which is what keeps serde — and any notion of a
//! "best streak" — out of the rules module.
//!
//! # What a word is worth
//!
//! ```text
//! (BASE_WORD_POINTS + POINTS_PER_REMAINING_GUESS × guesses left) × difficulty weight
//!     + STREAK_BONUS × min(streak − 1, MAX_STREAK_BONUS_STEPS)
//! ```
//!
//! So a clean Insane win at the top of a streak is `(50 + 10 × 6) × 4 + 25 × 4`
//! = 540, and the same word squeaked out with one guess left, first of a
//! streak, is `(50 + 10) × 4` = 240. A lost word is worth nothing.
//!
//! # Streaks
//!
//! The current streak counts **consecutive words solved**, and nothing but
//! failing a word ends it. It deliberately survives the end of a match, a
//! difficulty switch, a custom word list and a restart of the game — a streak
//! that reset every ten words would not be a streak. Losing a word, including
//! giving up on one, puts it back to zero. This is the opposite of the
//! per-match `words_won`/`words_lost` counters in [`crate::game`], which
//! [`crate::game::Game::set_difficulty`] resets on purpose.
//!
//! # Custom word lists
//!
//! A match played from a file has no [`Difficulty`], so there is no bucket to
//! put it in: it scores at weight 1 and counts towards the lifetime totals and
//! the streak, but towards none of the per-difficulty breakdown.

use serde::{Deserialize, Serialize};

use crate::game::{Difficulty, GameResult, MatchOutcome};

/// How many per-difficulty buckets there are, which is one each.
const DIFFICULTIES: usize = Difficulty::ALL.len();

/// What solving a word is worth before the guesses left, the difficulty and
/// the streak are taken into account.
pub const BASE_WORD_POINTS: u32 = 50;

/// What each *unused* wrong guess adds to a solved word, so finishing a word
/// with the gallows still empty is worth more than scraping through.
pub const POINTS_PER_REMAINING_GUESS: u32 = 10;

/// What each step of the streak bonus adds. The first word of a streak earns
/// none of it; the second earns one step, and so on.
pub const STREAK_BONUS: u32 = 25;

/// How many steps of [`STREAK_BONUS`] a word can earn. The bonus stops growing
/// at a streak of five so a long run cannot dwarf everything else.
pub const MAX_STREAK_BONUS_STEPS: u32 = 4;

/// What one solved word scores.
///
/// `difficulty` is `None` for a custom word list, which scores at weight 1.
/// `remaining_guesses` is the budget left when the word was finished, as
/// [`crate::game::Game::remaining_guesses`] reports it. `streak_after` is the
/// streak *including* this word, so a value of 1 — the first word of a streak —
/// earns no streak bonus.
///
/// Every step saturates: no input can make this panic, wrap or overflow.
pub fn word_points(
    difficulty: Option<Difficulty>,
    remaining_guesses: usize,
    streak_after: u32,
) -> u32 {
    let remaining = u32::try_from(remaining_guesses).unwrap_or(u32::MAX);
    let weight = difficulty.map_or(1, Difficulty::weight);

    let base =
        BASE_WORD_POINTS.saturating_add(POINTS_PER_REMAINING_GUESS.saturating_mul(remaining));
    let bonus =
        STREAK_BONUS.saturating_mul(streak_after.saturating_sub(1).min(MAX_STREAK_BONUS_STEPS));

    base.saturating_mul(weight).saturating_add(bonus)
}

/// The lifetime tally for one difficulty.
///
/// Matches played from a custom word list are counted nowhere in here — see the
/// module docs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DifficultyStats {
    /// Every point ever earned on this difficulty.
    pub points: u64,
    /// Words solved on it.
    pub words_won: u32,
    /// Words failed on it, giving up included.
    pub words_lost: u32,
    /// Matches finished on it with more wins than losses.
    pub matches_won: u32,
    /// …with more losses than wins.
    pub matches_lost: u32,
    /// …with the two equal.
    pub matches_tied: u32,
}

impl DifficultyStats {
    /// Words won plus words lost.
    pub fn words_played(&self) -> u32 {
        self.words_won.saturating_add(self.words_lost)
    }

    /// Matches won plus lost plus tied.
    pub fn matches_played(&self) -> u32 {
        self.matches_won
            .saturating_add(self.matches_lost)
            .saturating_add(self.matches_tied)
    }

    /// The share of played words that were solved, in `0.0..=1.0`.
    ///
    /// Nothing played is `0.0` rather than a division by zero.
    pub fn win_rate(&self) -> f32 {
        win_rate(self.words_won, self.words_played())
    }
}

/// Everything the game remembers about how you have played, for ever.
///
/// This is the value [`crate::settings::Settings`] persists, so every field has
/// to survive a round trip through JSON. The per-difficulty breakdown is stored
/// as an object keyed by [`Difficulty::label`] rather than as an array, so a
/// hand-edited file reads sensibly and a difficulty that does not exist in this
/// version is simply dropped.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    /// Every point ever earned, on every difficulty and every word list.
    pub points: u64,
    /// Words solved, ever.
    pub words_won: u32,
    /// Words failed, ever, giving up included.
    pub words_lost: u32,
    /// Words solved in a row right now. Survives matches and restarts; only a
    /// failed word clears it.
    pub streak: u32,
    /// The longest [`Stats::streak`] ever reached.
    pub best_streak: u32,
    /// Matches finished with more wins than losses.
    pub matches_won: u32,
    /// …with more losses than wins.
    pub matches_lost: u32,
    /// …with the two equal.
    pub matches_tied: u32,
    /// One bucket per [`Difficulty`], in [`Difficulty::ALL`] order. Private
    /// because the parallel indexing is an implementation detail;
    /// [`Stats::for_difficulty`] is the way in.
    #[serde(with = "stats_by_difficulty")]
    by_difficulty: [DifficultyStats; DIFFICULTIES],
}

impl Stats {
    /// The lifetime tally for one difficulty.
    pub fn for_difficulty(&self, difficulty: Difficulty) -> DifficultyStats {
        self.by_difficulty[bucket_index(difficulty)]
    }

    /// Record a finished word and return what it scored.
    ///
    /// A win extends the streak first and *then* scores, so the word that takes
    /// the streak from 1 to 2 is the first one to earn a streak bonus. A loss
    /// scores nothing and breaks the streak — see the module docs.
    pub fn record_word(
        &mut self,
        difficulty: Option<Difficulty>,
        result: GameResult,
        remaining_guesses: usize,
    ) -> u32 {
        match result {
            GameResult::Won => {
                self.streak = self.streak.saturating_add(1);
                self.best_streak = self.best_streak.max(self.streak);
                self.words_won = self.words_won.saturating_add(1);

                let points = word_points(difficulty, remaining_guesses, self.streak);
                self.points = self.points.saturating_add(u64::from(points));

                if let Some(bucket) = self.bucket_mut(difficulty) {
                    bucket.words_won = bucket.words_won.saturating_add(1);
                    bucket.points = bucket.points.saturating_add(u64::from(points));
                }
                points
            }
            GameResult::Lost => {
                self.streak = 0;
                self.words_lost = self.words_lost.saturating_add(1);
                if let Some(bucket) = self.bucket_mut(difficulty) {
                    bucket.words_lost = bucket.words_lost.saturating_add(1);
                }
                0
            }
        }
    }

    /// Record a finished match. The streak is not touched: it spans matches.
    pub fn record_match(&mut self, difficulty: Option<Difficulty>, outcome: MatchOutcome) {
        match outcome {
            MatchOutcome::Win => self.matches_won = self.matches_won.saturating_add(1),
            MatchOutcome::Loss => self.matches_lost = self.matches_lost.saturating_add(1),
            MatchOutcome::Tie => self.matches_tied = self.matches_tied.saturating_add(1),
        }
        if let Some(bucket) = self.bucket_mut(difficulty) {
            match outcome {
                MatchOutcome::Win => bucket.matches_won = bucket.matches_won.saturating_add(1),
                MatchOutcome::Loss => bucket.matches_lost = bucket.matches_lost.saturating_add(1),
                MatchOutcome::Tie => bucket.matches_tied = bucket.matches_tied.saturating_add(1),
            }
        }
    }

    /// Matches won plus lost plus tied.
    pub fn matches_played(&self) -> u32 {
        self.matches_won
            .saturating_add(self.matches_lost)
            .saturating_add(self.matches_tied)
    }

    /// Words won plus words lost.
    pub fn words_played(&self) -> u32 {
        self.words_won.saturating_add(self.words_lost)
    }

    /// The share of played words that were solved, in `0.0..=1.0`.
    ///
    /// Nothing played is `0.0` rather than a division by zero.
    pub fn win_rate(&self) -> f32 {
        win_rate(self.words_won, self.words_played())
    }

    /// Throw the lot away and start again from nothing.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The bucket for `difficulty`, or `None` for a custom word list — which
    /// belongs to no difficulty and so is counted only in the lifetime totals.
    fn bucket_mut(&mut self, difficulty: Option<Difficulty>) -> Option<&mut DifficultyStats> {
        difficulty.map(|difficulty| &mut self.by_difficulty[bucket_index(difficulty)])
    }
}

/// The running score of the process: the lifetime [`Stats`] plus the points
/// earned in the match on screen.
///
/// This is the one place the UI has to hold, so the UI itself does no
/// arithmetic. `match_points` is the only part that is not persisted: it
/// belongs to the match being played and dies with it.
#[derive(Debug, Default, Clone)]
pub struct Session {
    match_points: u32,
    stats: Stats,
}

impl Session {
    /// Start a session from the stats a previous run left behind. The match
    /// score starts at zero: no match is in progress yet.
    pub fn new(stats: Stats) -> Self {
        Self {
            match_points: 0,
            stats,
        }
    }

    /// The lifetime tally.
    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// What the match on screen has scored so far.
    pub fn match_points(&self) -> u32 {
        self.match_points
    }

    /// Begin a fresh match: the match score goes back to zero.
    ///
    /// The streak is deliberately left alone — it spans matches, and only a
    /// failed word ends it. Dealing the *next word* of a match is not a new
    /// match and must not come through here.
    pub fn start_match(&mut self) {
        self.match_points = 0;
    }

    /// Record a finished word, adding what it scored to the match total, and
    /// return those points so the caller can show them.
    pub fn record_word(
        &mut self,
        difficulty: Option<Difficulty>,
        result: GameResult,
        remaining_guesses: usize,
    ) -> u32 {
        let points = self
            .stats
            .record_word(difficulty, result, remaining_guesses);
        self.match_points = self.match_points.saturating_add(points);
        points
    }

    /// Record a finished match.
    pub fn record_match(&mut self, difficulty: Option<Difficulty>, outcome: MatchOutcome) {
        self.stats.record_match(difficulty, outcome);
    }

    /// Wipe the lifetime tally, and the match on screen with it.
    pub fn reset_stats(&mut self) {
        self.stats.reset();
        self.match_points = 0;
    }
}

/// Where `difficulty`'s bucket sits in the parallel array.
fn bucket_index(difficulty: Difficulty) -> usize {
    Difficulty::ALL
        .iter()
        .position(|candidate| *candidate == difficulty)
        // `Difficulty::ALL` is exhaustive by construction, so this is
        // unreachable; falling back beats an index that could panic.
        .unwrap_or(0)
}

/// `won / played`, and `0.0` when nothing has been played.
fn win_rate(won: u32, played: u32) -> f32 {
    if played == 0 {
        0.
    } else {
        won as f32 / played as f32
    }
}

/// Serde for the per-difficulty buckets, stored as an object keyed by the name
/// from the original's Difficulty menu:
///
/// ```json
/// "by_difficulty": { "Easy": { "points": 320, … }, "Medium": { … } }
/// ```
///
/// This mirrors `settings::difficulty_by_name`, and for the same reasons: the
/// file should be readable by hand, and it must never be able to fail a load.
/// A key this version does not know is dropped, a known key holding nonsense
/// falls back to an empty bucket, and a missing key is simply an empty bucket.
mod stats_by_difficulty {
    use std::collections::BTreeMap;

    use serde::ser::SerializeMap;
    use serde::{Deserialize as _, Deserializer, Serializer};

    use super::{DIFFICULTIES, DifficultyStats};
    use crate::game::Difficulty;

    type Buckets = [DifficultyStats; DIFFICULTIES];

    pub fn serialize<S: Serializer>(buckets: &Buckets, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(buckets.len()))?;
        for (difficulty, bucket) in Difficulty::ALL.into_iter().zip(buckets) {
            map.serialize_entry(difficulty.label(), bucket)?;
        }
        map.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Buckets, D::Error> {
        // Read the values as raw JSON first so that one bad entry costs only
        // that entry: deserializing straight into `DifficultyStats` would fail
        // the whole file over a single hand-edited typo.
        let named = BTreeMap::<String, serde_json::Value>::deserialize(deserializer)?;

        let mut buckets = Buckets::default();
        for (name, value) in named {
            let found = Difficulty::ALL
                .iter()
                .position(|difficulty| difficulty.label().eq_ignore_ascii_case(&name));
            if let Some(index) = found {
                buckets[index] = serde_json::from_value(value).unwrap_or_default();
            }
        }
        Ok(buckets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clean win: the whole guess budget still unspent.
    const CLEAN: usize = crate::game::MAX_WRONG_GUESSES;

    fn won(stats: &mut Stats, difficulty: Difficulty) -> u32 {
        stats.record_word(Some(difficulty), GameResult::Won, CLEAN)
    }

    fn lost(stats: &mut Stats, difficulty: Difficulty) {
        stats.record_word(Some(difficulty), GameResult::Lost, CLEAN);
    }

    // ------------------------------------------------------------ the formula

    #[test]
    fn the_first_word_of_a_streak_earns_no_bonus() {
        // (50 + 10 * 6) * 1 + 0
        assert_eq!(word_points(Some(Difficulty::Easy), 6, 1), 110);
    }

    #[test]
    fn every_remaining_guess_is_worth_ten_before_the_weight() {
        assert_eq!(word_points(Some(Difficulty::Easy), 0, 1), 50);
        assert_eq!(word_points(Some(Difficulty::Easy), 1, 1), 60);
        assert_eq!(word_points(Some(Difficulty::Medium), 1, 1), 120);
    }

    #[test]
    fn a_clean_insane_win_on_a_long_streak_is_five_hundred_and_forty() {
        assert_eq!(word_points(Some(Difficulty::Insane), 6, 5), 540);
    }

    #[test]
    fn the_streak_bonus_stops_growing_after_four_steps() {
        let capped = word_points(Some(Difficulty::Easy), 6, 5);
        assert_eq!(capped, 110 + STREAK_BONUS * MAX_STREAK_BONUS_STEPS);
        for streak in [6, 20, u32::MAX] {
            assert_eq!(word_points(Some(Difficulty::Easy), 6, streak), capped);
        }
    }

    #[test]
    fn a_custom_word_list_scores_at_weight_one() {
        assert_eq!(
            word_points(None, 6, 1),
            word_points(Some(Difficulty::Easy), 6, 1)
        );
    }

    #[test]
    fn absurd_inputs_saturate_instead_of_overflowing() {
        assert_eq!(
            word_points(Some(Difficulty::Insane), usize::MAX, 1),
            u32::MAX
        );
        assert_eq!(word_points(None, 0, 0), BASE_WORD_POINTS);
    }

    // ------------------------------------------------------------- the tally

    #[test]
    fn a_fresh_tally_is_empty() {
        let stats = Stats::default();

        assert_eq!(stats.points, 0);
        assert_eq!(stats.words_played(), 0);
        assert_eq!(stats.matches_played(), 0);
        assert_eq!(stats.streak, 0);
        assert_eq!(stats.best_streak, 0);
        assert_eq!(stats.win_rate(), 0.);
        for difficulty in Difficulty::ALL {
            assert_eq!(stats.for_difficulty(difficulty), DifficultyStats::default());
        }
    }

    #[test]
    fn a_won_word_scores_and_extends_the_streak() {
        let mut stats = Stats::default();

        let points = won(&mut stats, Difficulty::Medium);

        assert_eq!(points, 220); // (50 + 60) * 2, no bonus on the first word
        assert_eq!(stats.points, 220);
        assert_eq!(stats.words_won, 1);
        assert_eq!(stats.streak, 1);
        assert_eq!(stats.best_streak, 1);
        assert_eq!(stats.for_difficulty(Difficulty::Medium).points, 220);
        assert_eq!(stats.for_difficulty(Difficulty::Medium).words_won, 1);
    }

    #[test]
    fn the_second_word_of_a_streak_is_the_first_to_earn_a_bonus() {
        let mut stats = Stats::default();

        assert_eq!(won(&mut stats, Difficulty::Easy), 110);
        assert_eq!(won(&mut stats, Difficulty::Easy), 110 + STREAK_BONUS);
        assert_eq!(won(&mut stats, Difficulty::Easy), 110 + STREAK_BONUS * 2);
        assert_eq!(stats.streak, 3);
    }

    #[test]
    fn a_lost_word_scores_nothing_and_breaks_the_streak() {
        let mut stats = Stats::default();
        won(&mut stats, Difficulty::Easy);
        won(&mut stats, Difficulty::Easy);
        assert_eq!(stats.streak, 2);

        assert_eq!(
            stats.record_word(Some(Difficulty::Easy), GameResult::Lost, 0),
            0
        );

        assert_eq!(stats.streak, 0);
        assert_eq!(stats.best_streak, 2, "the best is a high-water mark");
        assert_eq!(stats.words_lost, 1);
        assert_eq!(stats.for_difficulty(Difficulty::Easy).words_lost, 1);
    }

    #[test]
    fn the_streak_survives_a_difficulty_change() {
        let mut stats = Stats::default();
        won(&mut stats, Difficulty::Easy);
        won(&mut stats, Difficulty::Insane);
        stats.record_match(Some(Difficulty::Insane), MatchOutcome::Win);
        won(&mut stats, Difficulty::Hard);

        assert_eq!(stats.streak, 3);
    }

    #[test]
    fn a_custom_word_list_counts_towards_the_totals_but_no_bucket() {
        let mut stats = Stats::default();

        let points = stats.record_word(None, GameResult::Won, 6);
        stats.record_match(None, MatchOutcome::Win);

        assert_eq!(points, 110);
        assert_eq!(stats.points, 110);
        assert_eq!(stats.words_won, 1);
        assert_eq!(stats.streak, 1);
        assert_eq!(stats.matches_won, 1);
        for difficulty in Difficulty::ALL {
            assert_eq!(
                stats.for_difficulty(difficulty),
                DifficultyStats::default(),
                "{difficulty:?}"
            );
        }
    }

    #[test]
    fn matches_are_counted_by_outcome() {
        let mut stats = Stats::default();

        stats.record_match(Some(Difficulty::Hard), MatchOutcome::Win);
        stats.record_match(Some(Difficulty::Hard), MatchOutcome::Loss);
        stats.record_match(Some(Difficulty::Hard), MatchOutcome::Tie);
        stats.record_match(Some(Difficulty::Easy), MatchOutcome::Win);

        assert_eq!(
            (stats.matches_won, stats.matches_lost, stats.matches_tied),
            (2, 1, 1)
        );
        assert_eq!(stats.matches_played(), 4);
        let hard = stats.for_difficulty(Difficulty::Hard);
        assert_eq!(
            (hard.matches_won, hard.matches_lost, hard.matches_tied),
            (1, 1, 1)
        );
        assert_eq!(hard.matches_played(), 3);
        assert_eq!(stats.for_difficulty(Difficulty::Easy).matches_won, 1);
    }

    #[test]
    fn the_win_rate_is_the_share_of_words_solved() {
        let mut stats = Stats::default();
        won(&mut stats, Difficulty::Easy);
        won(&mut stats, Difficulty::Easy);
        won(&mut stats, Difficulty::Easy);
        lost(&mut stats, Difficulty::Easy);

        assert_eq!(stats.words_played(), 4);
        assert_eq!(stats.win_rate(), 0.75);
        assert_eq!(stats.for_difficulty(Difficulty::Easy).win_rate(), 0.75);
    }

    #[test]
    fn resetting_puts_everything_back_to_nothing() {
        let mut stats = Stats::default();
        won(&mut stats, Difficulty::Insane);
        stats.record_match(Some(Difficulty::Insane), MatchOutcome::Win);

        stats.reset();

        assert_eq!(stats, Stats::default());
    }

    // ------------------------------------------------------------ the session

    #[test]
    fn a_session_adds_up_the_points_of_the_match_on_screen() {
        let mut session = Session::default();

        let first = session.record_word(Some(Difficulty::Easy), GameResult::Won, 6);
        let second = session.record_word(Some(Difficulty::Easy), GameResult::Won, 6);

        assert_eq!(session.match_points(), first + second);
        assert_eq!(session.stats().points, u64::from(first + second));
    }

    #[test]
    fn starting_a_match_zeroes_the_score_but_never_the_streak() {
        let mut session = Session::default();
        session.record_word(Some(Difficulty::Easy), GameResult::Won, 6);
        let lifetime = session.stats().points;

        session.start_match();

        assert_eq!(session.match_points(), 0);
        assert_eq!(session.stats().streak, 1);
        assert_eq!(session.stats().best_streak, 1);
        assert_eq!(session.stats().points, lifetime, "lifetime points are kept");
    }

    #[test]
    fn a_session_starts_from_the_stats_it_is_handed() {
        let mut stats = Stats::default();
        won(&mut stats, Difficulty::Hard);

        let session = Session::new(stats.clone());

        assert_eq!(session.stats(), &stats);
        assert_eq!(session.match_points(), 0, "no match is in progress yet");
    }

    #[test]
    fn resetting_a_session_clears_the_match_score_too() {
        let mut session = Session::default();
        session.record_word(Some(Difficulty::Easy), GameResult::Won, 6);

        session.reset_stats();

        assert_eq!(session.match_points(), 0);
        assert_eq!(session.stats(), &Stats::default());
    }

    // --------------------------------------------------------------- the file

    #[test]
    fn stats_survive_a_round_trip() {
        let mut stats = Stats::default();
        won(&mut stats, Difficulty::Easy);
        won(&mut stats, Difficulty::Insane);
        lost(&mut stats, Difficulty::Medium);
        stats.record_match(Some(Difficulty::Insane), MatchOutcome::Tie);

        let json = serde_json::to_string_pretty(&stats).expect("stats should serialize");

        assert_eq!(
            serde_json::from_str::<Stats>(&json).expect("stats should parse"),
            stats
        );
    }

    #[test]
    fn the_buckets_are_stored_by_name() {
        let mut stats = Stats::default();
        won(&mut stats, Difficulty::Hard);

        let json = serde_json::to_string(&stats).expect("stats should serialize");

        assert!(json.contains(r#""best_streak":1"#), "{json}");
        assert!(json.contains(r#""Hard":{"points":330"#), "{json}");
        assert!(json.contains(r#""Easy":{"points":0"#), "{json}");
    }

    #[test]
    fn a_missing_bucket_reads_as_an_empty_one() {
        let stats: Stats =
            serde_json::from_str(r#"{"points": 40, "by_difficulty": {"Easy": {"points": 40}}}"#)
                .expect("stats should parse");

        assert_eq!(stats.points, 40);
        assert_eq!(stats.for_difficulty(Difficulty::Easy).points, 40);
        assert_eq!(
            stats.for_difficulty(Difficulty::Insane),
            DifficultyStats::default()
        );
    }

    #[test]
    fn an_unknown_bucket_is_ignored_rather_than_failing() {
        let stats: Stats = serde_json::from_str(
            r#"{"by_difficulty": {"Trivial": {"points": 9}, "hard": {"points": 3}}}"#,
        )
        .expect("an unknown difficulty should not fail the parse");

        // Known names still land, case-insensitively, like the difficulty key.
        assert_eq!(stats.for_difficulty(Difficulty::Hard).points, 3);
        for difficulty in [Difficulty::Easy, Difficulty::Medium, Difficulty::Insane] {
            assert_eq!(
                stats.for_difficulty(difficulty),
                DifficultyStats::default(),
                "{difficulty:?}"
            );
        }
    }

    #[test]
    fn a_bucket_holding_nonsense_reads_as_an_empty_one() {
        let stats: Stats =
            serde_json::from_str(r#"{"points": 7, "by_difficulty": {"Easy": "lots"}}"#)
                .expect("one bad bucket should not fail the parse");

        assert_eq!(stats.points, 7);
        assert_eq!(
            stats.for_difficulty(Difficulty::Easy),
            DifficultyStats::default()
        );
    }

    #[test]
    fn an_empty_object_is_the_default_tally() {
        assert_eq!(
            serde_json::from_str::<Stats>("{}").expect("stats should parse"),
            Stats::default()
        );
    }
}
