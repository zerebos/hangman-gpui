//! Hangman — a Rust + GPUI port of Zack Rauen's 2015 Java hangman.

use hangman_gpui::game::{Difficulty, Game, MAX_WRONG_GUESSES};

fn main() {
    // Placeholder: the GPUI window lives here once the UI lands.
    let game = Game::new(Difficulty::default());
    println!(
        "{} — {} words, {MAX_WRONG_GUESSES} wrong guesses allowed",
        game.display(),
        game.total_words(),
    );
}
