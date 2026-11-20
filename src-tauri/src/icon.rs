//! Rasterizer for the date and optional clock in the Windows notification area.

use std::sync::OnceLock;

use ab_glyph::{point, Font, FontArc, Glyph, GlyphId, PxScale, ScaleFont};
use tauri::image::Image;

const FONT_FA: &[u8] = include_bytes!("../assets/Vazirmatn-FD-Bold.ttf");
const FONT_LATIN: &[u8] = include_bytes!("../assets/Vazirmatn-Bold.ttf");

/// Pixels that stay clear at every icon border. Glyph ink is never allowed
/// closer to the edge, so the number cannot be clipped by the tray.
const EDGE_INSET: f32 = 1.0;

fn font_fa() -> &'static FontArc {
    static FONT: OnceLock<FontArc> = OnceLock::new();
    FONT.get_or_init(|| {
        FontArc::try_from_slice(FONT_FA).expect("embedded Vazirmatn FD font is valid")
    })
}

fn font_latin() -> &'static FontArc {
    static FONT: OnceLock<FontArc> = OnceLock::new();
    FONT.get_or_init(|| {
        FontArc::try_from_slice(FONT_LATIN).expect("embedded Vazirmatn font is valid")
    })
}

/// A rectangle in icon pixel coordinates that a text run must fill.
#[derive(Clone, Copy, Debug)]
struct TextBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl TextBox {
    fn center_x(&self) -> f32 {
        self.x + self.width / 2.0
    }

    fn center_y(&self) -> f32 {
        self.y + self.height / 2.0
    }
}

/// Target box for the icon text: the whole icon minus the border inset.
fn day_box(size: usize) -> TextBox {
    let side = size as f32 - 2.0 * EDGE_INSET;
    TextBox {
        x: EDGE_INSET,
        y: EDGE_INSET,
        width: side,
        height: side,
    }
}

/// Resolve a user-selected RGB value or the automatic taskbar text color.
pub fn text_color(custom: Option<&str>, light_text: bool) -> [u8; 4] {
    if let Some(value) = custom.filter(|value| crate::settings::is_valid_hex_color(value)) {
        if let Ok(rgb) = u32::from_str_radix(&value[1..], 16) {
            return [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255];
        }
    }
    if light_text {
        [242, 245, 250, 255]
    } else {
        [26, 30, 38, 255]
    }
}

/// Fit each enabled clock field into its own row of the tray icon.
pub fn render_clock(parts: &[(u32, [u8; 4])], fa_digits: bool, px: u32) -> Image<'static> {
    let size = px.clamp(16, 128) as usize;
    let mut buffer = vec![0u8; size * size * 4];
    let mut row = day_box(size);
    let gap = (size / 16).max(1) as f32;
    row.height = (row.height + gap) / parts.len() as f32 - gap;
    for (value, color) in parts {
        draw_fitted_text(
            &mut buffer,
            size,
            &format!("{value:02}"),
            row,
            *color,
            fa_digits,
        );
        row.y += row.height + gap;
    }
    Image::new_owned(buffer, size as u32, size as u32)
}

// ---------------------------------------------------------------------------
// Text layout and auto-fitting
// ---------------------------------------------------------------------------

/// Ink bounding box of a laid-out text run, in icon pixel coordinates.
#[derive(Clone, Copy, Debug)]
struct InkBounds {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl InkBounds {
    fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    fn height(&self) -> f32 {
        self.max_y - self.min_y
    }

    fn center(&self) -> (f32, f32) {
        (
            (self.min_x + self.max_x) / 2.0,
            (self.min_y + self.max_y) / 2.0,
        )
    }
}

/// A text run laid out at a fixed scale with its baseline at the origin.
struct TextLayout {
    scale: PxScale,
    advances: Vec<(GlyphId, f32)>,
    ink: InkBounds,
}

/// Lay `text` out at `scale` and measure the union of the glyph ink boxes.
fn lay_out(font: &FontArc, scale: PxScale, text: &str) -> Option<TextLayout> {
    let scaled = font.as_scaled(scale);

    let mut advances = Vec::with_capacity(text.chars().count());
    let mut pen_x = 0.0f32;
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        advances.push((id, pen_x));
        pen_x += scaled.h_advance(id);
    }

