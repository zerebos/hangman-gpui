//! The gallows drawing, as plain geometry.
//!
//! Everything here is coordinates and arithmetic: no GPUI types, exactly like
//! [`crate::game`] and [`crate::settings`], so the whole drawing can be checked
//! by `cargo test` without opening a window. [`crate::ui`] turns what these
//! functions return into `PathBuilder` paths and paints them.
//!
//! The picture is described once, in a fixed [`DESIGN_WIDTH`] ×
//! [`DESIGN_HEIGHT`] box, and [`fit`] maps that box onto whatever rectangle the
//! window ends up giving it. Nothing is measured in real pixels until then,
//! which is what makes the drawing resolution-independent: it is re-tessellated
//! at the size it is drawn at instead of being a bitmap someone scaled.
//!
//! A drawing is a list of [`Stroke`]s, and a stroke is a polyline — even the
//! head, which is a circle flattened into one. That single representation is
//! what makes the "being drawn" animation trivial: [`Stroke::partial`] cuts a
//! polyline off at a fraction of its own length, so a limb grows out from the
//! shoulder and the head sweeps round from the rope, with no special case for
//! either.

/// The box every coordinate below is expressed in. It matches the size of the
/// artwork this replaced, so the panel around it did not have to move.
pub const DESIGN_WIDTH: f32 = 300.0;
/// The height of that box. See [`DESIGN_WIDTH`].
pub const DESIGN_HEIGHT: f32 = 350.0;

/// How many straight segments a full circle is flattened into. High enough that
/// the head reads as round at the sizes this is drawn at, low enough to stay
/// cheap to tessellate every frame of the draw-on animation.
const CIRCLE_SEGMENTS: usize = 48;
/// The same, for the short mouth arc, which is a fraction of a circle.
const ARC_SEGMENTS: usize = 12;

// ------------------------------------------------------------------ the frame
//
// The gallows itself: what is on screen before the first wrong guess and stays
// there for the rest of the game.

/// The ground the gallows stands on.
const GROUND_Y: f32 = 322.0;
const GROUND_X0: f32 = 34.0;
const GROUND_X1: f32 = 138.0;
/// The upright post, standing in the middle of that base.
const POST_X: f32 = 86.0;
/// The cross beam, and the far end of it the rope hangs from.
const BEAM_Y: f32 = 34.0;
const BEAM_X: f32 = 214.0;
/// The corner brace, a 45° strut between post and beam.
const BRACE_POST_Y: f32 = 90.0;
const BRACE_BEAM_X: f32 = 142.0;
/// The rope, from the beam down to the top of the head.
const ROPE_BOTTOM_Y: f32 = 74.0;

// ----------------------------------------------------------------- the victim

/// The head: a circle whose top is exactly where the rope stops.
const HEAD_X: f32 = BEAM_X;
const HEAD_RADIUS: f32 = 28.0;
const HEAD_Y: f32 = ROPE_BOTTOM_Y + HEAD_RADIUS;
/// The torso, from the chin to the hips.
const CHIN_Y: f32 = HEAD_Y + HEAD_RADIUS;
const HIP_Y: f32 = 210.0;
/// Where the arms leave the torso, and where they end.
const SHOULDER_Y: f32 = 152.0;
const HAND_Y: f32 = 196.0;
const ARM_REACH: f32 = 42.0;
/// Where the legs end.
const FOOT_Y: f32 = 268.0;
const LEG_REACH: f32 = 38.0;
/// The little ticks that finish the limbs off when the guess budget is large
/// enough to need more than the classic six parts.
const HAND_REACH: f32 = 12.0;
const HAND_DROP: f32 = 8.0;
const FOOT_REACH: f32 = 16.0;
const FOOT_DROP: f32 = 2.0;

/// The face drawn on at the end, when the drawing — and the game — is finished.
const EYE_Y: f32 = 94.0;
const EYE_OFFSET: f32 = 9.0;
const EYE_ARM: f32 = 5.0;
const MOUTH_Y: f32 = 120.0;
const MOUTH_RADIUS: f32 = 9.0;
const MOUTH_FROM: f32 = 200.0;
const MOUTH_TO: f32 = 340.0;

// ------------------------------------------------------------------- weights

