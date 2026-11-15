//! Dynamic tray icon rendering.
//!
//! The Windows tray icon shows today's Persian day number. Icons are rendered
//! in-process with `ab_glyph` from embedded Vazirmatn fonts, directly into an
//! RGBA buffer (no `image` crate needed). Two fonts are embedded:
//!
//! * `Vazirmatn-FD-Bold.ttf` – the "Farsi digits" variant, where ASCII digits
//!   render as ۰۱۲۳۴۵۶۷۸۹;
//! * `Vazirmatn-Bold.ttf` – normal digits, used when the user prefers Latin
//!   digits in the UI.
//!
//! The glyph colour follows the Windows app theme so the number stays readable
//! on light and dark taskbars.
//!
//! Text is *auto-fitted*: the ink bounding box of the run is measured with
//! `ab_glyph`, the font scale is chosen so that ink fills its target rectangle
//! in the constraining dimension, and only then is the text rasterised
//! centred. The old implementation used fixed ratios of the icon size, which
//! left the glyph ink at roughly half the icon height because the ink of a
//! digit is only about 70% of its em box.

use std::sync::OnceLock;

use ab_glyph::{point, Font, FontArc, Glyph, GlyphId, PxScale, ScaleFont};
use tauri::image::Image;

use crate::settings::TrayIconMode;

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

/// Target box for `TrayIconMode::Day`: the whole icon minus the border inset.
fn day_box(size: usize) -> TextBox {
    let side = size as f32 - 2.0 * EDGE_INSET;
    TextBox {
        x: EDGE_INSET,
        y: EDGE_INSET,
        width: side,
        height: side,
    }
}

