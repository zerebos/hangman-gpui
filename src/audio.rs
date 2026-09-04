//! The original game's two sound cues, behind the off-by-default `sound` feature.
//!
//! The Java version played `win.mp3` when you guessed the word and `loss.mp3`
//! when the drawing finished, and nothing at all when you gave up. This module
//! reproduces exactly that, and nothing else.
//!
//! There are **two** implementations of [`Audio`] below with identical public
//! signatures: a real one under `#[cfg(feature = "sound")]` and a zero-sized
//! do-nothing stub under `#[cfg(not(feature = "sound"))]`. That is the idiomatic
//! way to make an optional dependency optional: the conditional compilation is
//! confined to this one file, and every caller just writes
//! `self.audio.play_win()` with no `#[cfg]` at the call site. With the feature
//! off the stub's methods are empty, so the optimiser deletes them entirely.

#[cfg(feature = "sound")]
mod imp {
    use std::io::Cursor;

    use rodio::mixer::Mixer;
    use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

    /// The cues, baked into the binary like the word lists and the artwork.
    const WIN_MP3: &[u8] = include_bytes!("../assets/sounds/win.mp3");
    const LOSS_MP3: &[u8] = include_bytes!("../assets/sounds/loss.mp3");

    /// Owns the audio device for as long as the view that holds it lives.
    ///
    /// The `Option` is `None` when there is no usable sound device, in which
    /// case every method here is a no-op — a machine without working audio
    /// still gets a perfectly playable game.
    pub struct Audio(Option<Device>);

    struct Device {
        /// **Keep this field.** `MixerDeviceSink` has a `Drop` impl that closes
        /// the OS stream, and everything playing through its mixer goes silent
        /// the instant it drops. Storing only the `Mixer` is not enough: it is
        /// `Clone` and `Arc`-backed, so the code still compiles, still lets you
        /// `connect_new` a `Player`, and still plays nothing at all.
        _sink: MixerDeviceSink,
        mixer: Mixer,
    }

    impl Audio {
        /// Opens the default output device, or falls back to silence.
        ///
        /// rodio reports a missing or unusable device as an `Err` rather than
        /// panicking, and the variant varies by cause (a machine with no
        /// `/dev/snd` at all yields a config error, not `NoDevice`), so this
        /// deliberately matches every error the same way.
        pub fn new() -> Self {
            match DeviceSinkBuilder::open_default_sink() {
                Ok(sink) => {
                    let mixer = sink.mixer().clone();
                    Self(Some(Device { _sink: sink, mixer }))
                }
                Err(err) => {
                    eprintln!("hangman: no audio device, continuing silently: {err}");
                    Self(None)
                }
            }
        }

        /// Plays the win cue. Returns immediately; the clip finishes on rodio's
        /// own thread.
        pub fn play_win(&self) {
            self.play(WIN_MP3);
        }

        /// Plays the loss cue. Giving up is silent, as it was in the original.
        pub fn play_loss(&self) {
            self.play(LOSS_MP3);
        }

        fn play(&self, bytes: &'static [u8]) {
            let Some(device) = &self.0 else {
                return;
            };
            // A decode failure is no reason to spoil the game either.
            match Decoder::new(Cursor::new(bytes)) {
                Ok(decoder) => {
                    let player = Player::connect_new(&device.mixer);
                    player.append(decoder);
                    // Hands the player to the mixer so the clip outlives this
                    // handle instead of being cut off at the end of the call.
                    player.detach();
                }
                Err(err) => eprintln!("hangman: could not decode a sound cue: {err}"),
            }
        }
    }

    impl Default for Audio {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(not(feature = "sound"))]
mod imp {
    /// The silent stand-in used when the `sound` feature is off. Zero-sized,
    /// and every method compiles away to nothing.
    pub struct Audio;

    impl Audio {
        pub fn new() -> Self {
            Self
        }

        pub fn play_win(&self) {}

        pub fn play_loss(&self) {}
    }

    impl Default for Audio {
        fn default() -> Self {
            Self::new()
        }
    }
}

pub use imp::Audio;
