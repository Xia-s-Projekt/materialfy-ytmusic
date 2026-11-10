//! The pure part of icons.py (IconMetrics._measure): from an Alpha8 buffer with a drawn glyph, find its
//! visible outline. Drawing the glyph (QPainter) stays in the shell; only arithmetic happens here.

/// Padding around the text block in the buffer, in pixels.
pub const PAD: f64 = 4.0;
/// The threshold for a "visible" pixel: anything fainter is edge anti-aliasing.
pub const THRESHOLD: u8 = 96;

/// Buffer size for the glyph: (int(advance) + 8, int(height) + 8).
pub fn buffer_size(advance: f64, line_height: f64) -> (usize, usize) {
    (advance as usize + 8, line_height as usize + 8)
}

/// The font's opsz axis for the point size: max(20, min(48, size)).
pub fn optical_size(size: i32) -> i32 {
    size.clamp(20, 48)
}

/// Like QRectF in ink(): x, y — the offset of the outline's center from the text block's center (advance × height),
/// width, height — the size of the outline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InkRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A `width × height` buffer with row stride `stride`, text drawn at (PAD, PAD, advance, line_height).
/// Nothing visible — None (an empty QRectF in Python).
pub fn ink_rect(
    alpha: &[u8],
    stride: usize,
    width: usize,
    height: usize,
    advance: f64,
    line_height: f64,
) -> Option<InkRect> {
    let mut bounds: Option<(usize, usize, usize, usize)> = None; // top, bottom, left, right
    for y in 0..height {
        let Some(row) = alpha.get(y * stride..y * stride + width) else {
            break;
        };
        let Some(first) = row.iter().position(|&v| v > THRESHOLD) else {
            continue;
        };
        let last = row.iter().rposition(|&v| v > THRESHOLD).unwrap_or(first);
        bounds = Some(match bounds {
            None => (y, y, first, last),
            Some((top, _, left, right)) => (top, y, left.min(first), right.max(last)),
        });
    }
    let (top, bottom, left, right) = bounds?;
    Some(InkRect {
        x: (left + right + 1) as f64 / 2.0 - (PAD + advance / 2.0),
        y: (top + bottom + 1) as f64 / 2.0 - (PAD + line_height / 2.0),
        width: (right - left + 1) as f64,
        height: (bottom - top + 1) as f64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_square() {
        // a 10×10 block with 4 padding: an 18×18 buffer, a 4×4 square exactly centered
        let (w, h) = buffer_size(10.0, 10.0);
        let mut buf = vec![0u8; w * h];
        for y in 7..11 {
            for x in 7..11 {
                buf[y * w + x] = 255;
            }
        }
        // a faint pixel is not counted
        buf[0] = 50;
        let r = ink_rect(&buf, w, w, h, 10.0, 10.0).unwrap();
        assert_eq!(
            r,
            InkRect {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 4.0
            }
        );
    }

    #[test]
    fn empty() {
        let buf = vec![10u8; 16 * 16];
        assert_eq!(ink_rect(&buf, 16, 16, 16, 8.0, 8.0), None);
        assert_eq!(optical_size(12), 20);
        assert_eq!(optical_size(64), 48);
    }
}