/// How thick each kind of line is, in design units. [`Fit::line_width`] scales
/// these along with everything else.
const FRAME_WIDTH: f32 = 7.0;
const ROPE_WIDTH: f32 = 4.0;
const FIGURE_WIDTH: f32 = 6.0;
const FACE_WIDTH: f32 = 3.5;

/// A point in the design box. `y` grows downwards, as it does on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Distance from the left edge of the design box.
    pub x: f32,
    /// Distance from the top edge of the design box.
    pub y: f32,
}

impl Point {
    /// A point at `(x, y)`.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// How far it is from `other`.
    pub fn distance(self, other: Self) -> f32 {
        (other.x - self.x).hypot(other.y - self.y)
    }

    /// The point `t` of the way from `self` to `other`.
    pub fn lerp(self, other: Self, t: f32) -> Self {
        Self::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }
}

/// Which role a stroke plays, so the UI can give it a colour from the theme
/// rather than this module naming one it could only get right in one theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// The gallows: base, post, beam, brace.
    Frame,
    /// The rope hanging off the beam.
    Rope,
    /// The victim's head, torso and limbs.
    Figure,
    /// The face, drawn only once the figure is finished.
    Face,
}

/// One continuous line of the drawing.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    /// The polyline, in design-box coordinates. Fewer than two points means
    /// there is nothing to draw.
    pub points: Vec<Point>,
    /// Whether the last point joins back to the first. Only the finished head
    /// is closed; a half-drawn one is not.
    pub closed: bool,
    /// How thick the line is, in design units.
    pub width: f32,
    /// What the line is, for colouring.
    pub ink: Ink,
}

impl Stroke {
    fn new(points: Vec<Point>, width: f32, ink: Ink) -> Self {
        Self {
            points,
            closed: false,
            width,
            ink,
        }
    }

    fn line(from: Point, to: Point, width: f32, ink: Ink) -> Self {
        Self::new(vec![from, to], width, ink)
    }

    /// The total length of the polyline, following the closing segment too when
    /// the stroke is closed.
    pub fn length(&self) -> f32 {
        let mut total = 0.0;
        for pair in self.points.windows(2) {
            total += pair[0].distance(pair[1]);
        }
        if self.closed && self.points.len() > 2 {
            total += self.points[self.points.len() - 1].distance(self.points[0]);
        }
        total
    }

    /// The first `t` of this stroke, by length: what it looks like part-way
    /// through being drawn.
    ///
    /// `t <= 0` gives a stroke with no points at all — draw nothing — and
    /// `t >= 1` gives the stroke back unchanged, closing loop included. In
    /// between, the polyline is cut at the point `t` of the way along it, which
    /// turns a straight limb into a shorter limb and the head's circle into an
    /// arc sweeping round from the rope.
    pub fn partial(&self, t: f32) -> Self {
        if t >= 1.0 {
            return self.clone();
        }
        if t <= 0.0 || self.points.len() < 2 {
            return Self {
                points: Vec::new(),
                closed: false,
                ..*self
            };
        }

        let target = self.length() * t;
        let mut points = vec![self.points[0]];
        let mut walked = 0.0;

        // The closing segment is walked like any other, so a head cut at 0.99
        // stops just short of its start rather than snapping shut.
        let last = self.points.len() - 1;
        for i in 0..last + usize::from(self.closed && self.points.len() > 2) {
            let from = self.points[i];
            let to = self.points[(i + 1) % self.points.len()];
            let step = from.distance(to);
            if walked + step >= target {
                let along = if step > 0.0 {
                    (target - walked) / step
                } else {
                    0.0
                };
                points.push(from.lerp(to, along));
                break;
            }
            walked += step;
            points.push(to);
        }

        Self {
            points,
            closed: false,
            ..*self
        }
    }
}

/// One part of the victim, in the order the parts are drawn.
///
/// The first six are the classic hangman. The four after them are detail, and
/// only ever appear when the guess budget is generous enough to need more than
/// six stages — see [`part_count`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The head, drawn as a circle hanging off the rope.
    Head,
    /// The torso, from the chin to the hips.
    Torso,
    /// The arm on the viewer's left.
    LeftArm,
    /// The arm on the viewer's right.
    RightArm,
    /// The leg on the viewer's left.
    LeftLeg,
    /// The leg on the viewer's right.
    RightLeg,
    /// A tick finishing the left arm.
    LeftHand,
    /// A tick finishing the right arm.
    RightHand,
    /// A tick finishing the left leg.
    LeftFoot,
    /// A tick finishing the right leg.
    RightFoot,
}

