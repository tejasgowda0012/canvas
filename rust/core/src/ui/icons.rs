//! The Inkboard icon set, drawn on a 24 by 24 grid.

use super::paint::{push_polyline, rrect_path, Rf};
use tiny_skia::{Path, PathBuilder};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ico {
    Back,
    More,
    Kebab,
    Share,
    Undo,
    Redo,
    Select,
    Hand,
    Pen,
    Highlighter,
    Eraser,
    Shapes,
    Text,
    Convert,
    Refine,
    Plus,
    Minus,
    Fit,
    Close,
    Check,
    Chevron,
    ChevronLeft,
    Gear,
    Join,
    Claude,
    Export,
    Import,
    Update,
    Font,
    Rect,
    Ellipse,
    Arrow,
    Line,
    Triangle,
    Diamond,
    Star,
    Hexagon,
    Smart,
    Trash,
    Rename,
    Copy,
    Grid,
    Dot,
    Bigger,
    Smaller,
    Page,
    PagePlus,
    Table,
    MindMap,
    Formula,
    Image,
    Pdf,
    Palette,
    Dock,
    Graph,
    Lock,
    People,
    Globe,
    Link,
    Exit,
}

pub const ALL: [Ico; 60] = [
    Ico::Back,
    Ico::More,
    Ico::Kebab,
    Ico::Share,
    Ico::Undo,
    Ico::Redo,
    Ico::Select,
    Ico::Hand,
    Ico::Pen,
    Ico::Highlighter,
    Ico::Eraser,
    Ico::Shapes,
    Ico::Text,
    Ico::Convert,
    Ico::Refine,
    Ico::Plus,
    Ico::Minus,
    Ico::Fit,
    Ico::Close,
    Ico::Check,
    Ico::Chevron,
    Ico::ChevronLeft,
    Ico::Gear,
    Ico::Join,
    Ico::Claude,
    Ico::Export,
    Ico::Import,
    Ico::Update,
    Ico::Font,
    Ico::Rect,
    Ico::Ellipse,
    Ico::Arrow,
    Ico::Line,
    Ico::Triangle,
    Ico::Diamond,
    Ico::Star,
    Ico::Hexagon,
    Ico::Smart,
    Ico::Trash,
    Ico::Rename,
    Ico::Copy,
    Ico::Grid,
    Ico::Dot,
    Ico::Bigger,
    Ico::Smaller,
    Ico::Page,
    Ico::PagePlus,
    Ico::Table,
    Ico::MindMap,
    Ico::Formula,
    Ico::Image,
    Ico::Pdf,
    Ico::Palette,
    Ico::Dock,
    Ico::Graph,
    Ico::Lock,
    Ico::People,
    Ico::Globe,
    Ico::Link,
    Ico::Exit,
];

impl Ico {
    pub const ALL: [Ico; 60] = ALL;
}

fn outline(path: &mut PathBuilder, pts: &[(f32, f32)]) {
    push_polyline(path, pts);
    path.close();
}

fn rounded_rect(path: &mut PathBuilder, x: f32, y: f32, w: f32, h: f32, r: f32) {
    if let Some(rect) = rrect_path(Rf::new(x, y, w, h), r) {
        path.push_path(&rect);
    }
}

fn pen_outline(path: &mut PathBuilder) {
    push_polyline(path, &[(4.0, 20.0), (5.0, 16.0), (16.0, 5.0)]);
    // Two quarter arcs round the cap of the diagonal pen.
    path.cubic_to(16.83, 4.17, 18.17, 4.17, 19.0, 5.0);
    path.cubic_to(19.83, 5.83, 19.83, 7.17, 19.0, 8.0);
    path.line_to(8.0, 19.0);
    path.close();
}

