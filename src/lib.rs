//! Hangman — a Rust + GPUI port of Zack Rauen's 2015 Java hangman.
//!
//! The game rules live in [`game`] and know nothing about the UI, so they can
//! be unit-tested on their own. [`ui`] renders them with gpui-kit, and [`audio`]
//! plays the original's two sound cues when the `sound` feature is enabled.

pub mod audio;
pub mod game;
pub mod ui;
