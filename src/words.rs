//! The word packs a match is played from — the file format and nothing else.
//!
//! A **pack** is a named list of words, each of which may carry a category and
//! a clue. The four bundled packs are JSON files under `assets/words/`, baked
//! into the binary with `include_str!` so there is nothing to ship beside the
//! executable, and the player can load one of their own from disk.
//!
//! # The format
//!
//! ```json
//! {
//!   "name": "Easy",
//!   "words": [
//!     { "word": "Laptop", "category": "Technology", "clue": "A computer you can close." },
//!     { "word": "Add/Drop Form", "category": "College life" }
//!   ]
//! }
//! ```
//!
//! Everything but `word` is optional. The file is read and written by serde's
//! derives on [`Word`] and [`Pack`] below, which is the same mechanism
//! [`crate::settings`] uses for `settings.json`: the struct *is* the schema, so
//! the two cannot drift apart.
//!
//! **Unknown keys are ignored rather than refused.** That is serde's default
//! and it is wanted here: a pack written for a later version of the game, with
//! a field this version has never heard of, still plays. The mirror of it is
//! `#[serde(default)]` on every optional field, so a pack written *today* still
//! reads once more fields exist. Between them there is no need for a version
//! number, and none is written.
//!
//! The original's plain-text lists are still legal — [`Pack::parse`] sniffs the
//! text and reads anything that does not start with `{` one word per line, as
//! the game always has.
//!
//! **No UI types**, like [`crate::game`] and [`crate::settings`], so everything
//! here is unit-tested without a window.

use serde::{Deserialize, Serialize};

/// One word, and everything the pack knows about it.
///
/// `category` and `clue` are `Option` rather than `String` because a pack is
/// allowed to say nothing: the four bundled packs carry a category on every
/// word, but a list someone typed out by hand need not, and a plain `.txt`
/// cannot.
///
/// It derives `Serialize` as well as `Deserialize`, and since roadmap item 11
/// both halves have a caller: resuming the word you were on writes the word and
/// the pool still to play into `settings.json`, and storing them *by value*
/// rather than as indices into a pack is what makes editing a pack between
/// launches harmless — you come back to the word you were actually on, clue and
/// all. See [`crate::settings::SavedMatch`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Word {
    /// The word to guess. Spaces, slashes and apostrophes are all fine; only
    /// ASCII letters are guessable, and everything else shows through.
    pub word: String,
    /// What kind of thing it is — `"Food"`, `"College life"`. Shown on the
    /// right of the word panel's `THE WORD` heading while the word is in play,
    /// so it must never give the answer away.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// A sentence about the word's meaning, shown only if the player asks for
    /// it. Unlike a hint it costs nothing: it says what the word *means* and
    /// leaves you to spell it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clue: Option<String>,
}

impl Word {
    /// A word with nothing known about it but its text — what a line of a
    /// `.txt` becomes.
    pub fn bare(word: impl Into<String>) -> Self {
        Self {
            word: word.into(),
            category: None,
            clue: None,
        }
    }

    /// Trimmed and uppercased, as the game plays it.
    ///
    /// A `category` or `clue` that is present but **blank** becomes `None`
    /// here, so that "the pack said nothing" and "the pack said nothing
    /// useful" are the same state from this point on. Everything downstream
    /// asks `is_some()` and nothing re-checks for emptiness: without this, a
    /// `"clue": ""` lights the Clue button up, spends the one press it has,
    /// and reveals an empty line, and a `"category": ""` leaves the word
    /// panel's heading row with a right-hand side that is present and says
    /// nothing.
    fn normalized(&self) -> Self {
        Self {
            word: self.word.trim().to_ascii_uppercase(),
            category: Self::said_something(&self.category),
            clue: Self::said_something(&self.clue),
        }
    }

    /// The field, trimmed, or `None` if there was nothing in it.
    fn said_something(field: &Option<String>) -> Option<String> {
        field
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    }

    /// Is there anything in here to guess?
    ///
    /// A line of punctuation, or of digits, or nothing at all, would otherwise
    /// become an unplayable "word" — the original let you load any `.txt` at
    /// all, so this is not hypothetical.
    fn is_playable(&self) -> bool {
        self.word.chars().any(|c| c.is_ascii_alphabetic())
    }
}

