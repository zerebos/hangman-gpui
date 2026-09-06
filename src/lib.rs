//! Hangman — a Rust + GPUI port of Zack Rauen's 2015 Java hangman.
//!
//! The game rules live in [`game`] and know nothing about the UI, so they can
//! be unit-tested on their own. [`ui`] renders them with gpui-kit, [`audio`]
//! plays the original's two sound cues when the `sound` feature is enabled,
//! [`stats`] scores the words and keeps the streak, and [`settings`] is the
//! little JSON file that remembers the theme, the window, the difficulty and
//! the lifetime stats between launches.

pub mod audio;
pub mod game;
pub mod settings;
pub mod stats;
pub mod ui;
