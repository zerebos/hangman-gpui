//! The gallows picture that fills the right-hand panel, drawn line by line.
//!
//! There is no artwork behind this any more: [`crate::gallows`] describes the
//! whole picture as polylines in a fixed design box, and this module turns them
//! into GPUI paths. Two pieces do the work.
//!
//! [`canvas`] is GPUI's escape hatch for drawing something the layout engine
//! has no element for. It is an element like any other — it takes part in
//! layout and gets a rectangle — but instead of children it hands you that
//! rectangle and the [`Window`], and whatever you paint there is the element.
//!
//! [`PathBuilder`] is how you paint a shape: you move and line your way around
//! it exactly as an SVG `<path>` would, then `build()` tessellates it into
//! triangles for the GPU. A builder is either a fill or a stroke; every line
//! here is a stroke, so the same builder draws a beam, an arm and a circle.
//!
//! Both of them work in *window* coordinates, not in coordinates relative to
//! the element, which is why every point goes through `bounds.origin` on its
//! way to the builder.

use std::time::Duration;

use gpui_kit::component::{ActiveTheme as _, Theme};
use gpui_kit::gpui;
use gpui_kit::*;

use crate::gallows as art;

/// How long the newest body part takes to draw itself on.
///
/// Long enough to read as a stroke being drawn rather than a frame being
/// swapped, short enough not to hold up the next guess.
const DRAW_ON: Duration = Duration::from_millis(300);

/// The gallows, plus as much of the victim as `wrong` wrong guesses out of
/// `budget` have earned.
///
/// `wrong` is [`crate::game::Game::wrong_guesses`] and `budget` is the number
/// of wrong guesses that game allows. Nothing here assumes the budget is six:
/// [`crate::gallows::parts_drawn`] spreads however many body parts there are
/// over however many guesses there are, so the figure is always finished on the
/// last one.
pub fn gallows(wrong: usize, budget: usize) -> impl IntoElement {
    div().w_full().h(px(art::DESIGN_HEIGHT)).child(
        Drawing {
            wrong,
            budget,
            progress: 1.0,
        }
        .with_animation(
            // The wrong-guess count is *in the element id*, and that is
            // what makes this play again on every wrong guess.
            // `with_animation` keys its state on the id and starts the
            // clock the frame that id first appears; with a fixed id the
            // draw-on would run once, at startup, and every later part
            // would simply snap into place.
            ElementId::named_usize("gallows-stage", wrong),
            Animation::new(DRAW_ON).with_easing(ease_in_out),
            |mut drawing, delta| {
                drawing.progress = delta;
                drawing
            },
        ),
    )
}

/// The picture at one moment: the state it is in, and how far through drawing
/// the newest part on it is.
///
/// This is a component rather than a bare [`canvas`] because `with_animation`
/// hands its animator the *element* and asks for one back. A canvas takes its
/// paint callback when it is built, so there is nothing to change afterwards;
/// a little struct that builds the canvas in `render` can have its `progress`
/// rewritten every frame instead.
#[derive(gpui::IntoElement)]
struct Drawing {
    wrong: usize,
    budget: usize,
    progress: f32,
}

impl RenderOnce for Drawing {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let Drawing {
            wrong,
            budget,
            progress,
        } = self;
        let palette = Palette::new(budget > 0 && wrong >= budget, cx.theme());

        canvas(
            // Nothing to measure before painting: the drawing is derived from
            // the bounds, and the bounds arrive with the paint callback too.
            |_, _, _| (),
            move |bounds, _, window, _| {
                let fit = art::fit(bounds.size.width.as_f32(), bounds.size.height.as_f32());
                if fit.scale <= 0.0 {
                    return;
                }
                for stroke in art::frame()
                    .into_iter()
                    .chain(art::figure(budget, wrong, progress))
                {
                    paint_stroke(&stroke, fit, bounds.origin, palette.of(stroke.ink), window);
                }
            },
        )
        .size_full()
    }
}

/// Paint one polyline as a stroked path.
fn paint_stroke(
    stroke: &art::Stroke,
    fit: art::Fit,
    origin: Point<Pixels>,
    color: Hsla,
    window: &mut Window,
) {
    if stroke.points.len() < 2 {
        return;
    }

    let mut builder = PathBuilder::stroke(px(fit.line_width(stroke.width)));
    let mut points = stroke.points.iter().map(|p| {
        let mapped = fit.map(*p);
        point(origin.x + px(mapped.x), origin.y + px(mapped.y))
    });

    let Some(first) = points.next() else {
        return;
    };
    builder.move_to(first);
    for next in points {
        builder.line_to(next);
    }
    if stroke.closed {
        builder.close();
    }

    // `build()` tessellates, which can fail on a degenerate path. A missing
    // limb is not worth a panic in a drawing.
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// The colour of each kind of line, taken from the theme so the drawing works
/// in both.
///
/// Three colours rather than one: the gallows is structure and stays quiet in
/// `muted_foreground`, the rope is the warm `yellow` that ties the two halves
/// of the picture together, and the victim is drawn in the window's own
/// `foreground` — the strongest contrast there is — because it is the thing you
/// are meant to be watching. When the last guess goes, the whole figure turns
/// `danger` and the face arrives with it.
#[derive(Clone, Copy)]
struct Palette {
    frame: Hsla,
    rope: Hsla,
    figure: Hsla,
}

impl Palette {
    fn new(lost: bool, theme: &Theme) -> Self {
        Self {
            frame: theme.muted_foreground,
            rope: theme.yellow,
            figure: if lost { theme.danger } else { theme.foreground },
        }
    }

    fn of(self, ink: art::Ink) -> Hsla {
        match ink {
            art::Ink::Frame => self.frame,
            art::Ink::Rope => self.rope,
            art::Ink::Figure | art::Ink::Face => self.figure,
        }
    }
}