/// Render the tray icon at `px` logical pixels.
///
/// * `day` – Persian day-of-month (1..=31)
/// * `fa_digits` – Persian (۰-۹) or Latin (0-9) digits
/// * `light_text` – true when the number must be light (dark taskbar)
pub fn render(
    day: u32,
    fa_digits: bool,
    mode: TrayIconMode,
    light_text: bool,
    px: u32,
) -> Image<'static> {
    let size = px.clamp(16, 128) as usize;
    let mut buffer = vec![0u8; size * size * 4];
    let color: [u8; 4] = if light_text {
        [242, 245, 250, 255]
    } else {
        [26, 30, 38, 255]
    };

    match mode {
        TrayIconMode::Static => draw_calendar_glyph(&mut buffer, size, color),
        TrayIconMode::Day => {
            let text = day.to_string();
            draw_fitted_text(&mut buffer, size, &text, day_box(size), color, fa_digits);
        }
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

/// A minimal calendar glyph for `TrayIconMode::Static`.
fn draw_calendar_glyph(buffer: &mut [u8], size: usize, color: [u8; 4]) {
    let s = size as f32;
    let margin = (s * 0.14).round();
    let stroke = (s * 0.075).max(1.0);
    let radius = (s * 0.16).max(2.0);
    let top = margin;
    let bottom = s - margin;
    let left = margin;
    let right = s - margin;

    let rounded_sd = |x: f32, y: f32| -> f32 {
        // Signed distance to the outline of the rounded rectangle.
        let half_w = (right - left) / 2.0;
        let half_h = (bottom - top) / 2.0;
        let cx = left + half_w;
        let cy = top + half_h;
        let qx = (x - cx).abs() - (half_w - radius);
        let qy = (y - cy).abs() - (half_h - radius);
        let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
        let inside = qx.max(qy).min(0.0);
        outside + inside - radius
    };

    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let sd = rounded_sd(fx, fy);
            let mut coverage = 0.0f32;

            // outline
            let outline = 1.0 - ((sd.abs() - stroke / 2.0) / 0.9).clamp(0.0, 1.0);
            coverage = coverage.max(outline);

            // header band
            let band_height = (s * 0.24).max(2.0);
            if fy >= top && fy <= top + band_height && sd < 0.0 {
                coverage = coverage.max(0.85);
            }

            // two binding dots
            let dot_r = (s * 0.055).max(1.0);
            for dot_x in [left + (right - left) * 0.32, left + (right - left) * 0.68] {
                let dx = fx - dot_x;
                let dy = fy - (top - s * 0.035);
                let dist = (dx * dx + dy * dy).sqrt();
                let dot = 1.0 - ((dist - dot_r) / 0.9).clamp(0.0, 1.0);
                coverage = coverage.max(dot);
            }

            if coverage > 0.0 {
                blend_pixel(buffer, size, x, y, color, coverage.clamp(0.0, 1.0));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    /// Inclusive pixel bounding box of every pixel with any coverage.
    fn ink_bounds_rgba(rgba: &[u8], size: usize) -> Option<(usize, usize, usize, usize)> {
        let mut min_x = usize::MAX;
        let mut min_y = usize::MAX;
        let mut max_x = 0usize;
        let mut max_y = 0usize;
        for y in 0..size {
            for x in 0..size {
                if rgba[(y * size + x) * 4 + 3] > 0 {
                    min_x = min_x.min(x);
                    min_y = min_y.min(y);
                    max_x = max_x.max(x);
                    max_y = max_y.max(y);
                }
            }
        }
        if min_x == usize::MAX {
            None
        } else {
            Some((min_x, min_y, max_x, max_y))
        }
    }

    fn ink_bounds(image: &Image<'_>) -> Option<(usize, usize, usize, usize)> {
        ink_bounds_rgba(image.rgba(), image.width() as usize)
    }

    fn ink_fill(bounds: (usize, usize, usize, usize), target: TextBox) -> (f32, f32) {
        let width = (bounds.2 - bounds.0 + 1) as f32;
        let height = (bounds.3 - bounds.1 + 1) as f32;
        (width / target.width, height / target.height)
    }

    /// The acceptance rule: the ink must fill its box in the constraining
    /// dimension (the one where it runs out of room first) to at least 90%.
    fn assert_fills(label: &str, bounds: (usize, usize, usize, usize), target: TextBox) {
        let (fill_x, fill_y) = ink_fill(bounds, target);
        let constraining = fill_x.max(fill_y);
        assert!(
            constraining >= 0.90,
            "{label}: ink fills only {fill_x:.1}% of the box width and {fill_y:.1}% of its height; \
             the constraining dimension must reach 90%"
        );
    }

    fn assert_border_clear(label: &str, bounds: (usize, usize, usize, usize), size: usize) {
        assert!(
            bounds.0 >= 1 && bounds.1 >= 1 && bounds.2 <= size - 2 && bounds.3 <= size - 2,
            "{label}: ink {bounds:?} touches the icon border (icon is {size}px), so it could be clipped"
        );
    }

    #[test]
    fn renders_each_mode_at_common_sizes() {
        for px in [16, 24, 32, 64] {
            for mode in [TrayIconMode::Day, TrayIconMode::Static] {
                let image = render(22, true, mode, true, px);
                assert_eq!(image.width(), px);
                assert_eq!(image.height(), px);
                assert_eq!(image.rgba().len(), (px * px * 4) as usize);
            }
        }
    }

    #[test]
    fn renders_non_empty_icon() {
        let image = render(22, true, TrayIconMode::Day, true, 32);
        let opaque = image
            .rgba()
            .chunks_exact(4)
            .filter(|pixel| pixel[3] > 0)
            .count();
        assert!(opaque > 30, "expected visible glyph pixels, got {opaque}");
    }

    /// 1, 2 and 3 digit days must fill the Day-mode box and stay off the edge.
    #[test]
    fn day_mode_ink_fills_the_icon() {
        for px in [16u32, 24, 32, 64] {
            for day in [1u32, 7, 22, 31, 123] {
                for fa_digits in [true, false] {
                    let image = render(day, fa_digits, TrayIconMode::Day, true, px);
                    let bounds = ink_bounds(&image).expect("day mode must draw ink");
                    let label = format!("day {day} at {px}px (fa_digits={fa_digits})");
                    assert_fills(&label, bounds, day_box(px as usize));
                    assert_border_clear(&label, bounds, px as usize);
                }
            }
        }
    }

    /// The fixed calendar glyph must draw ink at every size.
    #[test]
    fn static_mode_draws_ink() {
        for px in [16u32, 24, 32, 64] {
            let image = render(22, true, TrayIconMode::Static, true, px);
            assert!(
                ink_bounds(&image).is_some(),
                "static mode drew nothing at {px}px"
            );
        }
    }

    /// Regression guard for the reported bug: the auto-fitted number must be
    /// clearly larger than the old fixed-ratio rendering.
    #[test]
    fn auto_fit_is_larger_than_the_old_fixed_ratios() {
        for px in [16u32, 24, 32, 64] {
            for day in [7u32, 22] {
                let size = px as usize;
                let new_image = render(day, true, TrayIconMode::Day, true, px);
                let new_bounds = ink_bounds(&new_image).expect("new render must draw ink");

                let old_buffer = render_old(day, true, px);
                let old_bounds =
                    ink_bounds_rgba(&old_buffer, size).expect("old render must draw ink");

                let new_height = (new_bounds.3 - new_bounds.1 + 1) as f32;
                let old_height = (old_bounds.3 - old_bounds.1 + 1) as f32;
                assert!(
                    new_height >= old_height * 1.15,
                    "day {day} at {px}px: new ink height {new_height} is not clearly larger than the old {old_height}"
                );
            }
        }
    }

    /// Dump PPM previews (only when `ICON_DUMP` is set) so the result can be
    /// inspected visually as PNGs. `ICON_DUMP=1` writes to
    /// `<repo>/.artifacts/icon-verify`; any other value is used as the directory.
    #[test]
    fn dumps_previews_when_requested() {
        let Ok(requested) = std::env::var("ICON_DUMP") else {
            return;
        };
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let requested = requested.trim();
        let out_dir: PathBuf = if requested.is_empty() || requested == "1" {
            repo_root.join(".artifacts").join("icon-verify")
        } else {
            PathBuf::from(requested)
        };
        std::fs::create_dir_all(&out_dir).expect("create preview directory");

        let background = [24u8, 26, 30];
        let sizes = [16u32, 24, 32, 64];

        for &px in &sizes {
            // New auto-fitted rendering.
            for (label, mode, day) in [
                ("day-7", TrayIconMode::Day, 7u32),
                ("day-22", TrayIconMode::Day, 22),
                ("day-123", TrayIconMode::Day, 123),
                ("static", TrayIconMode::Static, 22),
            ] {
                let image = render(day, true, mode, true, px);
                write_ppm_from_image(
                    &out_dir.join(format!("new-{label}-{px}.ppm")),
                    &image,
                    background,
                );
            }

            // Old fixed-ratio rendering, for the side-by-side comparison.
            for (label, day) in [("old-day-7", 7u32), ("old-day-22", 22)] {
                let rgba = render_old(day, true, px);
                write_ppm_from_rgba(
                    &out_dir.join(format!("{label}-{px}.ppm")),
                    px as usize,
                    &rgba,
                    background,
                );
            }
        }

        // Print the measured fill percentages so they can be reported verbatim.
        println!("icon previews written to {}", out_dir.display());
        println!("size day  fill-width fill-height (target box)");
        for &px in &sizes {
            let size = px as usize;
            for day in [7u32, 22, 123] {
                let image = render(day, true, TrayIconMode::Day, true, px);
                if let Some(bounds) = ink_bounds(&image) {
                    let target = day_box(size);
                    let (fx, fy) = ink_fill(bounds, target);
                    println!(
                        "{:>4} {:>4}  {:>9.1}%  {:>10.1}%  ({:.0}x{:.0})",
                        px,
                        day,
                        fx * 100.0,
                        fy * 100.0,
                        target.width,
                        target.height
                    );
                }
            }
        }

        // How much of the icon the day number used to take (fixed ratios) vs
        // now (auto-fit), for the bug report.
        println!("\nday-number ink height as % of the icon: old fixed ratios -> new auto-fit");
        for &px in &sizes {
            let size = px as usize;
            for day in [7u32, 22] {
                let mut old_buffer = vec![0u8; size * size * 4];
                draw_centered_text_old(
                    &mut old_buffer,
                    size,
                    &day.to_string(),
                    size as f32 * 0.78,
                    [242, 245, 250, 255],
                    0.0,
                    true,
                );
                let Some(old_bounds) = ink_bounds_rgba(&old_buffer, size) else {
                    continue;
                };

                let mut new_buffer = vec![0u8; size * size * 4];
                draw_fitted_text(
                    &mut new_buffer,
                    size,
                    &day.to_string(),
                    day_box(size),
                    [242, 245, 250, 255],
                    true,
                );
                let Some(new_bounds) = ink_bounds_rgba(&new_buffer, size) else {
                    continue;
                };

                let old_pct = (old_bounds.3 - old_bounds.1 + 1) as f32 / size as f32 * 100.0;
                let new_pct = (new_bounds.3 - new_bounds.1 + 1) as f32 / size as f32 * 100.0;
                println!("{:>4}px day {:<3}  {:>5.1}% -> {:>5.1}%", px, day, old_pct, new_pct);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Old fixed-ratio renderer, kept here only to produce comparison previews
    // and the size regression guard. Never used by production code.
    // -----------------------------------------------------------------------

    fn render_old(day: u32, fa_digits: bool, px: u32) -> Vec<u8> {
        let size = px as usize;
        let mut buffer = vec![0u8; size * size * 4];
        draw_centered_text_old(
            &mut buffer,
            size,
            &day.to_string(),
            size as f32 * 0.78,
            [242, 245, 250, 255],
            0.0,
            fa_digits,
        );
        buffer
    }

    fn draw_centered_text_old(
        buffer: &mut [u8],
        size: usize,
        text: &str,
        px_size: f32,
        color: [u8; 4],
        offset_y: f32,
        fa_digits: bool,
    ) {
        let font = if fa_digits { font_fa() } else { font_latin() };
        let scale = PxScale::from(px_size.max(4.0));
        let Some(layout) = lay_out(font, scale, text) else {
            return;
        };
        let dx = (size as f32 - layout.ink.width()) / 2.0 - layout.ink.min_x;
        let dy = (size as f32 - layout.ink.height()) / 2.0 - layout.ink.min_y + offset_y;
        rasterise(font, buffer, size, &layout, dx, dy, color);
    }

    fn write_ppm_from_image(path: &Path, image: &Image<'_>, background: [u8; 3]) {
        write_ppm_from_rgba(path, image.width() as usize, image.rgba(), background);
    }

    /// Write a P6 PPM, compositing the RGBA pixels over an opaque background.
    fn write_ppm_from_rgba(path: &Path, size: usize, rgba: &[u8], background: [u8; 3]) {
        let mut data = format!("P6\n{size} {size}\n255\n").into_bytes();
        data.reserve(size * size * 3);
        for pixel in rgba.chunks_exact(4) {
            let alpha = pixel[3] as f32 / 255.0;
            for (channel, &bg) in background.iter().enumerate() {
                let value = pixel[channel] as f32 * alpha + bg as f32 * (1.0 - alpha);
                data.push(value.round().clamp(0.0, 255.0) as u8);
            }
        }
        std::fs::write(path, data).unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
    }
}
