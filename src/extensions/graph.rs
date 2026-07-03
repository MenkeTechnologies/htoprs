//! #7 — braille history graph.
//!
//! Finishes what `GraphMeterMode_draw` leaves as "an honest stub"
//! (`src/ported/meter.rs:353`), as an extension-side renderer. A [`Scalar`]
//! ring of one metric renders to a multi-row braille bitmap via the shared
//! [`crate::extensions::braille::Canvas`]. Newest sample at the right, bars grow up.

use std::collections::VecDeque;

use crate::extensions::braille::Canvas;

/// Bounded ring of one scalar metric over time.
pub struct Scalar {
    cap: usize,
    buf: VecDeque<f64>,
}

impl Scalar {
    pub fn new(cap: usize) -> Self {
        Scalar {
            cap: cap.max(1),
            buf: VecDeque::with_capacity(cap),
        }
    }

    pub fn push(&mut self, v: f64) {
        if self.buf.len() == self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(v);
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Render the last `width_cells*2` samples as `height_cells` braille rows,
    /// scaling values to `max`. Each dot column is one sample; bars fill from
    /// the bottom up.
    pub fn render(&self, width_cells: usize, height_cells: usize, max: f64) -> Vec<String> {
        let w_dots = width_cells.max(1) * 2;
        let h_dots = height_cells.max(1) * 4;
        let mut cv = Canvas::new(w_dots, h_dots);

        let start = self.buf.len().saturating_sub(w_dots);
        for (col, &v) in self.buf.iter().skip(start).enumerate() {
            let frac = if max > 0.0 {
                (v / max).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let filled = (frac * h_dots as f64).round() as usize;
            for up in 0..filled {
                let y = h_dots - 1 - up; // bottom-anchored
                cv.set(col, y);
            }
        }
        cv.rows()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_by_capacity() {
        let mut s = Scalar::new(4);
        for i in 0..10 {
            s.push(i as f64);
        }
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn render_dimensions() {
        let mut s = Scalar::new(64);
        for i in 0..20 {
            s.push(i as f64);
        }
        let rows = s.render(10, 3, 20.0);
        assert_eq!(rows.len(), 3); // height_cells rows
        assert_eq!(rows[0].chars().count(), 10); // width_cells cols
    }

    #[test]
    fn full_value_lights_bottom_row() {
        let mut s = Scalar::new(8);
        s.push(100.0);
        let rows = s.render(1, 2, 100.0);
        // bottom cell must be non-blank; top may or may not be full
        assert_ne!(rows[1], "\u{2800}");
    }

    #[test]
    fn zero_max_is_blank() {
        let mut s = Scalar::new(8);
        s.push(50.0);
        let rows = s.render(2, 2, 0.0);
        assert!(rows.iter().all(|r| r.chars().all(|c| c == '\u{2800}')));
    }
}