fn hand_outline(path: &mut PathBuilder) {
    path.move_to(8.0, 13.0);
    path.line_to(8.0, 6.0);
    path.cubic_to(8.0, 4.0, 11.0, 4.0, 11.0, 6.0);
    path.line_to(11.0, 11.0);
    path.line_to(11.0, 4.5);
    path.cubic_to(11.0, 2.5, 14.0, 2.5, 14.0, 4.5);
    path.line_to(14.0, 11.0);
    path.line_to(14.0, 6.0);
    path.cubic_to(14.0, 4.0, 17.0, 4.0, 17.0, 6.0);
    path.line_to(17.0, 13.0);
    path.line_to(17.0, 11.5);
    path.cubic_to(17.0, 9.5, 20.0, 9.5, 20.0, 11.5);
    path.line_to(20.0, 15.5);
    path.cubic_to(20.0, 18.54, 17.54, 21.0, 14.5, 21.0);
    path.line_to(12.0, 21.0);
    path.cubic_to(10.43, 21.0, 8.95, 20.26, 8.0, 19.0);
    path.line_to(4.0, 12.4);
    path.cubic_to(2.93, 10.75, 5.31, 9.0, 6.4, 10.6);
    path.close();
}

fn history_arrow(path: &mut PathBuilder, mirrored: bool) {
    let x = |x| if mirrored { 24.0 - x } else { x };
    push_polyline(path, &[(x(9.0), 7.0), (x(4.0), 12.0), (x(9.0), 17.0)]);
    push_polyline(path, &[(x(4.0), 12.0), (x(14.0), 12.0)]);
    path.cubic_to(x(16.21), 12.0, x(18.0), 13.79, x(18.0), 16.0);
    path.cubic_to(x(18.0), 18.21, x(16.21), 20.0, x(14.0), 20.0);
    path.line_to(x(11.0), 20.0);
}

fn grid(path: &mut PathBuilder, radius: f32) {
    rounded_rect(path, 4.0, 4.0, 16.0, 16.0, radius);
    for split in [4.0 + 16.0 / 3.0, 4.0 + 32.0 / 3.0] {
        push_polyline(path, &[(4.0, split), (20.0, split)]);
        push_polyline(path, &[(split, 4.0), (split, 20.0)]);
    }
}

fn four_point_star(path: &mut PathBuilder, cx: f32, cy: f32, tip: f32, inner: f32) {
    outline(
        path,
        &[
            (cx, cy - tip),
            (cx + inner, cy - inner),
            (cx + tip, cy),
            (cx + inner, cy + inner),
            (cx, cy + tip),
            (cx - inner, cy + inner),
            (cx - tip, cy),
            (cx - inner, cy - inner),
        ],
    );
}