/// Every part, in drawing order.
pub const PARTS: [Part; 10] = [
    Part::Head,
    Part::Torso,
    Part::LeftArm,
    Part::RightArm,
    Part::LeftLeg,
    Part::RightLeg,
    Part::LeftHand,
    Part::RightHand,
    Part::LeftFoot,
    Part::RightFoot,
];

/// The classic six: the fewest parts that still make a whole hangman.
pub const CORE_PARTS: usize = 6;

impl Part {
    /// Where this part is, and how thick it is drawn.
    pub fn stroke(self) -> Stroke {
        let shoulder = Point::new(HEAD_X, SHOULDER_Y);
        let hip = Point::new(HEAD_X, HIP_Y);
        let left_hand = Point::new(HEAD_X - ARM_REACH, HAND_Y);
        let right_hand = Point::new(HEAD_X + ARM_REACH, HAND_Y);
        let left_foot = Point::new(HEAD_X - LEG_REACH, FOOT_Y);
        let right_foot = Point::new(HEAD_X + LEG_REACH, FOOT_Y);

        match self {
            Part::Head => {
                let mut head = Stroke::new(
                    circle(Point::new(HEAD_X, HEAD_Y), HEAD_RADIUS),
                    FIGURE_WIDTH,
                    Ink::Figure,
                );
                head.closed = true;
                head
            }
            Part::Torso => Stroke::line(Point::new(HEAD_X, CHIN_Y), hip, FIGURE_WIDTH, Ink::Figure),
            Part::LeftArm => Stroke::line(shoulder, left_hand, FIGURE_WIDTH, Ink::Figure),
            Part::RightArm => Stroke::line(shoulder, right_hand, FIGURE_WIDTH, Ink::Figure),
            Part::LeftLeg => Stroke::line(hip, left_foot, FIGURE_WIDTH, Ink::Figure),
            Part::RightLeg => Stroke::line(hip, right_foot, FIGURE_WIDTH, Ink::Figure),
            Part::LeftHand => Stroke::line(
                left_hand,
                Point::new(left_hand.x - HAND_REACH, HAND_Y + HAND_DROP),
                FIGURE_WIDTH,
                Ink::Figure,
            ),
            Part::RightHand => Stroke::line(
                right_hand,
                Point::new(right_hand.x + HAND_REACH, HAND_Y + HAND_DROP),
                FIGURE_WIDTH,
                Ink::Figure,
            ),
            Part::LeftFoot => Stroke::line(
                left_foot,
                Point::new(left_foot.x - FOOT_REACH, FOOT_Y + FOOT_DROP),
                FIGURE_WIDTH,
                Ink::Figure,
            ),
            Part::RightFoot => Stroke::line(
                right_foot,
                Point::new(right_foot.x + FOOT_REACH, FOOT_Y + FOOT_DROP),
                FIGURE_WIDTH,
                Ink::Figure,
            ),
        }
    }
}

/// The gallows: the part of the picture that is there from the first frame.
pub fn frame() -> Vec<Stroke> {
    vec![
        Stroke::line(
            Point::new(GROUND_X0, GROUND_Y),
            Point::new(GROUND_X1, GROUND_Y),
            FRAME_WIDTH,
            Ink::Frame,
        ),
        Stroke::line(
            Point::new(POST_X, GROUND_Y),
            Point::new(POST_X, BEAM_Y),
            FRAME_WIDTH,
            Ink::Frame,
        ),
        Stroke::line(
            Point::new(POST_X, BEAM_Y),
            Point::new(BEAM_X, BEAM_Y),
            FRAME_WIDTH,
            Ink::Frame,
        ),
        Stroke::line(
            Point::new(POST_X, BRACE_POST_Y),
            Point::new(BRACE_BEAM_X, BEAM_Y),
            FRAME_WIDTH,
            Ink::Frame,
        ),
        Stroke::line(
            Point::new(BEAM_X, BEAM_Y),
            Point::new(BEAM_X, ROPE_BOTTOM_Y),
            ROPE_WIDTH,
            Ink::Rope,
        ),
    ]
}

