//! A small line-icon set, drawn as vectors with the egui painter.
//!
//! Every icon is designed on a 24×24 grid with a ~1.75 unit stroke, round caps
//! and round joins, so they stay crisp at any size and share one visual weight.
//! No font or texture is involved, so there are no missing-glyph boxes.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    // Tools
    Cursor,
    SpawnPlanet,
    Brush,
    Comet,
    // Time
    Play,
    Pause,
    Slower,
    Faster,
    Clock,
    // View & app
    Orbit,
    Tag,
    Lens,
    Eye,
    EyeOff,
    Sun,
    Sparkles,
    Galaxy,
    Gear,
    Atom,
    Help,
    Close,
    Plus,
    ChevronDown,
    ChevronUp,
    Terminal,
    Activity,
    Particles,
    Keyboard,
    // Bodies
    Star,
    Planet,
    Globe,
    Moon,
    Rocky,
    BlackHole,
    NeutronStar,
    GasGiant,
    IceGiant,
    Volcano,
    Snowflake,
    Dunes,
    Cloud,
    Asteroid,
    Binary,
    Disk,
    Empty,
    // Terraform brushes
    Raise,
    Lower,
    Crater,
    Waves,
    Flame,
    Tree,
    Mountain,
    Eraser,
    // Actions
    Target,
    Focus,
    CircleOrbit,
    Trash,
    Reset,
    Dice,
    Rocket,
    Supernova,
    Implode,
    Burst,
    Flyby,
    Warning,
    // Exploring
    Astronaut,
    Thermometer,
    Download,
    Seat,
    // Saves and sound
    Save,
    Folder,
    Speaker,
    SpeakerOff,
    Music,
    Monitor,
}

impl Icon {
    pub const ALL: [Icon; 75] = [
        Icon::Cursor,
        Icon::SpawnPlanet,
        Icon::Brush,
        Icon::Comet,
        Icon::Play,
        Icon::Pause,
        Icon::Slower,
        Icon::Faster,
        Icon::Clock,
        Icon::Orbit,
        Icon::Tag,
        Icon::Lens,
        Icon::Eye,
        Icon::EyeOff,
        Icon::Sun,
        Icon::Sparkles,
        Icon::Galaxy,
        Icon::Gear,
        Icon::Atom,
        Icon::Help,
        Icon::Close,
        Icon::Plus,
        Icon::ChevronDown,
        Icon::ChevronUp,
        Icon::Terminal,
        Icon::Activity,
        Icon::Particles,
        Icon::Keyboard,
        Icon::Star,
        Icon::Planet,
        Icon::Globe,
        Icon::Moon,
        Icon::Rocky,
        Icon::BlackHole,
        Icon::NeutronStar,
        Icon::GasGiant,
        Icon::IceGiant,
        Icon::Volcano,
        Icon::Snowflake,
        Icon::Dunes,
        Icon::Cloud,
        Icon::Asteroid,
        Icon::Binary,
        Icon::Disk,
        Icon::Empty,
        Icon::Raise,
        Icon::Lower,
        Icon::Crater,
        Icon::Waves,
        Icon::Flame,
        Icon::Tree,
        Icon::Mountain,
        Icon::Eraser,
        Icon::Target,
        Icon::Focus,
        Icon::CircleOrbit,
        Icon::Trash,
        Icon::Reset,
        Icon::Dice,
        Icon::Rocket,
        Icon::Supernova,
        Icon::Implode,
        Icon::Burst,
        Icon::Flyby,
        Icon::Warning,
        Icon::Astronaut,
        Icon::Thermometer,
        Icon::Download,
        Icon::Seat,
        Icon::Save,
        Icon::Folder,
        Icon::Speaker,
        Icon::SpeakerOff,
        Icon::Music,
        Icon::Monitor,
    ];

    pub fn name(self) -> String {
        format!("{self:?}")
    }

    /// Draw the icon centred in `rect` (it uses the largest square that fits).
    pub fn paint(self, painter: &Painter, rect: Rect, color: Color32) {
        let size = rect.width().min(rect.height());
        let pen = Pen::new(painter, rect.center(), size, color);
        draw(self, &pen);
    }
}

/// Maps 24-unit icon coordinates to screen points and draws strokes with round caps/joins.
struct Pen<'a> {
    painter: &'a Painter,
    origin: Pos2,
    scale: f32,
    width: f32,
    color: Color32,
}

type P = (f32, f32);

impl<'a> Pen<'a> {
    fn new(painter: &'a Painter, center: Pos2, size: f32, color: Color32) -> Self {
        let scale = size / 24.0;
        // Slightly heavier at small sizes so thin lines don't wash out.
        let width = (1.75 * scale).max(1.25);
        Pen { painter, origin: center - egui::vec2(12.0, 12.0) * scale, scale, width, color }
    }

