//! Shared glyph rendering for #1 (sparkline column) and #7 (history graph).
//!
//! htoprs already defines the braille dot tables at `src/ported/meter.rs:902`
//! for `GraphMeterMode`; this is the extension-side reusable renderer. Two
//! outputs from one place:
//! - [`spark`]: a single-row block-element sparkline (`▁▂▃▄▅▆▇█`).
//! - [`Canvas`]: a 2x4-dots-per-cell braille bitmap for real graphs.

/// Block elements by height, index 0..=8 (0 = blank).
const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// One-row sparkline of `values`, one glyph per value, scaled to `max`.
///
/// `max <= 0` renders all-blank (no data / flat zero).
pub fn spark(values: &[f32], max: f32) -> String {
    if max <= 0.0 {
        return " ".repeat(values.len());
    }
    values
        .iter()
        .map(|&v| {
            let frac = (v / max).clamp(0.0, 1.0);
            let level = (frac * 8.0).round() as usize;
            BLOCKS[level.min(8)]
        })
        .collect()
}

/// A braille dot bitmap. Dot resolution is `2 * w` wide by `4 * h` tall.
pub struct Canvas {
    /// width in cells
    w: usize,
    /// height in cells
    h: usize,
    cells: Vec<u8>,
}

impl Canvas {
    /// Canvas holding at least `w_dots` x `h_dots` dots (rounded up to cells).
    pub fn new(w_dots: usize, h_dots: usize) -> Self {
        let w = w_dots.div_ceil(2).max(1);
        let h = h_dots.div_ceil(4).max(1);
        Canvas {
            w,
            h,
            cells: vec![0u8; w * h],
        }
    }

    /// Light the dot at `(x, y)` (dot coordinates, origin top-left).
    pub fn set(&mut self, x: usize, y: usize) {
        let cx = x / 2;
        let cy = y / 4;
        if cx >= self.w || cy >= self.h {
            return;
        }
        // Unicode braille bit layout, [row][col]:
        //   dot1 dot4      0x01 0x08
        //   dot2 dot5  ->  0x02 0x10
        //   dot3 dot6      0x04 0x20
        //   dot7 dot8      0x40 0x80
        const MAP: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];
        self.cells[cy * self.w + cx] |= MAP[y % 4][x % 2];
    }

    /// Render top-to-bottom as `h` strings of `w` braille chars each.
    pub fn rows(&self) -> Vec<String> {
        (0..self.h)
            .map(|cy| {
                (0..self.w)
                    .map(|cx| {
                        let bits = self.cells[cy * self.w + cx] as u32;
                        char::from_u32(0x2800 + bits).unwrap_or('?')
                    })
                    .collect()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spark_scales_full_range() {
        let s = spark(&[0.0, 50.0, 100.0], 100.0);
        let chars: Vec<char> = s.chars().collect();
        assert_eq!(chars[0], ' ');
        assert_eq!(chars[2], '█');
        assert_eq!(chars.len(), 3);
    }

    #[test]
    fn spark_no_data_is_blank() {
        assert_eq!(spark(&[1.0, 2.0], 0.0), "  ");
    }

    #[test]
    fn canvas_top_left_dot_is_2801() {
        let mut c = Canvas::new(2, 4);
        c.set(0, 0);
        assert_eq!(c.rows()[0].chars().next().unwrap(), '\u{2801}');
    }

    #[test]
    fn canvas_bottom_right_dot() {
        let mut c = Canvas::new(2, 4);
        c.set(1, 3); // dot8 -> 0x80
        assert_eq!(c.rows()[0].chars().next().unwrap(), '\u{2880}');
    }

    #[test]
    fn canvas_out_of_bounds_is_ignored() {
        let mut c = Canvas::new(2, 4);
        c.set(99, 99);
        assert_eq!(c.rows()[0], "\u{2800}");
    }
}