/// A named list of [`Word`]s, as read from a file.
///
/// Both the bundled packs and anything the player opens arrive as one of
/// these. Nothing here is sanitized until [`Pack::into_words`] is called.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Pack {
    /// What to call this list on screen. Empty for a `.txt`, which has nowhere
    /// to put a name, and for a JSON pack that omits it — the view falls back
    /// to its own wording in that case rather than showing a blank.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// How many wrong guesses this pack wants to allow, if it has an opinion.
    ///
    /// Only honoured for a pack the *player* loaded: the four bundled packs are
    /// reached through a difficulty, and a difficulty's budget is the rule for
    /// it. See `crate::game::budget_for`, which also clamps this into the range
    /// the gallows can actually draw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guess_budget: Option<usize>,
    /// The words themselves, in file order. The order does not survive: a match
    /// draws at random.
    #[serde(default)]
    pub words: Vec<Word>,
}

impl Pack {
    /// Read a pack from the contents of a file the player picked.
    ///
    /// The two formats are told apart by what the text *is* rather than by the
    /// name it was saved under, because an extension is the one part of a file
    /// anyone can get wrong: text whose first non-blank character is `{` is
    /// parsed as JSON, and anything else is read one word per line, exactly as
    /// every version of this game has read a word list.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] only for the JSON branch, and only when the JSON
    /// is malformed or shaped wrongly. The line branch cannot fail — every file
    /// is *some* list of lines, even if none of them turn out to be playable.
    pub fn parse(text: &str) -> Result<Self, PackError> {
        // The byte-order mark has to come off before either branch sees the
        // text. `trim_start` does not take it: U+FEFF is not whitespace in
        // Unicode, so a BOM'd pack would fail the `{` test, fall to the line
        // branch, and load `"word": "Laptop",` and its neighbours as words —
        // silently, which is the worst way for it to go wrong. Windows editors
        // write one by default, so this is the common case rather than an
        // exotic one. serde_json refuses a leading BOM too, so stripping it
        // here fixes both halves at once.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        if text.trim_start().starts_with('{') {
            serde_json::from_str(text).map_err(PackError)
        } else {
            Ok(Self::from_lines(text))
        }
    }

    /// A pack from plain text, one word per line, with no categories or clues.
    pub fn from_lines(text: &str) -> Self {
        Self {
            words: text.lines().map(Word::bare).collect(),
            ..Self::default()
        }
    }

    /// A pack from words already in hand. For tests, and for the string-shaped
    /// constructors on [`crate::game::Game`].
    pub fn from_words(words: Vec<String>) -> Self {
        Self {
            words: words.into_iter().map(Word::bare).collect(),
            ..Self::default()
        }
    }

    /// Read one of the packs baked into the binary.
    ///
    /// # Panics
    ///
    /// If the pack does not parse. That is deliberate, and it is the opposite
    /// of the rule [`crate::settings`] follows: a settings file we cannot read
    /// is the *player's*, and falling back costs them nothing they will miss,
    /// but a word pack we cannot read is ours and shipping one is a bug. The
    /// test below parses all four, so it cannot reach a release.
    pub fn bundled(json: &'static str) -> Self {
        serde_json::from_str(json).expect("a bundled word pack must parse")
    }

    /// The pack's name with its surrounding blanks gone.
    ///
    /// Blank is absent here exactly as it is for a category or a clue (see
    /// [`Word::said_something`]): `"name": "   "` is a pack that did not name
    /// itself, not a pack called three spaces. Taking the name through this is
    /// what lets [`crate::game::Game::pack_name`] go on testing emptiness
    /// alone — nothing untrimmed reaches it, so it has nothing to re-check.
    pub fn display_name(&self) -> &str {
        self.name.trim()
    }

    /// The pack's words, trimmed, uppercased, and with the unplayable ones
    /// dropped.
    ///
    /// The bundled packs are already well behaved; this exists because the
    /// original let the player load any `.txt` at all, and a blank or
    /// punctuation-only line would otherwise become a word with nothing in it
    /// to guess.
    pub fn into_words(self) -> Vec<Word> {
        sanitize(self.words)
    }
}