/// The face: two crossed eyes and a frown, drawn only on a finished figure.
pub fn face() -> Vec<Stroke> {
    let mut strokes = Vec::with_capacity(5);
    for eye in [HEAD_X - EYE_OFFSET, HEAD_X + EYE_OFFSET] {
        strokes.push(Stroke::line(
            Point::new(eye - EYE_ARM, EYE_Y - EYE_ARM),
            Point::new(eye + EYE_ARM, EYE_Y + EYE_ARM),
            FACE_WIDTH,
            Ink::Face,
        ));
        strokes.push(Stroke::line(
            Point::new(eye + EYE_ARM, EYE_Y - EYE_ARM),
            Point::new(eye - EYE_ARM, EYE_Y + EYE_ARM),
            FACE_WIDTH,
            Ink::Face,
        ));
    }
    strokes.push(Stroke::new(
        arc(
            Point::new(HEAD_X, MOUTH_Y),
            MOUTH_RADIUS,
            MOUTH_FROM,
            MOUTH_TO,
            ARC_SEGMENTS,
        ),
        FACE_WIDTH,
        Ink::Face,
    ));
    strokes
}

/// How many parts the figure is made of when the game allows `budget` wrong
/// guesses.
///
/// Never fewer than the classic six, because a hangman missing a leg is not a
/// hangman, and never more than [`PARTS`] has to offer. A budget in between
/// gets one part per wrong guess; outside it, [`parts_drawn`] spreads the parts
/// over the guesses instead.
pub fn part_count(budget: usize) -> usize {
    budget.clamp(CORE_PARTS, PARTS.len())
}

/// How much of the figure `wrong` wrong guesses have earned, out of a budget of
/// `budget`.
///
/// The figure is always complete on the last guess, whatever the budget is: a
/// tight budget draws more than one part per guess, and a very generous one
/// repeats a stage or two near the end rather than inventing body parts.
pub fn parts_drawn(budget: usize, wrong: usize) -> usize {
    let total = part_count(budget);
    if budget == 0 {
        // No budget at all means the game is over the moment it starts; there
        // is no sensible ramp, so show everything or nothing.
        return if wrong == 0 { 0 } else { total };
    }
    (wrong.min(budget) * total).div_ceil(budget)
}

/// The victim as it stands after `wrong` wrong guesses out of `budget`, with
/// the parts the latest guess just earned drawn only `progress` of the way.
///
/// Parts from earlier guesses are always whole; passing `progress` of 1 gives
/// the settled picture, which is what a finished animation — or a machine with
/// reduced motion turned on — should show.
pub fn figure(budget: usize, wrong: usize, progress: f32) -> Vec<Stroke> {
    let drawn = parts_drawn(budget, wrong);
    let settled = parts_drawn(budget, wrong.saturating_sub(1));
    let progress = progress.clamp(0.0, 1.0);

    let mut strokes = Vec::with_capacity(drawn + 5);
    for (index, part) in PARTS.iter().take(drawn).enumerate() {
        let stroke = part.stroke();
        if index < settled {
            strokes.push(stroke);
        } else {
            push_partial(&mut strokes, stroke, progress);
        }
    }

    // The face is the full stop on the drawing, so it arrives with the stroke
    // that finishes it rather than as a stage of its own.
    if drawn > 0 && drawn == part_count(budget) {
        for stroke in face() {
            if settled == drawn {
                strokes.push(stroke);
            } else {
                push_partial(&mut strokes, stroke, progress);
            }
        }
    }

    strokes
}

fn push_partial(strokes: &mut Vec<Stroke>, stroke: Stroke, progress: f32) {
    let partial = stroke.partial(progress);
    if partial.points.len() >= 2 {
        strokes.push(partial);
    }
}

/// How the design box maps onto the rectangle the window gave the drawing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    /// How many real pixels one design unit is worth.
    pub scale: f32,
    /// Where the top-left of the scaled design box lands inside that rectangle.
    pub offset: Point,
}

