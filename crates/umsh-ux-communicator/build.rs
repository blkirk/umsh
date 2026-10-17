//! Rasterizes the bundled fonts into Buoyant `PrerenderedFont` tables.
//!
//! Every character the font maps is rendered at each listed size, as 4-bit
//! coverage. Weights come from the variable fonts' `wght` axis.

use std::fmt::Write as _;
use std::path::PathBuf;

use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use swash::{FontRef, Setting};

struct Spec {
    /// The Rust name of the generated static.
    name: &'static str,
    file: &'static str,
    weight: f32,
    sizes: &'static [u16],
}

const SANS: &str = "fonts/MPLUS1[wght]-latin.ttf";
const MONO: &str = "fonts/MPLUSCodeLatin[wdth,wght]-latin.ttf";
const UI_SIZES: &[u16] = &[16, 17, 20];

const SPECS: &[Spec] = &[
    Spec {
        name: "REGULAR",
        file: SANS,
        weight: 400.0,
        sizes: UI_SIZES,
    },
    Spec {
        name: "MEDIUM",
        file: SANS,
        weight: 500.0,
        sizes: UI_SIZES,
    },
    Spec {
        name: "SEMIBOLD",
        file: SANS,
        weight: 600.0,
        sizes: UI_SIZES,
    },
    // The small and large sizes are for avatar lettering.
    Spec {
        name: "BOLD",
        file: SANS,
        weight: 700.0,
        sizes: &[10, 13, 16, 17, 20, 26],
    },
    Spec {
        name: "EXTRABOLD",
        file: SANS,
        weight: 800.0,
        sizes: UI_SIZES,
    },
    Spec {
        name: "MONO",
        file: MONO,
        weight: 400.0,
        sizes: &[16, 20],
    },
    Spec {
        name: "MONO_BOLD",
        file: MONO,
        weight: 700.0,
        sizes: &[10, 13, 16, 20],
    },
];

/// Drawn for characters a face lacks.
const FALLBACK: char = '?';

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut out = String::new();
    let mut context = ScaleContext::new();
    for spec in SPECS {
        println!("cargo:rerun-if-changed={}", spec.file);
        let data = std::fs::read(dir.join(spec.file)).expect("font file");
        let font = FontRef::from_index(&data, 0).expect("parsable font");
        let mut faces = Vec::new();
        for &size in spec.sizes {
            faces.push(face(&mut out, &mut context, font, spec, size));
        }
        writeln!(
            out,
            "pub static {}: PrerenderedFont = PrerenderedFont {{ faces: &[{}] }};",
            spec.name,
            faces.join(", ")
        )
        .unwrap();
    }
    println!("cargo:rerun-if-changed=build.rs");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out_dir.join("fonts.rs"), out).unwrap();
    std::fs::write(out_dir.join("shapes.rs"), shapes()).unwrap();
    std::fs::write(out_dir.join("icons.rs"), icons()).unwrap();
}

/// One layer of an icon: SVG path data, filled or stroked, added to the
/// mask or cut out of it.
struct Layer {
    path: &'static str,
    stroke: Option<f32>,
    cut: bool,
}

const fn stroke(path: &'static str, width: f32) -> Layer {
    Layer {
        path,
        stroke: Some(width),
        cut: false,
    }
}

const fn fill(path: &'static str) -> Layer {
    Layer {
        path,
        stroke: None,
        cut: false,
    }
}

const fn cut(path: &'static str) -> Layer {
    Layer {
        path,
        stroke: None,
        cut: true,
    }
}