/// Trim, uppercase, and drop anything with nothing in it to guess.
///
/// [`Pack::into_words`] is one caller and resuming a saved match
/// ([`crate::game::Game::resume`]) is the other: a match read back out of
/// `settings.json` has been through a file the player may have edited by hand,
/// so it deserves exactly the cleanup a pack read off disk gets rather than a
/// second, slightly different one.
pub fn sanitize(words: Vec<Word>) -> Vec<Word> {
    words
        .iter()
        .map(Word::normalized)
        .filter(Word::is_playable)
        .collect()
}

/// A JSON pack that would not parse.
///
/// Wraps serde_json's own error, which already says what was wrong and where.
/// The view does not show it — a list that will not load leaves the word on the
/// board and says so in one line — but it costs nothing to carry and makes the
/// failure debuggable from a test.
#[derive(Debug)]
pub struct PackError(serde_json::Error);

impl std::fmt::Display for PackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "the word pack could not be read: {}", self.0)
    }
}

impl std::error::Error for PackError {}

#[cfg(test)]
mod tests {
    use super::{Pack, PackError, Word};

    const FULL: &str = r#"{
        "name": "Test pack",
        "guess_budget": 8,
        "words": [
            { "word": "Laptop", "category": "Technology", "clue": "A computer you can close." },
            { "word": "Add/Drop Form", "category": "College life" },
            { "word": "Plain" }
        ]
    }"#;

    #[test]
    fn a_pack_reads_every_field_it_is_given() {
        let pack = Pack::parse(FULL).expect("the fixture is valid JSON");
        assert_eq!(pack.name, "Test pack");
        assert_eq!(pack.guess_budget, Some(8));
        assert_eq!(pack.words.len(), 3);
        assert_eq!(pack.words[0].category.as_deref(), Some("Technology"));
        assert_eq!(
            pack.words[0].clue.as_deref(),
            Some("A computer you can close.")
        );
    }

    #[test]
    fn the_optional_fields_are_optional() {
        let pack = Pack::parse(FULL).expect("the fixture is valid JSON");
        // The second word has a category and no clue, the third has neither.
        assert_eq!(pack.words[1].clue, None);
        assert_eq!(pack.words[2].category, None);
        assert_eq!(pack.words[2].clue, None);
    }

    #[test]
    fn a_pack_needs_nothing_but_its_words() {
        let pack = Pack::parse(r#"{ "words": [{ "word": "Alpha" }] }"#)
            .expect("name and guess_budget both default");
        assert_eq!(pack.name, "");
        assert_eq!(pack.guess_budget, None);
        assert_eq!(pack.words, vec![Word::bare("Alpha")]);
    }

    #[test]
    fn a_key_this_version_has_never_heard_of_is_ignored_rather_than_refused() {
        // The forward half of the no-version-number bargain: a pack written for
        // a later game still plays on this one. `deny_unknown_fields` would
        // turn this into a parse error, which is why it is not there.
        let pack = Pack::parse(
            r#"{ "author": "someone", "words": [{ "word": "Alpha", "difficulty": 9 }] }"#,
        )
        .expect("unknown keys are skipped");
        assert_eq!(pack.words, vec![Word::bare("Alpha")]);
    }

    #[test]
    fn malformed_json_is_an_error_rather_than_an_empty_pack() {
        // The distinction matters to the view: a file that will not parse
        // leaves the word on the board, so it must not look like a pack that
        // merely had nothing playable in it.
        let error = Pack::parse(r#"{ "words": [ }"#).expect_err("that is not JSON");
        assert!(
            error
                .to_string()
                .starts_with("the word pack could not be read"),
            "unexpected message: {error}"
        );
    }

    #[test]
    fn text_that_does_not_start_with_a_brace_is_read_as_lines() {
        let pack = Pack::parse("Alpha\nOmega\n").expect("the line branch cannot fail");
        assert_eq!(pack.words, vec![Word::bare("Alpha"), Word::bare("Omega")]);
        assert_eq!(pack.name, "");
    }

    #[test]
    fn the_format_is_chosen_by_the_content_and_not_by_a_file_name() {
        // Leading whitespace must not push a JSON pack down the line branch:
        // a pretty-printer or an editor can add it, and the player never sees
        // it.
        let pack = Pack::parse("\n  \t{ \"words\": [{ \"word\": \"Alpha\" }] }")
            .expect("leading whitespace is skipped");
        assert_eq!(pack.words, vec![Word::bare("Alpha")]);
    }

    #[test]
    fn a_byte_order_mark_does_not_turn_a_pack_into_a_word_list() {
        // U+FEFF is not whitespace, so `trim_start` leaves it and the `{` test
        // fails without the explicit strip. Windows editors write one by
        // default, and the failure is silent: every line of the JSON becomes a
        // "word".
        let pack =
            Pack::parse("\u{feff}{ \"name\": \"Pets\", \"words\": [{ \"word\": \"Cat\" }] }")
                .expect("a BOM is stripped before the format is sniffed");
        assert_eq!(pack.name, "Pets");
        assert_eq!(pack.words, vec![Word::bare("Cat")]);
    }

    #[test]
    fn a_blank_category_or_clue_is_the_same_as_not_having_one() {
        // Everything downstream asks `is_some()` and nothing re-checks for
        // emptiness, so a present-but-empty field would light the Clue button
        // up and reveal nothing, and would give the word panel's heading row a
        // right-hand side that is there and empty.
        let words =
            Pack::parse(r#"{ "words": [{ "word": "Alpha", "category": "", "clue": "   " }] }"#)
                .expect("valid JSON")
                .into_words();
        assert_eq!(words, vec![Word::bare("ALPHA")]);
    }

    #[test]
    fn a_category_or_clue_with_something_in_it_is_trimmed_rather_than_dropped() {
        let words = Pack::parse(
            r#"{ "words": [{ "word": "Alpha", "category": "  Letters  ", "clue": " The first. " }] }"#,
        )
        .expect("valid JSON")
        .into_words();
        assert_eq!(words[0].category.as_deref(), Some("Letters"));
        assert_eq!(words[0].clue.as_deref(), Some("The first."));
    }

    #[test]
    fn a_word_keeps_its_spaces_and_punctuation_but_loses_its_case() {
        let words = Pack::from_words(vec!["  Add/Drop Form  ".into()]).into_words();
        assert_eq!(words, vec![Word::bare("ADD/DROP FORM")]);
    }

    #[test]
    fn lines_with_nothing_to_guess_are_dropped() {
        let words = Pack::parse("Alpha\n\n   \n,,,\n123\nOmega\n")
            .expect("the line branch cannot fail")
            .into_words();
        assert_eq!(words, vec![Word::bare("ALPHA"), Word::bare("OMEGA")]);
    }

    #[test]
    fn sanitizing_keeps_the_category_and_clue_attached_to_the_word() {
        // The normalize step rebuilds each `Word`, so it is exactly the sort of
        // place a field gets quietly dropped.
        let words = Pack::parse(FULL)
            .expect("the fixture is valid JSON")
            .into_words();
        assert_eq!(words[0].word, "LAPTOP");
        assert_eq!(words[0].category.as_deref(), Some("Technology"));
        assert_eq!(words[0].clue.as_deref(), Some("A computer you can close."));
    }

    #[test]
    fn a_pack_survives_a_round_trip_through_json() {
        // Item 11 will write these back out, so the derives have to agree in
        // both directions. Asserted against a literal rather than by comparing
        // the two structs, which would pass even if `category` and `clue` were
        // swapped in both directions at once.
        let pack = Pack::parse(FULL).expect("the fixture is valid JSON");
        let written = serde_json::to_string(&pack).expect("a pack always serializes");
        assert_eq!(
            written,
            r#"{"name":"Test pack","guess_budget":8,"words":[{"word":"Laptop","category":"Technology","clue":"A computer you can close."},{"word":"Add/Drop Form","category":"College life"},{"word":"Plain"}]}"#
        );
    }

    #[test]
    fn what_a_word_does_not_have_is_left_out_of_the_json_rather_than_written_null() {
        let written =
            serde_json::to_string(&Word::bare("ALPHA")).expect("a word always serializes");
        assert_eq!(written, r#"{"word":"ALPHA"}"#);
    }

    /// `PackError` is returned by value from `parse`, so it has to be `Send`
    /// and `Sync` to travel out of the background read the view does.
    #[test]
    fn a_pack_error_can_cross_a_thread() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PackError>();
    }
}
