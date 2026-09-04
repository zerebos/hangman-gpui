//! Hangman — a Rust + GPUI port of Zack Rauen's 2015 Java hangman.
//!
//! The game rules live in [`game`] and know nothing about the UI, so they can
//! be unit-tested on their own.

pub mod game;