/// Icons drawn centered on the origin, each with the square size in pixels
/// it is rasterized at and its scale in pixels per unit. These are the
/// mockup's glyphs at the mockup's scales.
const ICONS: &[(&str, u32, f32, &[Layer])] = &[
    (
        "CHATS",
        28,
        1.5,
        &[
            stroke(
                "M-4.5 -7 H4.5 Q8 -7 8 -3.5 V1 Q8 4.5 4.5 4.5 H-4.5 Q-8 4.5 -8 1 V-3.5 Q-8 -7 -4.5 -7 Z",
                1.4,
            ),
            stroke("M-4 4.5 L-5.5 8 L0 4.5", 1.4),
        ],
    ),
    (
        "PEERS",
        28,
        1.5,
        &[
            stroke("M0 -6 L-7 5 L7 5 Z", 1.2),
            cut("M-2.6 -6 A2.6 2.6 0 1 0 2.6 -6 A2.6 2.6 0 1 0 -2.6 -6 Z"),
            cut("M-9.6 5 A2.6 2.6 0 1 0 -4.4 5 A2.6 2.6 0 1 0 -9.6 5 Z"),
            cut("M4.4 5 A2.6 2.6 0 1 0 9.6 5 A2.6 2.6 0 1 0 4.4 5 Z"),
            stroke(
                "M-2.6 -6 A2.6 2.6 0 1 0 2.6 -6 A2.6 2.6 0 1 0 -2.6 -6 Z",
                1.4,
            ),
            stroke("M-9.6 5 A2.6 2.6 0 1 0 -4.4 5 A2.6 2.6 0 1 0 -9.6 5 Z", 1.4),
            stroke("M4.4 5 A2.6 2.6 0 1 0 9.6 5 A2.6 2.6 0 1 0 4.4 5 Z", 1.4),
        ],
    ),
    (
        "DEVICE",
        28,
        1.5,
        &[
            stroke("M-4.6 0 A4.6 4.6 0 1 0 4.6 0 A4.6 4.6 0 1 0 -4.6 0 Z", 1.6),
            fill("M-1.4 0 A1.4 1.4 0 1 0 1.4 0 A1.4 1.4 0 1 0 -1.4 0 Z"),
            stroke(
                "M6 0 L8.2 0 M4.24 4.24 L5.8 5.8 M0 6 L0 8.2 M-4.24 4.24 L-5.8 5.8 \
                 M-6 0 L-8.2 0 M-4.24 -4.24 L-5.8 -5.8 M0 -6 L0 -8.2 M4.24 -4.24 L5.8 -5.8",
                2.2,
            ),
        ],
    ),
    (
        "BLUETOOTH",
        18,
        1.3,
        &[stroke("M-3 -3 L3 3 L0 6 V-6 L3 -3 L-3 3", 1.1)],
    ),
    (
        "GNSS",
        18,
        1.3,
        &[
            stroke("M-3.6 0 A3.6 3.6 0 1 0 3.6 0 A3.6 3.6 0 1 0 -3.6 0 Z", 1.1),
            fill("M-1.1 0 A1.1 1.1 0 1 0 1.1 0 A1.1 1.1 0 1 0 -1.1 0 Z"),
            stroke("M0 -6 V-4 M0 4 V6 M-6 0 H-4 M4 0 H6", 1.1),
        ],
    ),
    (
        "BELL_SLASH",
        18,
        1.3,
        &[
            stroke(
                "M-4 3 Q-3.5 1 -3.5 -1 Q-3.5 -5 0 -5 Q3.5 -5 3.5 -1 Q3.5 1 4 3 Z M-1.3 4.5 H1.3",
                1.1,
            ),
            stroke("M-5 -5 L5 5", 1.2),
        ],
    ),
    (
        "PENCIL",
        18,
        1.2,
        &[stroke("M-5 5 L-4 1 L3 -6 L6 -3 L-1 4 Z M1 -4 L4 -1", 1.4)],
    ),
    ("BACK", 14, 1.0, &[stroke("M3 -6 L-3 0 L3 6", 2.0)]),
];

/// The icons as Gray4 masks.
fn icons() -> String {
    use swash::zeno::{Cap, Join, Mask, Stroke, Transform};
    let mut out = String::new();
    for &(name, size, k, layers) in ICONS {
        let n = (size * size) as usize;
        let mut acc = vec![0f32; n];
        for layer in layers {
            let transform =
                Transform::scale(k, k).then_translate(size as f32 / 2.0, size as f32 / 2.0);
            let mut mask = Mask::new(layer.path);
            mask.size(size, size)
                .transform(Some(transform))
                .format(Format::Alpha);
            if let Some(width) = layer.stroke {
                let mut style = Stroke::new(width);
                style.cap(Cap::Round).join(Join::Round);
                mask.style(style);
            }
            let (alpha, placement) = mask.render();
            assert_eq!((placement.width, placement.height), (size, size));
            for (a, &v) in acc.iter_mut().zip(alpha.iter()) {
                let v = f32::from(v) / 255.0;
                *a = if layer.cut { *a * (1.0 - v) } else { a.max(v) };
            }
        }
        let row_bytes = size.div_ceil(2) as usize;
        let mut packed = vec![0u8; row_bytes * size as usize];
        for y in 0..size as usize {
            for x in 0..size as usize {
                let nibble = (acc[y * size as usize + x] * 15.0).round() as u8;
                packed[y * row_bytes + x / 2] |= if x % 2 == 0 { nibble << 4 } else { nibble };
            }
        }
        writeln!(
            out,
            "pub static {name}: ImageRaw<'static, Gray4> = ImageRaw::new(&{packed:?}, {size});"
        )
        .unwrap();
    }
    out
}

