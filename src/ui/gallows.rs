//! The gallows picture that fills the right-hand panel.
//!
//! This is the original Java game's artwork: a `pole.jpg` gallows with six
//! victim frames stacked on top of it, one per wrong guess. The images are
//! baked into the binary, so there is still nothing to install alongside the
//! executable.

use std::sync::{Arc, LazyLock};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// The gallows backdrop, drawn behind every stage.
///
/// The stage panel it sits on is themed, so this wants to be a transparent
/// PNG. It is still the original opaque `pole.jpg`: swap the filename (and
/// `jpeg` for `png`) once `pole.png` lands and nothing else has to change.
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

/// The gallows, plus as much of the victim as `stage` has earned.
///
/// `stage` is [`crate::game::Game::wrong_guesses`] — 0 through 6 — and maps
/// one-to-one onto the original's seven drawing stages, 0 being the bare pole.
pub fn gallows(stage: usize) -> impl IntoElement {
    div()
        // `relative` makes this the box the stage image is positioned against.
        .relative()
        .w(px(GALLOWS_WIDTH))
        .h(px(GALLOWS_HEIGHT))
        .child(img(GALLOWS.clone()).size_full())
        .when(stage > 0, |this| {
            this.child(
                img(STAGES[(stage - 1).min(STAGES.len() - 1)].clone())
                    .absolute()
                    .left(px(STAGE_LEFT))
                    .top(px(STAGE_TOP))
                    .w(px(STAGE_WIDTH))
                    .h(px(STAGE_HEIGHT)),
            )
        })
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
