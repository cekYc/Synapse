// ============================================================
// Synapse — Vision Checks for Flow Nodes
// ============================================================
// Turns an IR `VisionCheck` into something the executor can
// evaluate repeatedly. Colors are parsed and template images are
// loaded and prepared once, so a trigger polling the screen only
// pays for capture + matching on each poll.
//
// Errors from `PreparedCheck::new` are configuration problems
// (bad color, missing or blank template); errors from `evaluate`
// come from screen capture.
// ============================================================

use crate::engine::ir::{Region, VisionCheck};
use crate::vision::pixel::{self, Rgb};
use crate::vision::template::Template;
use std::path::Path;

/// Where a check matched, in screen coordinates. For images this is the
/// center of the match.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VisionHit {
    pub x: i32,
    pub y: i32,
    pub confidence: Option<f64>,
}

pub enum PreparedCheck {
    Pixel {
        x: i32,
        y: i32,
        color: Rgb,
        tolerance: u32,
    },
    ColorInRegion {
        region: Region,
        color: Rgb,
        tolerance: u32,
    },
    Image {
        template: Template,
        confidence: f64,
        region: Option<Region>,
    },
}

impl PreparedCheck {
    pub fn new(check: &VisionCheck) -> Result<Self, String> {
        Ok(match check {
            VisionCheck::Pixel { x, y, color, tolerance } => Self::Pixel {
                x: *x,
                y: *y,
                color: pixel::parse_hex_color(color)?,
                tolerance: *tolerance,
            },
            VisionCheck::ColorInRegion { region, color, tolerance } => Self::ColorInRegion {
                region: *region,
                color: pixel::parse_hex_color(color)?,
                tolerance: *tolerance,
            },
            VisionCheck::Image { template_path, confidence, region } => {
                if template_path.trim().is_empty() {
                    return Err("No template image selected".into());
                }
                Self::Image {
                    template: Template::load(template_path)?,
                    confidence: *confidence,
                    region: *region,
                }
            }
        })
    }

    /// Capture the screen and evaluate; `Ok(None)` when the check does not hold
    pub fn evaluate(&self) -> Result<Option<VisionHit>, String> {
        match self {
            Self::Pixel { x, y, color, tolerance } => Ok(pixel::pixel_matches(*x, *y, *color, *tolerance)?
                .then_some(VisionHit { x: *x, y: *y, confidence: None })),
            Self::ColorInRegion { region, color, tolerance } => {
                Ok(pixel::find_color(Some(region.as_tuple()), *color, *tolerance)?
                    .map(|(x, y)| VisionHit { x, y, confidence: None }))
            }
            Self::Image { template, confidence, region } => Ok(template
                .find(*confidence, region.map(|r| r.as_tuple()))?
                .map(|m| VisionHit {
                    x: m.center_x,
                    y: m.center_y,
                    confidence: Some(m.confidence),
                })),
        }
    }
}

/// Short description of a check for execution logs
pub fn describe(check: &VisionCheck) -> String {
    match check {
        VisionCheck::Pixel { x, y, color, tolerance } => {
            format!("pixel ({x}, {y}) ≈ {color} (±{tolerance})")
        }
        VisionCheck::ColorInRegion { region, color, tolerance } => format!(
            "{color} (±{tolerance}) in {}x{} at ({}, {})",
            region.w, region.h, region.x, region.y
        ),
        VisionCheck::Image { template_path, confidence, .. } => {
            let name = Path::new(template_path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| template_path.clone());
            format!("image '{name}' (≥ {:.0}%)", confidence * 100.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Write a 24-bit bottom-up BMP with a per-pixel color function
    fn write_bmp(name: &str, w: u32, h: u32, color: impl Fn(u32, u32) -> [u8; 3]) -> PathBuf {
        let stride = (w * 3).div_ceil(4) * 4;
        let size = 54 + stride * h;
        let mut data = Vec::with_capacity(size as usize);
        data.extend_from_slice(b"BM");
        data.extend_from_slice(&size.to_le_bytes());
        data.extend_from_slice(&[0; 4]);
        data.extend_from_slice(&54u32.to_le_bytes());
        data.extend_from_slice(&40u32.to_le_bytes());
        data.extend_from_slice(&(w as i32).to_le_bytes());
        data.extend_from_slice(&(h as i32).to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&24u16.to_le_bytes());
        data.extend_from_slice(&[0; 24]);
        for y in (0..h).rev() {
            let mut row: Vec<u8> = (0..w)
                .flat_map(|x| {
                    let [r, g, b] = color(x, y);
                    [b, g, r]
                })
                .collect();
            row.resize(stride as usize, 0);
            data.extend_from_slice(&row);
        }

        let path = std::env::temp_dir().join(format!("synapse-test-{}-{name}.bmp", std::process::id()));
        std::fs::write(&path, data).unwrap();
        path
    }

    fn image_check(path: &str) -> VisionCheck {
        VisionCheck::Image {
            template_path: path.into(),
            confidence: 0.9,
            region: None,
        }
    }

    #[test]
    fn invalid_colors_are_rejected_up_front() {
        let check = VisionCheck::Pixel { x: 0, y: 0, color: "red".into(), tolerance: 0 };
        assert!(PreparedCheck::new(&check).is_err());

        let check = VisionCheck::ColorInRegion {
            region: Region { x: 0, y: 0, w: 10, h: 10 },
            color: "#12345".into(),
            tolerance: 0,
        };
        assert!(PreparedCheck::new(&check).is_err());
    }

    #[test]
    fn missing_or_empty_template_is_rejected_up_front() {
        assert!(PreparedCheck::new(&image_check("  ")).is_err());
        let err = PreparedCheck::new(&image_check("/definitely/not/here.bmp")).err().unwrap();
        assert!(err.contains("not found"), "{err}");
    }

    #[test]
    fn templates_are_loaded_and_validated() {
        let textured = write_bmp("textured", 7, 5, |x, y| [(x * 30) as u8, (y * 50) as u8, 90]);
        assert!(PreparedCheck::new(&image_check(textured.to_str().unwrap())).is_ok());

        let blank = write_bmp("blank", 6, 6, |_, _| [40, 40, 40]);
        let err = PreparedCheck::new(&image_check(blank.to_str().unwrap())).err().unwrap();
        assert!(err.contains("blank"), "{err}");

        let _ = std::fs::remove_file(textured);
        let _ = std::fs::remove_file(blank);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn capture_errors_surface_from_evaluate() {
        let check = VisionCheck::Pixel { x: 1, y: 2, color: "#FFFFFF".into(), tolerance: 5 };
        let prepared = PreparedCheck::new(&check).unwrap();
        assert!(prepared.evaluate().is_err());
    }

    #[test]
    fn describes_checks_for_logs() {
        assert_eq!(
            describe(&image_check("C:/templates/ok_button.bmp")),
            "image 'ok_button.bmp' (≥ 90%)"
        );
        let check = VisionCheck::Pixel { x: 3, y: 4, color: "#FF0000".into(), tolerance: 10 };
        assert_eq!(describe(&check), "pixel (3, 4) ≈ #FF0000 (±10)");
    }
}