impl Fit {
    /// Where `p` ends up, relative to the top-left of the rectangle.
    pub fn map(self, p: Point) -> Point {
        Point::new(
            self.offset.x + p.x * self.scale,
            self.offset.y + p.y * self.scale,
        )
    }

    /// A design-unit line width in real pixels, never thinner than a hairline —
    /// a stroke rounded down to nothing would simply disappear.
    pub fn line_width(self, width: f32) -> f32 {
        (width * self.scale).max(1.0)
    }
}

/// Fit the design box into a `width` × `height` rectangle: as large as it goes
/// without distorting it, and centred in whatever room is left over.
pub fn fit(width: f32, height: f32) -> Fit {
    let scale = (width / DESIGN_WIDTH).min(height / DESIGN_HEIGHT).max(0.0);
    Fit {
        scale,
        offset: Point::new(
            (width - DESIGN_WIDTH * scale) / 2.0,
            (height - DESIGN_HEIGHT * scale) / 2.0,
        ),
    }
}

/// A circle flattened into a polyline, starting at the top and going clockwise,
/// so the head draws away from the rope in both directions at once.
fn circle(center: Point, radius: f32) -> Vec<Point> {
    (0..CIRCLE_SEGMENTS)
        .map(|i| {
            let angle = -std::f32::consts::FRAC_PI_2
                + std::f32::consts::TAU * i as f32 / CIRCLE_SEGMENTS as f32;
            Point::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        })
        .collect()
}