    // The pen advance says nothing about where the ink actually is, so walk the
    // glyph outlines and union their integer pixel bounding boxes.
    let mut ink: Option<InkBounds> = None;
    for (id, x) in &advances {
        let glyph: Glyph = id.with_scale_and_position(scale, point(*x, 0.0));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            ink = Some(match ink {
                None => InkBounds {
                    min_x: bounds.min.x,
                    min_y: bounds.min.y,
                    max_x: bounds.max.x,
                    max_y: bounds.max.y,
                },
                Some(ink) => InkBounds {
                    min_x: ink.min_x.min(bounds.min.x),
                    min_y: ink.min_y.min(bounds.min.y),
                    max_x: ink.max_x.max(bounds.max.x),
                    max_y: ink.max_y.max(bounds.max.y),
                },
            });
        }
    }

    let ink = ink?;
    if ink.width() <= 0.0 || ink.height() <= 0.0 {
        return None;
    }
    Some(TextLayout {
        scale,
        advances,
        ink,
    })
}

/// Rasterise an already measured text run with its baseline shifted by
/// `(dx, dy)`, blending the glyph colour into the RGBA buffer.
fn rasterise(
    font: &FontArc,
    buffer: &mut [u8],
    size: usize,
    layout: &TextLayout,
    dx: f32,
    dy: f32,
    color: [u8; 4],
) {
    for (id, x) in &layout.advances {
        let glyph = id.with_scale_and_position(layout.scale, point(x + dx, dy));
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            outlined.draw(|gx, gy, coverage| {
                if coverage <= 0.0 {
                    return;
                }
                let px = bounds.min.x as i32 + gx as i32;
                let py = bounds.min.y as i32 + gy as i32;
                if px >= 0 && py >= 0 && (px as usize) < size && (py as usize) < size {
                    blend_pixel(buffer, size, px as usize, py as usize, color, coverage);
                }
            });
        }
    }
}

/// Choose the largest scale whose ink fits `target`, then centre the ink inside
/// the target box and draw it.
fn draw_fitted_text(
    buffer: &mut [u8],
    size: usize,
    text: &str,
    target: TextBox,
    color: [u8; 4],
    fa_digits: bool,
) {
    let font = if fa_digits { font_fa() } else { font_latin() };
    if let Some((layout, dx, dy)) = fit_text(font, size, text, target) {
        rasterise(font, buffer, size, &layout, dx, dy, color);
    }
}

/// Find the scale at which `text` fills `target` and return the layout with the
/// baseline offset that centres its ink box inside the target.
///
/// `px_bounds` is rounded to whole pixels, so the ink does not scale perfectly
/// linearly; the scale is refined in a couple of iterations until the measured
/// ink box lands on the target box.
fn fit_text(
    font: &FontArc,
    size: usize,
    text: &str,
    target: TextBox,
) -> Option<(TextLayout, f32, f32)> {
    // Start generous: digit ink is only ~70% of the em box, so the scale that
    // fills a box is considerably larger than the box itself.
    let mut px_size = (target.width.max(target.height) * 1.6).max(8.0);
    let mut layout = lay_out(font, PxScale::from(px_size), text)?;

    for _ in 0..5 {
        let fit = fit_factor(&layout.ink, target);
        if (0.999..=1.002).contains(&fit) {
            break;
        }
        px_size = (px_size * fit).clamp(1.0, 4096.0);
        layout = lay_out(font, PxScale::from(px_size), text)?;
    }

    // Pixel rounding can overshoot on the last step: shrink until the measured
    // ink really fits the target rectangle.
    for _ in 0..6 {
        let fit = fit_factor(&layout.ink, target);
        if fit >= 0.999 {
            break;
        }
        px_size = (px_size * fit * 0.997).max(1.0);
        layout = lay_out(font, PxScale::from(px_size), text)?;
    }

    // Centre the ink box on the target box, but always keep it inside the icon
    // border so the tray cannot clip the glyph.
    let (ink_center_x, ink_center_y) = layout.ink.center();
    let desired_dx = (target.center_x() - ink_center_x).round();
    let desired_dy = (target.center_y() - ink_center_y).round();
    let dx = clamp_offset(
        desired_dx,
        EDGE_INSET - layout.ink.min_x,
        size as f32 - EDGE_INSET - layout.ink.max_x,
    );
    let dy = clamp_offset(
        desired_dy,
        EDGE_INSET - layout.ink.min_y,
        size as f32 - EDGE_INSET - layout.ink.max_y,
    );
    Some((layout, dx, dy))
}

