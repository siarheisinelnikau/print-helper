use crate::i18n::tr;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PT_PER_MM: f64 = 72.0 / 25.4;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum Paper {
    #[default]
    A4,
    Letter,
    A5,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub paper: Paper,
    pub landscape: bool,
    pub per_sheet: usize,
    pub margin_mm: f64,
    pub gap_mm: f64,
    pub grayscale: bool,
    pub dpi: u32,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            paper: Paper::A4,
            landscape: false,
            per_sheet: 2,
            margin_mm: 10.0,
            gap_mm: 5.0,
            grayscale: false,
            dpi: 150,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Page {
    pub path: PathBuf,
    pub sha256: String,
    pub source_page: u32,
    pub width_pt: f64,
    pub height_pt: f64,
    /// Fraction removed from the original left, top, right and bottom edges.
    pub crop: [f64; 4],
    pub quarter_turns: u8,
}

#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    pub layout: Layout,
    pub pages: Vec<Page>,
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Page {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.width_pt.is_finite()
                && self.height_pt.is_finite()
                && self.width_pt > 0.0
                && self.height_pt > 0.0,
            tr("Invalid page dimensions")
        );
        ensure!(
            self.crop
                .iter()
                .all(|x| x.is_finite() && *x >= 0.0 && *x <= 0.45)
                && self.crop[0] + self.crop[2] < 0.95
                && self.crop[1] + self.crop[3] < 0.95,
            tr("Crop must leave a visible page (each edge: 0–45%)")
        );
        ensure!(self.quarter_turns < 4, tr("Invalid rotation"));
        Ok(())
    }
    pub fn visible_size(&self) -> (f64, f64) {
        let w = self.width_pt * (1.0 - self.crop[0] - self.crop[2]);
        let h = self.height_pt * (1.0 - self.crop[1] - self.crop[3]);
        if self.quarter_turns % 2 == 1 {
            (h, w)
        } else {
            (w, h)
        }
    }
}

impl Layout {
    pub fn paper_pt(&self) -> (f64, f64) {
        let (w, h) = match self.paper {
            Paper::A4 => (210.0, 297.0),
            Paper::Letter => (215.9, 279.4),
            Paper::A5 => (148.0, 210.0),
        };
        if self.landscape {
            (h * PT_PER_MM, w * PT_PER_MM)
        } else {
            (w * PT_PER_MM, h * PT_PER_MM)
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            [1, 2, 4].contains(&self.per_sheet),
            tr("Choose 1, 2 or 4 pages per sheet")
        );
        ensure!([150, 300].contains(&self.dpi), tr("Choose 150 or 300 DPI"));
        ensure!(
            self.margin_mm.is_finite() && (0.0..=50.0).contains(&self.margin_mm),
            tr("Invalid margin")
        );
        ensure!(
            self.gap_mm.is_finite() && (0.0..=20.0).contains(&self.gap_mm),
            tr("Invalid gap")
        );
        let (w, h) = self.paper_pt();
        ensure!(
            w.min(h) > (2.0 * self.margin_mm + self.gap_mm) * PT_PER_MM + 10.0,
            tr("Margins and gap leave no printable area")
        );
        Ok(())
    }
    pub fn sheet_count(&self, pages: usize) -> usize {
        pages.div_ceil(self.per_sheet.max(1))
    }
    pub fn cells(&self) -> Vec<Rect> {
        let (pw, ph) = self.paper_pt();
        let (cols, rows) = match self.per_sheet {
            4 => (2, 2),
            2 if self.landscape => (2, 1),
            2 => (1, 2),
            _ => (1, 1),
        };
        let m = self.margin_mm * PT_PER_MM;
        let g = self.gap_mm * PT_PER_MM;
        let w = (pw - 2.0 * m - g * (cols - 1) as f64) / cols as f64;
        let h = (ph - 2.0 * m - g * (rows - 1) as f64) / rows as f64;
        (0..rows)
            .flat_map(|r| {
                (0..cols).map(move |c| Rect {
                    x: m + c as f64 * (w + g),
                    y: m + r as f64 * (h + g),
                    w,
                    h,
                })
            })
            .collect()
    }
}

impl Project {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.format_version == 1, tr("Unsupported project format"));
        ensure!(
            self.pages.len() <= 500,
            tr("A project can contain at most 500 pages")
        );
        self.layout.validate()?;
        for p in &self.pages {
            p.validate()?;
        }
        Ok(())
    }
}

pub fn fit(size: (f64, f64), cell: Rect) -> Rect {
    let scale = (cell.w / size.0).min(cell.h / size.1);
    let (w, h) = (size.0 * scale, size.1 * scale);
    Rect {
        x: cell.x + (cell.w - w) / 2.0,
        y: cell.y + (cell.h - h) / 2.0,
        w,
        h,
    }
}
