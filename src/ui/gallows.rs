//! The gallows drawing that fills the right-hand panel.
//!
//! The Java original shipped raster art: a `pole.jpg` gallows with six victim
//! frames painted on top of it. This port draws the same seven stages as vector
//! paths on a GPUI [`canvas`] instead, so there is no artwork to ship and the
//! figure scales with the window.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;

/// The coordinate system the drawing below is written in.
///
/// Everything is authored against this fixed box and then scaled uniformly to
/// fit whatever bounds the canvas is actually handed, so the drawing is
/// resolution independent and always keeps its proportions.
const DESIGN_WIDTH: f32 = 240.0;
const DESIGN_HEIGHT: f32 = 340.0;

/// Line thickness, in design units. One value for every stroke keeps the
/// gallows and the figure looking like they were drawn with the same pen.
const STROKE: f32 = 6.0;

/// Where the rope ends and the figure begins, in design units.
const FIGURE_X: f32 = 170.0;
/// Radius of the head.
const HEAD_RADIUS: f32 = 19.0;
/// Centre of the head; the rope stops exactly at the top of this circle.
const HEAD_CENTER: (f32, f32) = (FIGURE_X, 74.0);
/// Top and bottom of the torso.
const SHOULDERS: (f32, f32) = (FIGURE_X, 93.0);
const HIPS: (f32, f32) = (FIGURE_X, 185.0);
/// Where the arms leave the torso.
const ARMPITS: (f32, f32) = (FIGURE_X, 112.0);

/// The gallows, plus as much of the figure as `wrong_guesses` has earned.
///
/// `stage` is [`crate::game::Game::wrong_guesses`] — 0 through 6 — and maps
/// one-to-one onto the original's seven drawing stages.
pub fn gallows(stage: usize, cx: &App) -> impl IntoElement {
    // The paint closure runs outside of `render`, so the colours have to be
    // read from the theme now and moved into it.
    let frame = cx.theme().foreground;
    let figure = cx.theme().red;
    let stage = stage.min(6);

    canvas(
        // Nothing to measure before painting; the paint pass gets the bounds.
        |_, _, _| {},
        move |bounds, _, window, _| {
            let pen = Pen::fitted_to(bounds);

            // The gallows itself is always there.
            pen.line(window, (20.0, 322.0), (170.0, 322.0), frame); // base
            pen.line(window, (50.0, 322.0), (50.0, 18.0), frame); // post
            pen.line(window, (50.0, 18.0), (170.0, 18.0), frame); // crossbeam
            pen.line(window, (50.0, 70.0), (100.0, 18.0), frame); // corner brace
            pen.line(window, (FIGURE_X, 18.0), (FIGURE_X, 55.0), frame); // rope

            // ...and one more body part per wrong guess.
            if stage >= 1 {
                pen.circle(window, HEAD_CENTER, HEAD_RADIUS, figure);
            }
            if stage >= 2 {
                pen.line(window, SHOULDERS, HIPS, figure);
            }
            if stage >= 3 {
                pen.line(window, ARMPITS, (132.0, 152.0), figure);
            }
            if stage >= 4 {
                pen.line(window, ARMPITS, (208.0, 152.0), figure);
            }
            if stage >= 5 {
                pen.line(window, HIPS, (138.0, 240.0), figure);
            }
            if stage >= 6 {
                pen.line(window, HIPS, (202.0, 240.0), figure);
            }
        },
    )
    .size_full()
}

/// Maps design-space coordinates onto a canvas's real pixel bounds and draws
/// with them. GPUI paints in absolute window coordinates, so every point has to
/// be offset by the canvas origin as well as scaled.
#[derive(Clone, Copy)]
struct Pen {
    origin_x: f32,
    origin_y: f32,
    scale: f32,
}

impl Pen {
    /// Fit the design box into `bounds`, centred, without distorting it.
    fn fitted_to(bounds: Bounds<Pixels>) -> Self {
        let scale = (bounds.size.width.as_f32() / DESIGN_WIDTH)
            .min(bounds.size.height.as_f32() / DESIGN_HEIGHT)
            .max(0.01);

        Self {
            origin_x: bounds.origin.x.as_f32()
                + (bounds.size.width.as_f32() - DESIGN_WIDTH * scale) / 2.0,
            origin_y: bounds.origin.y.as_f32()
                + (bounds.size.height.as_f32() - DESIGN_HEIGHT * scale) / 2.0,
            scale,
        }
    }

    fn at(self, (x, y): (f32, f32)) -> Point<Pixels> {
        point(
            px(self.origin_x + x * self.scale),
            px(self.origin_y + y * self.scale),
        )
    }

    fn width(self) -> Pixels {
        px(STROKE * self.scale)
    }

    fn line(self, window: &mut Window, from: (f32, f32), to: (f32, f32), color: Hsla) {
        let mut builder = PathBuilder::stroke(self.width());
        builder.move_to(self.at(from));
        builder.line_to(self.at(to));
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }

    /// `PathBuilder` has no circle primitive, so use the standard four-cubic
    /// approximation. `K` is the magic constant that makes those four Béziers
    /// land within a fraction of a pixel of a true circle.
    fn circle(self, window: &mut Window, center: (f32, f32), radius: f32, color: Hsla) {
        const K: f32 = 0.552_284_8;

        let (cx, cy) = center;
        let (r, k) = (radius, radius * K);

        let mut builder = PathBuilder::stroke(self.width());
        builder.move_to(self.at((cx, cy - r)));
        builder.cubic_bezier_to(
            self.at((cx + r, cy)),
            self.at((cx + k, cy - r)),
            self.at((cx + r, cy - k)),
        );
        builder.cubic_bezier_to(
            self.at((cx, cy + r)),
            self.at((cx + r, cy + k)),
            self.at((cx + k, cy + r)),
        );
        builder.cubic_bezier_to(
            self.at((cx - r, cy)),
            self.at((cx - k, cy + r)),
            self.at((cx - r, cy + k)),
        );
        builder.cubic_bezier_to(
            self.at((cx, cy - r)),
            self.at((cx - r, cy - k)),
            self.at((cx - k, cy - r)),
        );
        builder.close();

        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}