fn fit_factor(ink: &InkBounds, target: TextBox) -> f32 {
    (target.width / ink.width()).min(target.height / ink.height())
}

fn clamp_offset(desired: f32, min: f32, max: f32) -> f32 {
    if min > max {
        // The ink is larger than the icon; anchor it to the top-left corner.
        min
    } else {
        desired.clamp(min, max)
    }
}

fn blend_pixel(buffer: &mut [u8], size: usize, x: usize, y: usize, color: [u8; 4], coverage: f32) {
    let index = (y * size + x) * 4;
    if index + 3 >= buffer.len() {
        return;
    }
    let src_a = (color[3] as f32 / 255.0) * coverage.clamp(0.0, 1.0);

    // Simple source-over compositing against the (usually transparent) buffer.
    let dst_a = buffer[index + 3] as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.0 {
        return;
    }
    for channel in 0..3 {
        let src = color[channel] as f32 / 255.0;
        let dst = buffer[index + channel] as f32 / 255.0;
        buffer[index + channel] = (((src * src_a + dst * dst_a * (1.0 - src_a)) / out_a) * 255.0)
            .round()
            .clamp(0.0, 255.0) as u8;
    }
    buffer[index + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

/// Fit the local Solar Hijri day into the notification icon.
pub fn render_day(day: u32, fa_digits: bool, color: [u8; 4], px: u32) -> Image<'static> {
    let size = px.clamp(16, 128) as usize;
    let mut buffer = vec![0u8; size * size * 4];
    draw_fitted_text(
        &mut buffer,
        size,
        &day.to_string(),
        day_box(size),
        color,
        fa_digits,
    );
    Image::new_owned(buffer, size as u32, size as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_renders_hours_and_minutes_with_their_own_colors() {
        let hour = [255, 0, 0, 255];
        let minute = [0, 0, 255, 255];
        let image = render_clock(&[(12, hour), (47, minute)], true, 48);
        let (top, bottom) = image.rgba().split_at(48 * 24 * 4);
        assert!(top
            .chunks_exact(4)
            .any(|pixel| pixel[0] == 255 && pixel[3] != 0));
        assert!(bottom
            .chunks_exact(4)
            .any(|pixel| pixel[2] == 255 && pixel[3] != 0));
    }

    #[test]
    fn native_date_draws_each_day_in_the_requested_digit_style_and_color() {
        let color = [64, 123, 231, 255];
        let day = render_day(29, true, color, 32);
        assert!(day.rgba().chunks_exact(4).any(|pixel| pixel == color));
        assert_eq!(day.rgba()[3], 0);
        assert_ne!(day.rgba(), render_day(30, true, color, 32).rgba());
        assert_ne!(day.rgba(), render_day(29, false, color, 32).rgba());
    }

    #[test]
    fn enabled_seconds_use_their_own_color_in_the_last_row() {
        let image = render_clock(
            &[
                (18, [255, 0, 0, 255]),
                (32, [0, 0, 255, 255]),
                (5, [0, 255, 0, 255]),
            ],
            true,
            48,
        );
        let (upper, last) = image.rgba().split_at(48 * 32 * 4);
        assert!(!upper.chunks_exact(4).any(|pixel| pixel[1] == 255));
        assert!(last
            .chunks_exact(4)
            .any(|pixel| pixel[1] == 255 && pixel[3] != 0));
    }
}
