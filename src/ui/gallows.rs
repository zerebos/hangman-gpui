//! The gallows picture that fills the right-hand panel.
//!
//! This is the original Java game's artwork: a `pole.png` gallows with six
//! victim frames stacked on top of it, one per wrong guess. The images are
//! baked into the binary, so there is still nothing to install alongside the
//! executable.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// The gallows backdrop, drawn behind every stage.
///
/// The stage panel it sits on is themed, so this is a transparent PNG: the
/// panel's own colour shows through the artwork in both light and dark.
static GALLOWS: LazyLock<Arc<Image>> =
    LazyLock::new(|| png(include_bytes!("../../assets/images/pole.png")));

/// The six victim frames, in the order the original showed them.
static STAGES: LazyLock<[Arc<Image>; 6]> = LazyLock::new(|| {
    [
        png(include_bytes!("../../assets/images/head.png")),
        png(include_bytes!("../../assets/images/step2.png")),
        png(include_bytes!("../../assets/images/step3.png")),
        png(include_bytes!("../../assets/images/step4.png")),
        png(include_bytes!("../../assets/images/step5.png")),
        png(include_bytes!("../../assets/images/final.png")),
    ]
});

/// The gallows image's own pixel size, which is also the size of the frame the
/// stages are positioned inside.
const GALLOWS_WIDTH: f32 = 300.0;
const GALLOWS_HEIGHT: f32 = 350.0;

/// Every stage image is this size, and sits at this offset from the gallows'
/// top-left corner. The Java drew the gallows at x = 10 and the stage at x = 75
/// on a shared canvas (`HangmanPicture.java`), hence the 65 pixel gap here.
const STAGE_WIDTH: f32 = 125.0;
const STAGE_HEIGHT: f32 = 187.0;
const STAGE_LEFT: f32 = 65.0;
const STAGE_TOP: f32 = 90.0;

/// How long a new stage takes to fade in over the one before it. Long enough
/// to read as a stroke being drawn, short enough not to hold up the next guess.
const CROSS_FADE: Duration = Duration::from_millis(280);

/// The gallows, plus as much of the victim as `stage` has earned.
///
/// `stage` is [`crate::game::Game::wrong_guesses`] — 0 through 6 — and maps
/// one-to-one onto the original's seven drawing stages, 0 being the bare pole.
///
/// The frames are cumulative, so a new stage is drawn *over* the one before it
/// rather than replacing it: the previous frame stays put at full opacity while
/// the new one fades in on top, which reads as the missing limb appearing
/// instead of the whole victim blinking.
pub fn gallows(stage: usize) -> impl IntoElement {
    // The artwork runs out at `final.png`, and so does the game; clamp anyway
    // rather than index on that assumption.
    let stage = stage.min(STAGES.len());

    div()
        // `relative` makes this the box the stage image is positioned against.
        .relative()
        .w(px(GALLOWS_WIDTH))
        .h(px(GALLOWS_HEIGHT))
        .child(img(GALLOWS.clone()).size_full())
        .when(stage > 1, |this| this.child(frame(stage - 1)))
        .when(stage > 0, |this| {
            this.child(frame(stage).with_animation(
                // The stage number is *in the element id*, and that is what
                // makes this play again on every wrong guess. `with_animation`
                // keys its state on the id and starts the clock the frame that
                // id first appears; with a fixed id the fade would run once, at
                // startup, and every later stage would hard-swap as before. A
                // new stage is a new id, so it mounts fresh and runs again.
                ElementId::named_usize("gallows-stage", stage),
                Animation::new(CROSS_FADE).with_easing(ease_in_out),
                |this, delta| this.opacity(delta),
            ))
        })
}

/// One victim frame, positioned over the gallows. `stage` is 1-based, matching
/// [`gallows`]'s argument rather than the [`STAGES`] index.
fn frame(stage: usize) -> Img {
    img(STAGES[stage - 1].clone())
        .absolute()
        .left(px(STAGE_LEFT))
        .top(px(STAGE_TOP))
        .w(px(STAGE_WIDTH))
        .h(px(STAGE_HEIGHT))
}

/// Wrap bytes compiled into the binary as an image GPUI can draw.
///
/// `img()` wants an [`Arc`] because it hands the image to the renderer's decode
/// cache, which outlives any single frame. Note that this needs no
/// `AssetSource`: registering one is the usual route for images, but that is
/// only for looking a path up — bytes we already hold go straight into the
/// cache, keyed on their content hash.
fn embedded(format: ImageFormat, bytes: &[u8]) -> Arc<Image> {
    Arc::new(Image::from_bytes(format, bytes.to_vec()))
}

fn png(bytes: &[u8]) -> Arc<Image> {
    embedded(ImageFormat::Png, bytes)
}