    fn pt(&self, (x, y): P) -> Pos2 {
        self.origin + egui::vec2(x, y) * self.scale
    }

    fn stroke(&self) -> Stroke {
        Stroke::new(self.width, self.color)
    }

    fn cap(&self, p: Pos2) {
        self.painter.circle_filled(p, self.width * 0.5, self.color);
    }

    /// Open polyline through grid points; every vertex gets a round join.
    fn line(&self, pts: &[P]) {
        let pts: Vec<Pos2> = pts.iter().map(|p| self.pt(*p)).collect();
        self.screen_line(&pts, true);
    }

    fn screen_line(&self, pts: &[Pos2], round_every_vertex: bool) {
        if pts.len() < 2 {
            return;
        }
        if round_every_vertex {
            for p in pts {
                self.cap(*p);
            }
        } else {
            self.cap(pts[0]);
            self.cap(pts[pts.len() - 1]);
        }
        self.painter.add(Shape::line(pts.to_vec(), self.stroke()));
    }

    /// Closed polygon outline.
    fn closed(&self, pts: &[P]) {
        let pts: Vec<Pos2> = pts.iter().map(|p| self.pt(*p)).collect();
        for p in &pts {
            self.cap(*p);
        }
        self.painter.add(Shape::closed_line(pts, self.stroke()));
    }

    fn circle(&self, c: P, r: f32) {
        self.painter.circle_stroke(self.pt(c), r * self.scale, self.stroke());
    }

    fn dot(&self, c: P, r: f32) {
        self.painter.circle_filled(self.pt(c), r * self.scale, self.color);
    }

    /// Points along an ellipse (`rot` and angles in degrees, y down, 0° = +x).
    fn ellipse_pts(&self, c: P, rx: f32, ry: f32, rot: f32, a0: f32, a1: f32) -> Vec<P> {
        let n = (((a1 - a0).abs() / 6.0).ceil() as usize).max(4);
        let (sr, cr) = rot.to_radians().sin_cos();
        (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                let (x, y) = (rx * a.cos(), ry * a.sin());
                (c.0 + x * cr - y * sr, c.1 + x * sr + y * cr)
            })
            .collect()
    }

    fn arc(&self, c: P, r: f32, a0: f32, a1: f32) {
        self.ellipse(c, r, r, 0.0, a0, a1);
    }

    fn ellipse(&self, c: P, rx: f32, ry: f32, rot: f32, a0: f32, a1: f32) {
        let pts: Vec<Pos2> = self.ellipse_pts(c, rx, ry, rot, a0, a1).into_iter().map(|p| self.pt(p)).collect();
        self.screen_line(&pts, false);
    }

    /// Full ellipse, hiding the parts that pass behind a disc of radius `hole` at `c`
    /// on the far side (negative-angle half), like a ring around a planet.
    fn ring(&self, c: P, rx: f32, ry: f32, rot: f32, hole: f32) {
        let pts = self.ellipse_pts(c, rx, ry, rot, -180.0, 180.0);
        let n = pts.len();
        let mut seg: Vec<Pos2> = Vec::new();
        for (i, p) in pts.iter().enumerate() {
            let a = -180.0 + 360.0 * i as f32 / (n - 1) as f32;
            let behind = a < 0.0 && a > -180.0;
            let d = ((p.0 - c.0).powi(2) + (p.1 - c.1).powi(2)).sqrt();
            if behind && d < hole {
                self.screen_line(&seg, false);
                seg.clear();
            } else {
                seg.push(self.pt(*p));
            }
        }
        self.screen_line(&seg, false);
    }

    fn bezier_pts(p0: P, p1: P, p2: P, p3: P) -> Vec<P> {
        (0..=16)
            .map(|i| {
                let t = i as f32 / 16.0;
                let u = 1.0 - t;
                let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                (a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1)
            })
            .collect()
    }

    fn bezier(&self, p0: P, p1: P, p2: P, p3: P) {
        let pts: Vec<Pos2> = Self::bezier_pts(p0, p1, p2, p3).into_iter().map(|p| self.pt(p)).collect();
        self.screen_line(&pts, false);
    }

    /// A path built from straight and curved pieces, stroked as one line.
    fn path(&self, pieces: &[Vec<P>], closed: bool) {
        let mut pts: Vec<Pos2> = Vec::new();
        for piece in pieces {
            for p in piece {
                let q = self.pt(*p);
                if pts.last().is_none_or(|l| l.distance(q) > 0.01) {
                    pts.push(q);
                }
            }
        }
        if closed {
            self.painter.add(Shape::closed_line(pts, self.stroke()));
        } else {
            self.screen_line(&pts, false);
        }
    }

    /// Four-point sparkle with concave sides.
    fn sparkle(&self, c: P, r: f32) {
        let k = r * 0.18;
        let pts = [
            (c.0, c.1 - r),
            (c.0 + k, c.1 - k),
            (c.0 + r, c.1),
            (c.0 + k, c.1 + k),
            (c.0, c.1 + r),
            (c.0 - k, c.1 + k),
            (c.0 - r, c.1),
            (c.0 - k, c.1 - k),
        ];
        self.closed(&pts);
    }

    /// Radial line segments, `n` evenly spaced.
    fn rays(&self, c: P, n: usize, r0: f32, r1: f32, offset: f32) {
        for i in 0..n {
            let a = (offset + i as f32 * 360.0 / n as f32).to_radians();
            let (s, co) = a.sin_cos();
            self.line(&[(c.0 + co * r0, c.1 + s * r0), (c.0 + co * r1, c.1 + s * r1)]);
        }
    }

    fn wave(&self, y: f32, x0: f32, x1: f32, amp: f32) {
        let pts: Vec<P> = (0..=24)
            .map(|i| {
                let t = i as f32 / 24.0;
                (x0 + (x1 - x0) * t, y + amp * (t * std::f32::consts::TAU * 1.5).sin())
            })
            .collect();
        let pts: Vec<Pos2> = pts.into_iter().map(|p| self.pt(p)).collect();
        self.screen_line(&pts, false);
    }
}