/// Avatar diameters, in pixels, that the screens draw.
const AVATAR_SIZES: &[u32] = &[26, 28, 34];

/// The avatar outlines as Gray4 masks: one static per shape and size, named
/// like `HEXAGON_34`, each row padded to a whole byte.
fn shapes() -> String {
    let mut out = String::new();
    for &size in AVATAR_SIZES {
        let s = size as f32;
        let c = s / 2.0;
        let triangle_height = s * 1.16;
        let half_base = triangle_height / 3f32.sqrt();
        let half_width = s / 3f32.sqrt();
        let outlines = [
            ("CIRCLE", rounded_polygon(&circle_points(c, c, c), 0.0)),
            (
                "TRIANGLE",
                rounded_polygon(
                    &[
                        (c, s - triangle_height),
                        (c + half_base, s),
                        (c - half_base, s),
                    ],
                    s * 0.16,
                ),
            ),
            (
                "HEXAGON",
                rounded_polygon(
                    &[
                        (c - half_width, c),
                        (c - half_width / 2.0, 0.0),
                        (c + half_width / 2.0, 0.0),
                        (c + half_width, c),
                        (c + half_width / 2.0, s),
                        (c - half_width / 2.0, s),
                    ],
                    s * 0.2,
                ),
            ),
            (
                "SQUARE",
                rounded_polygon(&[(0.0, 0.0), (s, 0.0), (s, s), (0.0, s)], s * 0.25),
            ),
        ];
        for (name, path) in outlines {
            // The hexagon is wider than tall; every mask is centered in a
            // frame wide enough for it.
            let width = if name == "HEXAGON" {
                (2.0 * half_width).ceil() as u32
            } else {
                size
            };
            let dx = (width as f32 - s) / 2.0;
            let (alpha, placement) = swash::zeno::Mask::new(path.as_str())
                .size(width, size)
                .offset(swash::zeno::Vector::new(dx, 0.0))
                .format(Format::Alpha)
                .render();
            assert_eq!((placement.width, placement.height), (width, size));
            let row_bytes = width.div_ceil(2) as usize;
            let mut packed = vec![0u8; row_bytes * size as usize];
            for y in 0..size as usize {
                for x in 0..width as usize {
                    let a = alpha[y * width as usize + x];
                    let nibble = ((u16::from(a) * 15 + 127) / 255) as u8;
                    packed[y * row_bytes + x / 2] |= if x % 2 == 0 { nibble << 4 } else { nibble };
                }
            }
            writeln!(
                out,
                "pub static {name}_{size}: ImageRaw<'static, Gray4> = ImageRaw::new(&{packed:?}, {width});"
            )
            .unwrap();
        }
    }
    out
}