pub(super) fn paths(ico: Ico) -> (Option<Path>, Option<Path>) {
    use Ico::*;

    let mut stroke = PathBuilder::new();
    let mut fill = PathBuilder::new();
    match ico {
        Back => {
            push_polyline(&mut stroke, &[(19.0, 12.0), (5.0, 12.0)]);
            push_polyline(&mut stroke, &[(11.0, 6.0), (5.0, 12.0), (11.0, 18.0)]);
        }
        More | Kebab => {
            for p in [5.0, 12.0, 19.0] {
                let (x, y) = if ico == More { (p, 12.0) } else { (12.0, p) };
                fill.push_circle(x, y, 1.6);
            }
        }
        Share => {
            for (x, y) in [(6.0, 12.0), (18.0, 6.0), (18.0, 18.0)] {
                stroke.push_circle(x, y, 2.6);
            }
            push_polyline(&mut stroke, &[(8.4, 10.8), (15.6, 7.2)]);
            push_polyline(&mut stroke, &[(8.4, 13.2), (15.6, 16.8)]);
        }
        Undo | Redo => history_arrow(&mut stroke, ico == Redo),
        Select => outline(
            &mut stroke,
            &[(5.0, 3.0), (19.0, 11.0), (13.0, 13.0), (11.0, 19.0)],
        ),
        Hand => hand_outline(&mut stroke),
        Pen | Rename => {
            pen_outline(&mut stroke);
            if ico == Pen {
                push_polyline(&mut stroke, &[(14.0, 7.0), (17.0, 10.0)]);
            } else {
                push_polyline(&mut stroke, &[(13.0, 20.0), (21.0, 20.0)]);
            }
        }
        Highlighter => {
            outline(
                &mut stroke,
                &[(9.0, 14.0), (16.0, 7.0), (19.0, 10.0), (12.0, 17.0)],
            );
            push_polyline(&mut stroke, &[(9.0, 14.0), (8.0, 18.0), (12.0, 17.0)]);
            push_polyline(&mut stroke, &[(4.0, 21.0), (12.0, 21.0)]);
        }
        Eraser => {
            outline(
                &mut stroke,
                &[
                    (4.0, 15.0),
                    (13.0, 6.0),
                    (16.0, 6.0),
                    (19.0, 9.0),
                    (19.0, 12.0),
                    (12.0, 19.0),
                    (8.0, 19.0),
                ],
            );
            push_polyline(&mut stroke, &[(9.0, 11.0), (15.0, 17.0)]);
            push_polyline(&mut stroke, &[(12.0, 20.0), (20.0, 20.0)]);
        }
        Shapes => {
            rounded_rect(&mut stroke, 3.0, 11.0, 9.0, 9.0, 1.5);
            stroke.push_circle(15.0, 9.0, 5.5);
        }
        Text => {
            push_polyline(
                &mut stroke,
                &[(5.0, 6.0), (5.0, 4.0), (19.0, 4.0), (19.0, 6.0)],
            );
            push_polyline(&mut stroke, &[(12.0, 4.0), (12.0, 20.0)]);
            push_polyline(&mut stroke, &[(9.0, 20.0), (15.0, 20.0)]);
        }
        Convert => {
            push_polyline(
                &mut stroke,
                &[
                    (2.0, 17.0),
                    (4.0, 14.8),
                    (6.0, 16.6),
                    (8.0, 13.3),
                    (10.0, 14.0),
                ],
            );
            push_polyline(&mut stroke, &[(13.0, 5.0), (22.0, 5.0)]);
            push_polyline(&mut stroke, &[(17.5, 5.0), (17.5, 14.0)]);
            four_point_star(&mut fill, 19.0, 20.0, 3.0, 0.9);
        }
        Refine => {
            // A wavy line of writing settling onto a straight baseline.
            push_polyline(
                &mut stroke,
                &[(2.0, 12.0), (4.5, 8.5), (7.0, 12.5), (9.5, 9.0), (12.0, 12.5), (14.5, 9.5), (16.0, 12.0)],
            );
            push_polyline(&mut stroke, &[(2.0, 17.0), (16.0, 17.0)]);
            four_point_star(&mut fill, 19.5, 6.0, 3.0, 0.9);
        }
        Plus | Minus => {
            push_polyline(&mut stroke, &[(5.0, 12.0), (19.0, 12.0)]);
            if ico == Plus {
                push_polyline(&mut stroke, &[(12.0, 5.0), (12.0, 19.0)]);
            }
        }
        Fit => {
            push_polyline(&mut stroke, &[(9.0, 4.0), (4.0, 4.0), (4.0, 9.0)]);
            push_polyline(&mut stroke, &[(15.0, 4.0), (20.0, 4.0), (20.0, 9.0)]);
            push_polyline(&mut stroke, &[(9.0, 20.0), (4.0, 20.0), (4.0, 15.0)]);
            push_polyline(&mut stroke, &[(15.0, 20.0), (20.0, 20.0), (20.0, 15.0)]);
        }
        Close => {
            push_polyline(&mut stroke, &[(6.0, 6.0), (18.0, 18.0)]);
            push_polyline(&mut stroke, &[(18.0, 6.0), (6.0, 18.0)]);
        }
        Check => push_polyline(&mut stroke, &[(5.0, 12.5), (9.5, 17.0), (19.0, 7.5)]),
        Chevron => push_polyline(&mut stroke, &[(9.0, 5.0), (16.0, 12.0), (9.0, 19.0)]),
        ChevronLeft => push_polyline(&mut stroke, &[(15.0, 5.0), (8.0, 12.0), (15.0, 19.0)]),
        Gear => {
            stroke.push_circle(12.0, 12.0, 3.2);
            for i in 0..8 {
                let (y, x) = (i as f32 * std::f32::consts::FRAC_PI_4).sin_cos();
                push_polyline(
                    &mut stroke,
                    &[
                        (12.0 + 7.0 * x, 12.0 + 7.0 * y),
                        (12.0 + 9.5 * x, 12.0 + 9.5 * y),
                    ],
                );
            }
        }
        Join => {
            stroke.push_circle(9.0, 8.0, 3.5);
            stroke.move_to(2.5, 20.0);
            stroke.cubic_to(2.5, 16.41, 5.41, 13.5, 9.0, 13.5);
            stroke.cubic_to(12.59, 13.5, 15.5, 16.41, 15.5, 20.0);
            push_polyline(&mut stroke, &[(19.0, 8.0), (19.0, 14.0)]);
            push_polyline(&mut stroke, &[(16.0, 11.0), (22.0, 11.0)]);
        }
        Claude => four_point_star(&mut stroke, 12.0, 11.0, 8.5, 2.2),
        Export | Import | Update => {
            if ico == Update {
                push_polyline(&mut stroke, &[(5.0, 20.0), (19.0, 20.0)]);
            } else {
                push_polyline(
                    &mut stroke,
                    &[(4.0, 15.0), (4.0, 20.0), (20.0, 20.0), (20.0, 15.0)],
                );
            }
            push_polyline(&mut stroke, &[(12.0, 3.0), (12.0, 15.0)]);
            if ico == Export {
                push_polyline(&mut stroke, &[(7.0, 8.0), (12.0, 3.0), (17.0, 8.0)]);
            } else {
                push_polyline(&mut stroke, &[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]);
            }
        }
        Font => {
            push_polyline(&mut stroke, &[(3.0, 19.0), (8.0, 6.0), (13.0, 19.0)]);
            push_polyline(&mut stroke, &[(5.0, 14.0), (11.0, 14.0)]);
            push_polyline(&mut stroke, &[(15.0, 19.0), (18.5, 11.0), (22.0, 19.0)]);
            push_polyline(&mut stroke, &[(16.3, 16.5), (20.7, 16.5)]);
        }
        Rect => rounded_rect(&mut stroke, 4.0, 6.0, 16.0, 12.0, 1.5),
        Ellipse => {
            if let Some(oval) = tiny_skia::Rect::from_xywh(3.0, 5.5, 18.0, 13.0) {
                stroke.push_oval(oval);
            }
        }
        Arrow | Line => {
            push_polyline(&mut stroke, &[(5.0, 19.0), (19.0, 5.0)]);
            if ico == Arrow {
                push_polyline(&mut stroke, &[(10.0, 5.0), (19.0, 5.0), (19.0, 14.0)]);
            }
        }
        Triangle => outline(&mut stroke, &[(12.0, 4.0), (21.0, 20.0), (3.0, 20.0)]),
        Diamond => outline(
            &mut stroke,
            &[(12.0, 3.0), (21.0, 12.0), (12.0, 21.0), (3.0, 12.0)],
        ),
        Star => outline(
            &mut stroke,
            &[
                (12.0, 3.0),
                (14.7, 8.5),
                (20.8, 9.4),
                (16.4, 13.7),
                (17.4, 19.7),
                (12.0, 16.9),
                (6.6, 19.7),
                (7.6, 13.7),
                (3.2, 9.4),
                (9.3, 8.5),
            ],
        ),
        Hexagon => outline(
            &mut stroke,
            &[
                (7.5, 4.2),
                (16.5, 4.2),
                (21.0, 12.0),
                (16.5, 19.8),
                (7.5, 19.8),
                (3.0, 12.0),
            ],
        ),
        Smart => {
            push_polyline(
                &mut stroke,
                &[
                    (2.5, 16.0),
                    (4.0, 9.0),
                    (6.0, 14.0),
                    (8.0, 10.0),
                    (10.0, 12.0),
                    (12.0, 12.0),
                ],
            );
            rounded_rect(&mut stroke, 12.0, 7.0, 10.0, 10.0, 1.0);
        }
        Trash => {
            push_polyline(&mut stroke, &[(4.0, 7.0), (20.0, 7.0)]);
            push_polyline(
                &mut stroke,
                &[(9.0, 7.0), (9.0, 4.0), (15.0, 4.0), (15.0, 7.0)],
            );
            push_polyline(
                &mut stroke,
                &[(6.0, 7.0), (7.0, 20.0), (17.0, 20.0), (18.0, 7.0)],
            );
            push_polyline(&mut stroke, &[(10.0, 11.0), (10.0, 17.0)]);
            push_polyline(&mut stroke, &[(14.0, 11.0), (14.0, 17.0)]);
        }
        Copy => {
            rounded_rect(&mut stroke, 8.0, 8.0, 12.0, 12.0, 2.0);
            stroke.move_to(16.0, 8.0);
            stroke.line_to(16.0, 5.0);
            stroke.quad_to(16.0, 4.0, 15.0, 4.0);
            stroke.line_to(5.0, 4.0);
            stroke.quad_to(4.0, 4.0, 4.0, 5.0);
            stroke.line_to(4.0, 15.0);
            stroke.quad_to(4.0, 16.0, 5.0, 16.0);
            stroke.line_to(8.0, 16.0);
        }
        Grid => grid(&mut stroke, 0.0),
        Dot => fill.push_circle(12.0, 12.0, 7.0),
        Bigger | Smaller => {
            push_polyline(&mut stroke, &[(3.0, 19.0), (8.5, 5.0), (14.0, 19.0)]);
            push_polyline(&mut stroke, &[(4.9, 14.0), (12.1, 14.0)]);
            push_polyline(&mut stroke, &[(18.5, 7.0), (18.5, 17.0)]);
            let (wings, tip) = if ico == Bigger {
                (10.0, 7.0)
            } else {
                (14.0, 17.0)
            };
            push_polyline(&mut stroke, &[(15.5, wings), (18.5, tip), (21.5, wings)]);
        }
        Page | PagePlus | Pdf => {
            outline(
                &mut stroke,
                &[
                    (5.0, 3.0),
                    (14.0, 3.0),
                    (19.0, 8.0),
                    (19.0, 21.0),
                    (5.0, 21.0),
                ],
            );
            push_polyline(&mut stroke, &[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]);
            match ico {
                PagePlus => {
                    push_polyline(&mut stroke, &[(9.0, 14.0), (15.0, 14.0)]);
                    push_polyline(&mut stroke, &[(12.0, 11.0), (12.0, 17.0)]);
                }
                Pdf => {
                    push_polyline(&mut stroke, &[(8.5, 7.5), (10.5, 7.5)]);
                    push_polyline(&mut stroke, &[(8.5, 12.0), (15.5, 12.0)]);
                    push_polyline(&mut stroke, &[(8.5, 16.5), (14.0, 16.5)]);
                }
                _ => {}
            }
        }
        Table => grid(&mut stroke, 2.5),
        MindMap => {
            stroke.push_circle(7.0, 12.0, 3.0);
            for y in [4.5, 12.0, 19.5] {
                stroke.push_circle(18.0, y, 2.0);
            }
            push_polyline(&mut stroke, &[(9.4, 10.2), (16.8, 6.1)]);
            push_polyline(&mut stroke, &[(10.0, 12.0), (16.0, 12.0)]);
            push_polyline(&mut stroke, &[(9.4, 13.8), (16.8, 17.9)]);
        }
        Formula => {
            push_polyline(
                &mut stroke,
                &[
                    (3.0, 12.0),
                    (5.0, 12.0),
                    (8.0, 19.0),
                    (12.0, 5.0),
                    (21.0, 5.0),
                ],
            );
            push_polyline(&mut stroke, &[(14.0, 11.0), (20.0, 17.0)]);
            push_polyline(&mut stroke, &[(20.0, 11.0), (14.0, 17.0)]);
        }
        Graph => {
            push_polyline(&mut stroke, &[(4.0, 4.0), (4.0, 20.0), (20.0, 20.0)]);
            push_polyline(&mut stroke, &[(7.0, 14.0), (10.0, 10.0), (13.0, 13.0), (19.0, 6.0)]);
        }
        Image => {
            rounded_rect(&mut stroke, 3.0, 4.0, 18.0, 16.0, 2.0);
            stroke.push_circle(8.0, 9.0, 1.5);
            push_polyline(
                &mut stroke,
                &[
                    (3.0, 17.0),
                    (8.0, 13.0),
                    (11.0, 16.0),
                    (15.0, 11.0),
                    (21.0, 17.0),
                ],
            );
        }
        Palette => {
            stroke.move_to(12.0, 3.5);
            stroke.cubic_to(6.5, 3.5, 3.0, 7.1, 3.0, 12.0);
            stroke.cubic_to(3.0, 17.1, 7.0, 20.5, 12.0, 20.5);
            stroke.cubic_to(14.0, 20.5, 15.0, 18.8, 14.0, 17.0);
            stroke.cubic_to(13.1, 15.3, 14.1, 14.0, 16.0, 14.0);
            stroke.line_to(17.5, 14.0);
            stroke.cubic_to(20.0, 14.0, 21.0, 12.5, 21.0, 10.5);
            stroke.cubic_to(21.0, 6.6, 17.0, 3.5, 12.0, 3.5);
            stroke.close();
            for (x, y) in [(7.0, 12.5), (10.5, 7.8), (16.1, 8.5)] {
                stroke.push_circle(x, y, 1.2);
            }
        }
        Dock => {
            rounded_rect(&mut stroke, 3.0, 5.0, 18.0, 14.0, 2.5);
            push_polyline(&mut stroke, &[(8.5, 5.0), (8.5, 19.0)]);
        }
        Lock => {
            rounded_rect(&mut stroke, 5.0, 11.0, 14.0, 9.0, 2.0);
            stroke.move_to(8.0, 11.0);
            stroke.line_to(8.0, 8.0);
            stroke.cubic_to(8.0, 5.8, 9.8, 4.0, 12.0, 4.0);
            stroke.cubic_to(14.2, 4.0, 16.0, 5.8, 16.0, 8.0);
            stroke.line_to(16.0, 11.0);
        }
        People => {
            stroke.push_circle(9.0, 8.0, 3.2);
            stroke.move_to(3.0, 20.0);
            stroke.cubic_to(3.0, 16.5, 5.7, 14.0, 9.0, 14.0);
            stroke.cubic_to(12.3, 14.0, 15.0, 16.5, 15.0, 20.0);
            stroke.push_circle(17.0, 8.5, 2.5);
            stroke.move_to(17.5, 13.6);
            stroke.cubic_to(19.6, 13.8, 21.0, 15.6, 21.0, 18.0);
        }
        Globe => {
            stroke.push_circle(12.0, 12.0, 9.0);
            if let Some(oval) = tiny_skia::Rect::from_xywh(8.0, 3.0, 8.0, 18.0) {
                stroke.push_oval(oval);
            }
            push_polyline(&mut stroke, &[(3.0, 12.0), (21.0, 12.0)]);
        }
        Link => {
            rounded_rect(&mut stroke, 2.5, 8.5, 10.5, 7.0, 3.5);
            rounded_rect(&mut stroke, 11.0, 8.5, 10.5, 7.0, 3.5);
        }
        Exit => {
            push_polyline(&mut stroke, &[(10.0, 4.0), (5.0, 4.0), (5.0, 20.0), (10.0, 20.0)]);
            push_polyline(&mut stroke, &[(9.0, 12.0), (20.0, 12.0)]);
            push_polyline(&mut stroke, &[(16.0, 8.0), (20.0, 12.0), (16.0, 16.0)]);
        }
    }
    (stroke.finish(), fill.finish())
}