fn draw(icon: Icon, p: &Pen) {
    use Icon::*;
    match icon {
        // ── Tools ───────────────────────────────────────────────
        Cursor => {
            p.closed(&[(5.0, 4.0), (20.0, 10.5), (13.5, 12.8), (10.8, 19.8)]);
        }
        SpawnPlanet => {
            p.circle((10.0, 14.0), 6.5);
            p.ellipse((10.0, 14.0), 3.0, 6.5, 0.0, -90.0, 90.0);
            p.line(&[(19.0, 2.5), (19.0, 8.5)]);
            p.line(&[(16.0, 5.5), (22.0, 5.5)]);
        }
        Brush => {
            p.line(&[(10.0, 17.0), (20.6, 6.4)]);
            p.line(&[(7.0, 14.0), (17.6, 3.4)]);
            p.arc((19.1, 4.9), 2.1, -135.0, 45.0);
            p.line(&[(11.0, 10.0), (14.0, 13.0)]);
            p.path(
                &[
                    vec![(6.5, 21.0)],
                    p.ellipse_pts((6.5, 17.5), 3.5, 3.5, 0.0, 90.0, -180.0),
                    Pen::bezier_pts((3.0, 17.5), (3.0, 18.6), (2.6, 19.4), (2.3, 19.8)),
                    vec![(3.0, 21.0), (6.5, 21.0)],
                ],
                false,
            );
        }
        Comet => {
            p.circle((15.5, 15.5), 5.0);
            p.line(&[(3.0, 3.0), (11.5, 11.5)]);
            p.line(&[(3.5, 9.5), (10.6, 13.8)]);
            p.line(&[(9.5, 3.5), (13.8, 10.6)]);
        }

        // ── Time ────────────────────────────────────────────────
        Play => p.closed(&[(7.5, 4.5), (19.5, 12.0), (7.5, 19.5)]),
        Pause => {
            p.line(&[(8.5, 5.0), (8.5, 19.0)]);
            p.line(&[(15.5, 5.0), (15.5, 19.0)]);
        }
        Slower => {
            p.line(&[(12.0, 6.0), (6.0, 12.0), (12.0, 18.0)]);
            p.line(&[(18.5, 6.0), (12.5, 12.0), (18.5, 18.0)]);
        }
        Faster => {
            p.line(&[(12.0, 6.0), (18.0, 12.0), (12.0, 18.0)]);
            p.line(&[(5.5, 6.0), (11.5, 12.0), (5.5, 18.0)]);
        }
        Clock => {
            p.circle((12.0, 12.0), 9.0);
            p.line(&[(12.0, 7.0), (12.0, 12.0), (15.5, 14.0)]);
        }

        // ── View & app ──────────────────────────────────────────
        Orbit => {
            p.ellipse((12.0, 12.0), 10.0, 5.0, -20.0, 0.0, 360.0);
            p.circle((12.0, 12.0), 2.5);
            let q = p.ellipse_pts((12.0, 12.0), 10.0, 5.0, -20.0, 40.0, 40.0)[0];
            p.dot(q, 2.2);
        }
        Tag => {
            p.closed(&[(3.0, 3.0), (11.0, 3.0), (21.0, 13.0), (13.0, 21.0), (3.0, 11.0)]);
            p.dot((7.5, 7.5), 1.4);
        }
        Lens => {
            p.circle((12.0, 12.0), 9.5);
            p.arc((12.0, 12.0), 5.0, -160.0, -20.0);
            p.arc((12.0, 12.0), 5.0, 20.0, 160.0);
            p.dot((12.0, 12.0), 1.8);
        }
        Eye => {
            p.bezier((2.0, 12.0), (5.5, 5.5), (18.5, 5.5), (22.0, 12.0));
            p.bezier((2.0, 12.0), (5.5, 18.5), (18.5, 18.5), (22.0, 12.0));
            p.circle((12.0, 12.0), 3.0);
        }
        EyeOff => {
            p.bezier((2.0, 12.0), (5.5, 5.5), (18.5, 5.5), (22.0, 12.0));
            p.bezier((2.0, 12.0), (5.5, 18.5), (18.5, 18.5), (22.0, 12.0));
            p.circle((12.0, 12.0), 3.0);
            p.line(&[(3.5, 3.5), (20.5, 20.5)]);
        }
        Sun => {
            p.circle((12.0, 12.0), 4.0);
            p.rays((12.0, 12.0), 8, 7.0, 9.5, -90.0);
        }
        Sparkles => {
            p.sparkle((10.0, 13.0), 8.0);
            p.line(&[(19.0, 2.5), (19.0, 7.5)]);
            p.line(&[(16.5, 5.0), (21.5, 5.0)]);
            p.dot((20.0, 19.0), 1.2);
        }
        Galaxy => {
            for arm in [0.0f32, 180.0] {
                let pts: Vec<P> = (0..=40)
                    .map(|i| {
                        let t = i as f32 / 40.0;
                        let a = (arm + t * 300.0).to_radians();
                        let r = 1.8 + t * 8.2;
                        (12.0 + r * a.cos(), 12.0 + r * 0.8 * a.sin())
                    })
                    .collect();
                p.path(&[pts], false);
            }
            p.dot((12.0, 12.0), 1.6);
        }
        Gear => {
            let mut pts = Vec::new();
            for i in 0..8 {
                let a = i as f32 * 45.0;
                for (r, da) in [(7.0, -16.0), (9.8, -9.0), (9.8, 9.0), (7.0, 16.0)] {
                    let t = (a + da).to_radians();
                    pts.push((12.0 + r * t.cos(), 12.0 + r * t.sin()));
                }
            }
            p.closed(&pts);
            p.circle((12.0, 12.0), 3.0);
        }
        Atom => {
            // Nucleus with three crossed electron orbits.
            for rot in [0.0, 60.0, 120.0] {
                p.ellipse((12.0, 12.0), 10.0, 3.8, rot, 0.0, 360.0);
            }
            p.dot((12.0, 12.0), 2.0);
        }
        Help => {
            p.circle((12.0, 12.0), 9.5);
            p.path(&[p.ellipse_pts((12.0, 9.5), 2.8, 2.8, 0.0, -180.0, 50.0), vec![(12.0, 13.6)]], false);
            p.dot((12.0, 17.0), 1.2);
        }
        Close => {
            p.line(&[(6.0, 6.0), (18.0, 18.0)]);
            p.line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Plus => {
            p.line(&[(12.0, 5.0), (12.0, 19.0)]);
            p.line(&[(5.0, 12.0), (19.0, 12.0)]);
        }
        ChevronDown => p.line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
        ChevronUp => p.line(&[(6.0, 15.0), (12.0, 9.0), (18.0, 15.0)]),
        Terminal => {
            p.closed(&[(2.5, 4.5), (21.5, 4.5), (21.5, 19.5), (2.5, 19.5)]);
            p.line(&[(7.0, 9.5), (10.0, 12.0), (7.0, 14.5)]);
            p.line(&[(12.5, 15.0), (17.0, 15.0)]);
        }
        Activity => p.line(&[(2.0, 12.0), (6.0, 12.0), (9.0, 4.0), (15.0, 20.0), (18.0, 12.0), (22.0, 12.0)]),
        Particles => {
            for (c, r) in [((6.0, 7.0), 1.6), ((14.0, 5.0), 1.2), ((19.0, 10.0), 1.8), ((10.0, 13.0), 2.0), ((5.0, 18.0), 1.2), ((16.0, 18.0), 1.5)] {
                p.dot(c, r);
            }
        }
        Keyboard => {
            p.closed(&[(2.5, 6.0), (21.5, 6.0), (21.5, 18.0), (2.5, 18.0)]);
            for x in [6.5, 10.0, 13.5, 17.0] {
                p.dot((x, 10.0), 1.0);
            }
            p.line(&[(7.5, 14.5), (16.5, 14.5)]);
        }

        // ── Bodies ──────────────────────────────────────────────
        Star => {
            p.circle((12.0, 12.0), 5.0);
            p.rays((12.0, 12.0), 8, 8.0, 10.5, -90.0);
        }
        Planet => {
            p.circle((12.0, 12.0), 6.0);
            p.ring((12.0, 12.0), 11.0, 3.6, -22.0, 6.9);
        }
        Globe => {
            p.circle((12.0, 12.0), 9.5);
            p.ellipse((12.0, 12.0), 4.2, 9.5, 0.0, 0.0, 360.0);
            p.line(&[(2.5, 12.0), (21.5, 12.0)]);
        }
        Moon => {
            // Two open arcs meeting at round caps: a closed path would spike at the sharp tips.
            p.arc((12.0, 12.0), 9.0, 60.0, 300.0);
            p.arc((22.0, 12.0), 9.54, 125.0, 235.0);
        }
        Rocky => {
            p.circle((12.0, 12.0), 9.5);
            p.circle((9.0, 9.0), 2.2);
            p.circle((15.5, 14.5), 3.0);
            p.dot((8.5, 15.5), 1.0);
        }
        BlackHole => {
            p.dot((12.0, 12.0), 3.6);
            p.ring((12.0, 12.0), 10.5, 3.6, -12.0, 5.2);
            p.arc((12.0, 12.0), 6.6, -165.0, -15.0);
        }
        NeutronStar => {
            p.circle((12.0, 12.0), 3.0);
            p.line(&[(9.5, 8.8), (4.0, 2.5)]);
            p.line(&[(8.8, 9.5), (2.5, 4.0)]);
            p.line(&[(14.5, 15.2), (20.0, 21.5)]);
            p.line(&[(15.2, 14.5), (21.5, 20.0)]);
            p.ellipse((12.0, 12.0), 8.0, 3.0, 45.0, 0.0, 360.0);
        }
        GasGiant => {
            p.circle((12.0, 12.0), 9.5);
            for (y, dy) in [(8.0, 4.0f32), (16.0, 4.0)] {
                let hw = (9.5f32.powi(2) - dy.powi(2)).sqrt();
                p.line(&[(12.0 - hw, y), (12.0 + hw, y)]);
            }
            p.ellipse((14.0, 12.0), 2.5, 1.4, 0.0, 0.0, 360.0);
        }
        IceGiant => {
            p.circle((12.0, 12.0), 6.5);
            p.ring((12.0, 12.0), 3.4, 11.0, -18.0, 7.2);
        }
        Volcano => {
            p.line(&[(2.0, 21.0), (9.0, 10.0), (15.0, 10.0), (22.0, 21.0)]);
            p.line(&[(2.0, 21.0), (22.0, 21.0)]);
            p.line(&[(9.0, 10.0), (11.0, 12.5), (13.0, 11.0), (15.0, 10.0)]);
            p.line(&[(12.0, 7.0), (12.0, 3.0)]);
            p.line(&[(8.5, 7.0), (6.5, 4.0)]);
            p.line(&[(15.5, 7.0), (17.5, 4.0)]);
        }
        Snowflake => {
            for i in 0..3 {
                let a = (90.0 + i as f32 * 60.0f32).to_radians();
                let (s, c) = a.sin_cos();
                p.line(&[(12.0 - c * 10.0, 12.0 - s * 10.0), (12.0 + c * 10.0, 12.0 + s * 10.0)]);
            }
            for i in 0..6 {
                let a = (90.0 + i as f32 * 60.0f32).to_radians();
                let (s, c) = a.sin_cos();
                let (bx, by) = (12.0 + c * 6.5, 12.0 + s * 6.5);
                for side in [-1.0f32, 1.0] {
                    let b = a + side * 0.75;
                    let (bs, bc) = b.sin_cos();
                    p.line(&[(bx, by), (bx + bc * 3.0, by + bs * 3.0)]);
                }
            }
        }
        Dunes => {
            p.circle((17.5, 6.5), 2.5);
            p.bezier((2.0, 17.0), (5.0, 11.5), (9.5, 11.5), (13.5, 17.0));
            p.bezier((9.0, 14.5), (12.5, 10.5), (17.5, 10.5), (22.0, 16.0));
            p.line(&[(2.0, 20.0), (22.0, 20.0)]);
        }
        Cloud => {
            p.path(
                &[
                    vec![(17.5, 19.0), (8.5, 19.0)],
                    p.ellipse_pts((8.5, 12.5), 6.5, 6.5, 0.0, 90.0, 343.0),
                    vec![(17.5, 10.5)],
                    p.ellipse_pts((17.5, 14.75), 4.25, 4.25, 0.0, -90.0, 90.0),
                ],
                true,
            );
        }
        Asteroid => {
            p.closed(&[(7.0, 4.5), (14.5, 3.0), (20.0, 7.5), (21.0, 14.0), (16.5, 20.5), (8.5, 21.0), (3.5, 15.0), (3.0, 9.0)]);
            p.circle((9.0, 10.0), 2.0);
            p.circle((15.0, 15.0), 2.4);
        }
        Binary => {
            p.circle((7.5, 12.0), 4.0);
            p.circle((17.5, 12.0), 2.8);
            p.ellipse((12.0, 12.0), 11.0, 4.0, 0.0, 15.0, 165.0);
        }
        Disk => {
            p.ellipse((12.0, 12.0), 10.5, 4.5, 0.0, 0.0, 360.0);
            p.ellipse((12.0, 12.0), 5.5, 2.2, 0.0, 0.0, 360.0);
            p.dot((12.0, 12.0), 1.4);
            p.dot((18.5, 9.5), 1.0);
            p.dot((5.0, 14.0), 1.0);
        }
        Empty => {
            for i in 0..12 {
                let a = i as f32 * 30.0;
                p.arc((12.0, 12.0), 9.0, a + 4.0, a + 18.0);
            }
        }

        // ── Terraform brushes ───────────────────────────────────
        Raise => {
            p.line(&[(12.0, 17.0), (12.0, 4.0)]);
            p.line(&[(6.5, 9.5), (12.0, 4.0), (17.5, 9.5)]);
            p.line(&[(4.0, 20.5), (20.0, 20.5)]);
        }
        Lower => {
            p.line(&[(12.0, 7.0), (12.0, 20.0)]);
            p.line(&[(6.5, 14.5), (12.0, 20.0), (17.5, 14.5)]);
            p.line(&[(4.0, 3.5), (20.0, 3.5)]);
        }
        Crater => {
            p.ellipse((12.0, 14.0), 10.0, 4.5, 0.0, 0.0, 360.0);
            p.ellipse((12.0, 14.5), 5.0, 2.0, 0.0, 180.0, 360.0);
            p.line(&[(6.0, 7.0), (4.5, 4.5)]);
            p.line(&[(12.0, 6.0), (12.0, 3.0)]);
            p.line(&[(18.0, 7.0), (19.5, 4.5)]);
        }
        Waves => {
            p.wave(6.5, 2.5, 21.5, 1.6);
            p.wave(12.0, 2.5, 21.5, 1.6);
            p.wave(17.5, 2.5, 21.5, 1.6);
        }
        Flame => {
            p.path(
                &[
                    Pen::bezier_pts((12.0, 2.5), (13.5, 7.0), (19.0, 9.5), (19.0, 15.0)),
                    p.ellipse_pts((12.0, 15.0), 7.0, 7.0, 0.0, 0.0, 180.0),
                    Pen::bezier_pts((5.0, 15.0), (5.0, 11.5), (7.5, 9.5), (8.5, 8.0)),
                    Pen::bezier_pts((8.5, 8.0), (9.5, 9.5), (10.5, 10.0), (10.0, 10.5)),
                    Pen::bezier_pts((10.0, 10.5), (10.0, 7.5), (11.0, 5.0), (12.0, 2.5)),
                ],
                true,
            );
            p.path(
                &[
                    Pen::bezier_pts((12.0, 12.0), (13.0, 14.0), (15.0, 14.5), (15.0, 17.0)),
                    p.ellipse_pts((12.0, 17.0), 3.0, 3.0, 0.0, 0.0, 180.0),
                    Pen::bezier_pts((9.0, 17.0), (9.0, 15.0), (11.0, 14.0), (12.0, 12.0)),
                ],
                true,
            );
        }
        Tree => {
            p.closed(&[(12.0, 2.5), (17.5, 10.0), (15.0, 10.0), (19.5, 16.5), (4.5, 16.5), (9.0, 10.0), (6.5, 10.0)]);
            p.line(&[(12.0, 16.5), (12.0, 21.5)]);
        }
        Mountain => {
            p.closed(&[(2.0, 20.0), (9.0, 6.5), (13.0, 13.0), (15.5, 9.5), (22.0, 20.0)]);
            p.line(&[(6.8, 10.8), (8.0, 11.8), (9.0, 10.8), (10.3, 11.8), (11.2, 10.4)]);
        }
        Eraser => {
            p.line(&[(7.0, 21.0), (2.8, 16.8), (2.8, 13.4), (12.6, 3.6), (16.0, 3.6), (21.2, 8.8), (21.2, 12.2), (12.4, 21.0)]);
            p.line(&[(22.0, 21.0), (7.0, 21.0)]);
            p.line(&[(5.5, 10.9), (13.6, 19.0)]);
        }

        // ── Actions ─────────────────────────────────────────────
        Target => {
            p.circle((12.0, 12.0), 9.5);
            p.circle((12.0, 12.0), 5.5);
            p.dot((12.0, 12.0), 1.7);
        }
        Focus => {
            p.circle((12.0, 12.0), 6.5);
            p.dot((12.0, 12.0), 1.7);
            p.line(&[(12.0, 1.5), (12.0, 5.5)]);
            p.line(&[(12.0, 18.5), (12.0, 22.5)]);
            p.line(&[(1.5, 12.0), (5.5, 12.0)]);
            p.line(&[(18.5, 12.0), (22.5, 12.0)]);
        }
        CircleOrbit => {
            // Gap in the orbit where the planet sits, so it reads as a body on a path.
            p.arc((12.0, 12.0), 9.0, -22.0, 247.0);
            p.dot((12.0, 12.0), 2.2);
            let q = p.ellipse_pts((12.0, 12.0), 9.0, 9.0, 0.0, -45.0, -45.0)[0];
            p.circle(q, 2.4);
        }
        Trash => {
            p.line(&[(3.0, 6.0), (21.0, 6.0)]);
            p.line(&[(8.5, 6.0), (8.5, 3.5), (15.5, 3.5), (15.5, 6.0)]);
            p.line(&[(5.5, 6.0), (6.5, 20.5), (17.5, 20.5), (18.5, 6.0)]);
            p.line(&[(10.0, 10.5), (10.0, 16.0)]);
            p.line(&[(14.0, 10.5), (14.0, 16.0)]);
        }
        Reset => {
            p.path(&[p.ellipse_pts((12.0, 12.0), 9.0, 9.0, 0.0, 180.0, -135.0), vec![(3.0, 8.0)]], false);
            p.line(&[(3.0, 3.0), (3.0, 8.0), (8.0, 8.0)]);
        }
        Dice => {
            p.closed(&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0)]);
            for c in [(8.5, 8.5), (15.5, 8.5), (12.0, 12.0), (8.5, 15.5), (15.5, 15.5)] {
                p.dot(c, 1.4);
            }
        }
        Rocket => {
            p.path(
                &[
                    Pen::bezier_pts((12.0, 2.0), (16.0, 5.0), (16.5, 10.5), (15.3, 15.5)),
                    vec![(8.7, 15.5)],
                    Pen::bezier_pts((8.7, 15.5), (7.5, 10.5), (8.0, 5.0), (12.0, 2.0)),
                ],
                true,
            );
            p.circle((12.0, 8.5), 1.8);
            p.line(&[(8.4, 11.5), (5.0, 15.0), (5.0, 18.0), (8.8, 15.5)]);
            p.line(&[(15.6, 11.5), (19.0, 15.0), (19.0, 18.0), (15.2, 15.5)]);
            p.line(&[(12.0, 18.0), (12.0, 22.0)]);
        }
        Supernova => {
            p.circle((12.0, 12.0), 3.2);
            p.rays((12.0, 12.0), 8, 6.0, 10.5, -90.0);
            p.rays((12.0, 12.0), 8, 6.0, 7.8, -67.5);
        }
        Implode => {
            p.dot((12.0, 12.0), 2.6);
            for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let o = |d: f32| (12.0 + sx * d, 12.0 + sy * d);
                p.line(&[o(9.5), o(5.0)]);
                let tip = o(5.0);
                p.line(&[(tip.0 + sx * 4.0, tip.1), tip, (tip.0, tip.1 + sy * 4.0)]);
            }
        }
        Burst => {
            let pts: Vec<P> = (0..16)
                .map(|i| {
                    let r = if i % 2 == 0 { if i % 4 == 0 { 10.5 } else { 8.5 } } else { 4.5 };
                    let a = (i as f32 * 22.5 - 90.0).to_radians();
                    (12.0 + r * a.cos(), 12.0 + r * a.sin())
                })
                .collect();
            p.closed(&pts);
        }
        Flyby => {
            p.circle((15.0, 12.0), 6.0);
            p.line(&[(2.0, 12.0), (6.5, 12.0)]);
            p.line(&[(4.0, 8.0), (8.0, 8.0)]);
            p.line(&[(4.0, 16.0), (8.0, 16.0)]);
        }
        Warning => {
            p.closed(&[(12.0, 3.0), (22.0, 20.5), (2.0, 20.5)]);
            p.line(&[(12.0, 9.5), (12.0, 14.0)]);
            p.dot((12.0, 17.2), 1.2);
        }
        // ── Exploring ───────────────────────────────────────────
        Astronaut => {
            // Helmet with a visor, shoulders, and a walking pair of legs.
            p.circle((12.0, 6.5), 4.3);
            p.line(&[(9.8, 6.2), (14.2, 6.2)]);
            p.line(&[(7.0, 16.0), (8.2, 12.6), (15.8, 12.6), (17.0, 16.0)]);
            p.line(&[(10.5, 12.8), (10.0, 17.0), (7.5, 21.5)]);
            p.line(&[(13.5, 12.8), (14.0, 17.0), (16.5, 21.5)]);
        }
        Thermometer => {
            p.path(
                &[
                    vec![(9.8, 13.6), (9.8, 4.8)],
                    p.ellipse_pts((12.0, 4.8), 2.2, 2.2, 0.0, 180.0, 360.0),
                    vec![(14.2, 13.6)],
                    p.ellipse_pts((12.0, 17.0), 4.0, 4.0, 0.0, -57.0, 237.0),
                ],
                true,
            );
            p.dot((12.0, 17.0), 1.9);
            p.line(&[(12.0, 9.0), (12.0, 16.0)]);
        }
        Download => {
            p.line(&[(12.0, 3.5), (12.0, 15.0)]);
            p.line(&[(7.0, 10.5), (12.0, 15.5), (17.0, 10.5)]);
            p.line(&[(4.0, 16.5), (4.0, 20.0), (20.0, 20.0), (20.0, 16.5)]);
        }
        Seat => {
            // Cockpit window: a canopy arc over a dashboard with a joystick.
            p.bezier((3.0, 15.0), (3.5, 2.5), (20.5, 2.5), (21.0, 15.0));
            p.line(&[(2.5, 15.0), (21.5, 15.0)]);
            p.line(&[(4.5, 20.5), (19.5, 20.5)]);
            p.line(&[(12.0, 15.0), (12.0, 20.5)]);
            p.line(&[(12.0, 3.5), (12.0, 6.5)]);
        }
        Save => {
            // Floppy disk: body with a clipped corner, shutter on top, label below.
            p.closed(&[(4.0, 4.0), (17.0, 4.0), (20.0, 7.0), (20.0, 20.0), (4.0, 20.0)]);
            p.line(&[(8.0, 4.0), (8.0, 8.5), (15.0, 8.5), (15.0, 4.0)]);
            p.line(&[(7.5, 20.0), (7.5, 13.5), (16.5, 13.5), (16.5, 20.0)]);
        }
        Folder => {
            p.closed(&[(3.0, 6.0), (3.0, 19.0), (21.0, 19.0), (21.0, 8.5), (11.5, 8.5), (9.5, 6.0)]);
            p.line(&[(3.0, 11.0), (21.0, 11.0)]);
        }
        Speaker => {
            p.closed(&[(3.5, 9.5), (7.5, 9.5), (12.5, 5.0), (12.5, 19.0), (7.5, 14.5), (3.5, 14.5)]);
            p.arc((12.5, 12.0), 4.0, -45.0, 45.0);
            p.arc((12.5, 12.0), 8.0, -45.0, 45.0);
        }
        SpeakerOff => {
            p.closed(&[(3.5, 9.5), (7.5, 9.5), (12.5, 5.0), (12.5, 19.0), (7.5, 14.5), (3.5, 14.5)]);
            p.line(&[(16.0, 9.5), (21.0, 14.5)]);
            p.line(&[(21.0, 9.5), (16.0, 14.5)]);
        }
        Music => {
            // Two quavers joined by a beam.
            p.line(&[(9.0, 17.5), (9.0, 5.5), (20.0, 3.5), (20.0, 15.5)]);
            p.line(&[(9.0, 9.5), (20.0, 7.5)]);
            p.circle((6.5, 17.5), 2.5);
            p.circle((17.5, 15.5), 2.5);
        }
        Monitor => {
            p.closed(&[(3.0, 4.5), (21.0, 4.5), (21.0, 16.0), (3.0, 16.0)]);
            p.line(&[(12.0, 16.0), (12.0, 20.0)]);
            p.line(&[(8.0, 20.0), (16.0, 20.0)]);
        }
    }
}
