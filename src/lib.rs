//! Hangman — a Rust + GPUI port of Zack Rauen's 2015 Java hangman.
//!
//! The game rules live in [`game`] and know nothing about the UI, so they can
//! be unit-tested on their own. [`ui`] renders them with gpui-kit, [`audio`]
//! plays the original's two sound cues when the `sound` feature is enabled, and
//! [`settings`] is the little JSON file that remembers the theme, the window
//! and the difficulty between launches.

pub mod audio;
pub mod game;
pub mod settings;
pub mod ui;