/// A circle, as a polygon so it shares the path code: enough sides that no
/// facet survives rasterization at these sizes.
fn circle_points(cx: f32, cy: f32, r: f32) -> Vec<(f32, f32)> {
    (0..96)
        .map(|i| {
            let a = i as f32 / 96.0 * std::f32::consts::TAU;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

/// SVG path data for a closed polygon whose corners are arcs of `radius`
/// tangent to both adjoining edges, matching the iOS `PeerAvatarOutline`.
fn rounded_polygon(points: &[(f32, f32)], radius: f32) -> String {
    let n = points.len();
    let mut d = String::new();
    for i in 0..n {
        let p0 = points[(i + n - 1) % n];
        let p1 = points[i];
        let p2 = points[(i + 1) % n];
        let v1 = (p0.0 - p1.0, p0.1 - p1.1);
        let v2 = (p2.0 - p1.0, p2.1 - p1.1);
        let l1 = v1.0.hypot(v1.1);
        let l2 = v2.0.hypot(v2.1);
        let angle = ((v1.0 * v2.0 + v1.1 * v2.1) / (l1 * l2))
            .clamp(-1.0, 1.0)
            .acos();
        let t = if radius > 0.0 {
            radius / (angle / 2.0).tan()
        } else {
            0.0
        };
        let a = (p1.0 + v1.0 / l1 * t, p1.1 + v1.1 / l1 * t);
        let b = (p1.0 + v2.0 / l2 * t, p1.1 + v2.1 / l2 * t);
        write!(d, "{}{:.3} {:.3}", if i == 0 { "M" } else { "L" }, a.0, a.1).unwrap();
        if radius > 0.0 {
            write!(d, "A{radius:.3} {radius:.3} 0 0 1 {:.3} {:.3}", b.0, b.1).unwrap();
        }
    }
    d.push('Z');
    d
}

/// Writes one face's tables and returns its `Face` expression.
fn face(
    out: &mut String,
    context: &mut ScaleContext,
    font: FontRef,
    spec: &Spec,
    size: u16,
) -> String {
    let settings = [Setting {
        tag: swash::tag_from_bytes(b"wght"),
        value: spec.weight,
    }];
    let coords: Vec<_> = font
        .variations()
        .normalized_coords(settings.iter().copied())
        .collect();
    let glyph_metrics = font.glyph_metrics(&coords).scale(f32::from(size));

    let mut scaler = context
        .builder(font)
        .size(f32::from(size))
        .hint(false)
        .variations(settings.iter().copied())
        .build();
    let mut renderer = Render::new(&[Source::Outline]);
    renderer.format(Format::Alpha);

    let mut chars = Vec::new();
    font.charmap().enumerate(|code, id| {
        if let Some(c) = char::from_u32(code)
            && id != 0
        {
            chars.push((c, id));
        }
    });
    chars.sort_unstable();
    let images: Vec<_> = chars
        .iter()
        .map(|&(c, id)| {
            let image = renderer
                .render(&mut scaler, id)
                .filter(|i| i.placement.width > 0 && i.placement.height > 0);
            (c, glyph_metrics.advance_width(id).round() as i32, image)
        })
        .collect();

    // The font's own line metrics make room for Japanese, about 1.45 em.
    // The line box here fits ASCII instead, and lines are 1.2 em apart:
    // 24 px for 20 px text, as in the mockup.
    let ascii = || {
        images
            .iter()
            .filter(|(c, ..)| c.is_ascii_graphic())
            .filter_map(|(_, _, i)| i.as_ref())
    };
    let ascent = ascii().map(|i| i.placement.top).max().unwrap_or(0);
    let descent = -ascii()
        .map(|i| i.placement.height as i32 - i.placement.top)
        .max()
        .unwrap_or(0);
    let line_gap = ((f32::from(size) * 1.2).round() as i32 - (ascent - descent)).max(0);

    let mut pixels: Vec<u8> = Vec::new();
    let mut entries = Vec::new();
    let mut fallback = None;
    for (c, advance, image) in images {
        let start = pixels.len();
        let (left, top, width, height) = match image {
            Some(image) => {
                let p = image.placement;
                // Round 8-bit alpha to 4 bits.
                pixels.extend(
                    image
                        .data
                        .iter()
                        .map(|&a| ((u16::from(a) * 15 + 127) / 255) as u8),
                );
                (p.left, ascent - p.top, p.width, p.height)
            }
            None => (0, 0, 0, 0),
        };
        let entry = format!(
            "GlyphEntry {{ character: {c:?}, advance: {advance}, left: {left}, top: {top}, \
             width: {width}, height: {height}, start: {start} }}"
        );
        if c == FALLBACK {
            fallback = Some(entry.clone());
        }
        entries.push(entry);
    }

    let id = format!("{}_{}", spec.name, size);
    let mut packed = Vec::with_capacity(pixels.len().div_ceil(2));
    for pair in pixels.chunks(2) {
        packed.push((pair[0] << 4) | pair.get(1).copied().unwrap_or(0));
    }
    writeln!(
        out,
        "static {id}_GLYPHS: [GlyphEntry; {}] = [{}];",
        entries.len(),
        entries.join(", ")
    )
    .unwrap();
    writeln!(
        out,
        "static {id}_COVERAGE: [u8; {}] = {packed:?};",
        packed.len()
    )
    .unwrap();
    format!(
        "Face {{ size: {size}, ascent: {ascent}, descent: {descent}, line_gap: {line_gap}, \
         glyphs: &{id}_GLYPHS, coverage: &{id}_COVERAGE, fallback: {} }}",
        fallback.map_or_else(|| "None".to_string(), |e| format!("Some({e})"))
    )
}