/// Part of a circle, flattened the same way. Angles are in degrees, clockwise
/// from three o'clock.
fn arc(center: Point, radius: f32, from: f32, to: f32, segments: usize) -> Vec<Point> {
    let segments = segments.max(1);
    (0..=segments)
        .map(|i| {
            let angle = (from + (to - from) * i as f32 / segments as f32).to_radians();
            Point::new(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every stroke of a finished drawing at the largest budget the parts list
    /// can serve: the widest the picture ever gets.
    fn everything() -> Vec<Stroke> {
        let mut strokes = frame();
        strokes.extend(figure(PARTS.len(), PARTS.len(), 1.0));
        strokes
    }

    #[test]
    fn part_count_never_drops_below_the_classic_six() {
        for budget in 0..=CORE_PARTS {
            assert_eq!(part_count(budget), CORE_PARTS, "budget {budget}");
        }
    }

    #[test]
    fn part_count_follows_a_bigger_budget_up_to_the_parts_there_are() {
        assert_eq!(part_count(7), 7);
        assert_eq!(part_count(PARTS.len()), PARTS.len());
        assert_eq!(part_count(PARTS.len() + 5), PARTS.len());
    }

    #[test]
    fn the_classic_budget_draws_one_part_per_wrong_guess() {
        for wrong in 0..=CORE_PARTS {
            assert_eq!(parts_drawn(CORE_PARTS, wrong), wrong, "wrong {wrong}");
        }
    }

    #[test]
    fn nothing_is_drawn_before_the_first_wrong_guess() {
        for budget in 0..=12 {
            assert_eq!(parts_drawn(budget, 0), 0, "budget {budget}");
        }
    }

    #[test]
    fn the_figure_is_always_finished_on_the_last_guess() {
        for budget in 1..=12 {
            assert_eq!(
                parts_drawn(budget, budget),
                part_count(budget),
                "budget {budget}"
            );
        }
    }

    #[test]
    fn the_drawing_never_goes_backwards() {
        for budget in 1..=12 {
            for wrong in 1..=budget {
                assert!(
                    parts_drawn(budget, wrong) >= parts_drawn(budget, wrong - 1),
                    "budget {budget}, wrong {wrong}"
                );
            }
        }
    }

    #[test]
    fn a_tight_budget_draws_more_than_one_part_per_guess() {
        assert_eq!(
            (1..=4).map(|w| parts_drawn(4, w)).collect::<Vec<_>>(),
            vec![2, 3, 5, 6]
        );
    }

    #[test]
    fn a_generous_budget_reaches_for_the_detail_parts() {
        assert_eq!(
            (1..=8).map(|w| parts_drawn(8, w)).collect::<Vec<_>>(),
            vec![1, 2, 3, 4, 5, 6, 7, 8]
        );
    }

    #[test]
    fn more_guesses_than_parts_repeats_a_stage_rather_than_overrunning() {
        let drawn: Vec<_> = (1..=12).map(|w| parts_drawn(12, w)).collect();
        assert_eq!(*drawn.last().unwrap(), PARTS.len());
        assert!(drawn.windows(2).any(|pair| pair[0] == pair[1]));
        assert!(drawn.iter().all(|&n| n <= PARTS.len()));
    }

    #[test]
    fn a_budget_of_nothing_cannot_divide_by_zero() {
        assert_eq!(parts_drawn(0, 0), 0);
        assert_eq!(parts_drawn(0, 1), CORE_PARTS);
    }

    #[test]
    fn extra_wrong_guesses_are_clamped_to_the_budget() {
        assert_eq!(parts_drawn(6, 99), parts_drawn(6, 6));
    }

    #[test]
    fn a_settled_figure_is_one_stroke_per_part() {
        assert!(figure(6, 0, 1.0).is_empty());
        assert_eq!(figure(6, 3, 1.0).len(), 3);
        assert_eq!(figure(6, 5, 1.0).len(), 5);
    }

    #[test]
    fn the_newest_part_is_missing_at_the_start_of_its_animation() {
        assert_eq!(figure(6, 3, 0.0).len(), 2);
        assert_eq!(figure(6, 3, 0.5).len(), 3);
    }

    #[test]
    fn the_face_arrives_only_with_the_finished_figure() {
        assert!(
            figure(6, 5, 1.0)
                .iter()
                .all(|stroke| stroke.ink != Ink::Face)
        );
        assert_eq!(figure(6, 6, 1.0).len(), CORE_PARTS + face().len());
    }

    #[test]
    fn the_face_is_drawn_on_with_the_stroke_that_finishes_the_figure() {
        let mid = figure(6, 6, 0.5);
        assert_eq!(mid.len(), CORE_PARTS + face().len());
        let mouth = mid.last().unwrap();
        assert!(mouth.length() < face().last().unwrap().length());
    }

    #[test]
    fn a_half_drawn_line_stops_at_its_midpoint() {
        let stroke = Stroke::line(
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            1.0,
            Ink::Figure,
        );
        let half = stroke.partial(0.5);
        assert_eq!(
            half.points,
            vec![Point::new(0.0, 0.0), Point::new(5.0, 0.0)]
        );
    }

    #[test]
    fn partial_at_the_ends_is_all_or_nothing() {
        let head = Part::Head.stroke();
        assert!(head.partial(0.0).points.is_empty());
        assert!(head.partial(-1.0).points.is_empty());
        assert_eq!(head.partial(1.0), head);
        assert_eq!(head.partial(2.0), head);
    }

    #[test]
    fn a_half_drawn_head_is_an_open_arc_of_about_half_the_circle() {
        let head = Part::Head.stroke();
        assert!(head.closed);
        let half = head.partial(0.5);
        assert!(!half.closed);
        assert!((half.length() / head.length() - 0.5).abs() < 0.01);
    }

    #[test]
    fn a_closed_head_measures_the_whole_way_round() {
        let head = Part::Head.stroke();
        let circumference = std::f32::consts::TAU * HEAD_RADIUS;
        // Flattened into straight segments it is a hair short of a true circle.
        assert!((head.length() - circumference).abs() < circumference * 0.01);
    }

    #[test]
    fn every_stroke_has_somewhere_to_go() {
        for stroke in everything() {
            assert!(stroke.points.len() >= 2, "{stroke:?}");
            assert!(stroke.length() > 0.0, "{stroke:?}");
            assert!(stroke.width > 0.0, "{stroke:?}");
        }
    }

    #[test]
    fn the_whole_drawing_stays_inside_the_design_box() {
        for stroke in everything() {
            let margin = stroke.width / 2.0;
            for point in &stroke.points {
                assert!(point.x - margin >= 0.0, "{point:?} off the left");
                assert!(point.x + margin <= DESIGN_WIDTH, "{point:?} off the right");
                assert!(point.y - margin >= 0.0, "{point:?} off the top");
                assert!(
                    point.y + margin <= DESIGN_HEIGHT,
                    "{point:?} off the bottom"
                );
            }
        }
    }

    #[test]
    fn the_drawing_is_roughly_centred_in_its_box() {
        let (mut left, mut right) = (f32::MAX, f32::MIN);
        for stroke in everything() {
            for point in &stroke.points {
                left = left.min(point.x);
                right = right.max(point.x);
            }
        }
        assert!(((left + right) / 2.0 - DESIGN_WIDTH / 2.0).abs() < 5.0);
    }

    #[test]
    fn the_body_hangs_off_the_rope_without_a_gap() {
        let rope = frame().pop().expect("the rope is the last frame stroke");
        assert_eq!(rope.ink, Ink::Rope);
        let rope_end = *rope.points.last().unwrap();
        let head_top = Part::Head.stroke().points[0];
        assert!(rope_end.distance(head_top) < 0.01);
    }

    #[test]
    fn the_limbs_start_on_the_body() {
        let torso = Part::Torso.stroke();
        let (chin, hip) = (torso.points[0], torso.points[1]);
        assert!(chin.distance(Point::new(HEAD_X, CHIN_Y)) < 0.01);
        for arm in [Part::LeftArm, Part::RightArm] {
            let shoulder = arm.stroke().points[0];
            assert_eq!(shoulder.x, chin.x);
            assert!(shoulder.y > chin.y && shoulder.y < hip.y);
        }
        for leg in [Part::LeftLeg, Part::RightLeg] {
            assert_eq!(leg.stroke().points[0], hip);
        }
    }

    #[test]
    fn the_detail_parts_hang_off_the_limbs_they_finish() {
        for (limb, detail) in [
            (Part::LeftArm, Part::LeftHand),
            (Part::RightArm, Part::RightHand),
            (Part::LeftLeg, Part::LeftFoot),
            (Part::RightLeg, Part::RightFoot),
        ] {
            let end = *limb.stroke().points.last().unwrap();
            assert_eq!(detail.stroke().points[0], end);
        }
    }

    #[test]
    fn the_face_fits_inside_the_head() {
        let head = Point::new(HEAD_X, HEAD_Y);
        for stroke in face() {
            for point in &stroke.points {
                assert!(point.distance(head) < HEAD_RADIUS, "{point:?}");
            }
        }
    }

    #[test]
    fn a_box_the_size_of_the_design_needs_no_scaling() {
        let fit = fit(DESIGN_WIDTH, DESIGN_HEIGHT);
        assert_eq!(fit.scale, 1.0);
        assert_eq!(fit.offset, Point::new(0.0, 0.0));
        assert_eq!(fit.map(Point::new(7.0, 9.0)), Point::new(7.0, 9.0));
    }

    #[test]
    fn a_bigger_box_scales_the_drawing_up_whole() {
        let fit = fit(DESIGN_WIDTH * 2.0, DESIGN_HEIGHT * 2.0);
        assert_eq!(fit.scale, 2.0);
        assert_eq!(fit.map(Point::new(10.0, 20.0)), Point::new(20.0, 40.0));
        assert_eq!(fit.line_width(3.0), 6.0);
    }

    #[test]
    fn spare_width_is_split_either_side_rather_than_stretched() {
        let fit = fit(DESIGN_WIDTH + 100.0, DESIGN_HEIGHT);
        assert_eq!(fit.scale, 1.0);
        assert_eq!(fit.offset, Point::new(50.0, 0.0));
    }

    #[test]
    fn spare_height_is_split_above_and_below() {
        let fit = fit(DESIGN_WIDTH, DESIGN_HEIGHT + 60.0);
        assert_eq!(fit.scale, 1.0);
        assert_eq!(fit.offset, Point::new(0.0, 30.0));
    }

    #[test]
    fn a_shrunken_drawing_keeps_hairline_strokes_visible() {
        let fit = fit(DESIGN_WIDTH / 10.0, DESIGN_HEIGHT / 10.0);
        assert!((fit.scale - 0.1).abs() < 0.001);
        assert_eq!(fit.line_width(FACE_WIDTH), 1.0);
    }

    #[test]
    fn an_empty_box_asks_for_nothing_impossible() {
        let fit = fit(0.0, 0.0);
        assert_eq!(fit.scale, 0.0);
        assert_eq!(fit.line_width(FRAME_WIDTH), 1.0);
    }
}
