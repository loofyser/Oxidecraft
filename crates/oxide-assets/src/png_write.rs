//! PNG screenshots: encode an RGBA frame and mint the screenshot file name.
//!
//! The writer is the port's `ImageIO.write` half (`ScreenShotHelper.java:116`);
//! the decode half stays in [`texture`](super::texture). The name mint is the
//! source's `dateFormat` (`"yyyy-MM-dd_HH.mm.ss"`, `:24` — `mm` is minutes,
//! so the middle field renders hours.minutes.seconds) with the `""`, `"_2"`,
//! `"_3"`, … collision loop (`:135-151`, starting at 2, the requested name
//! used verbatim). The mint reads UTC, not the source's local zone — the
//! shape is pinned, the zone is recorded.

use std::fs::File;
use std::path::{Path, PathBuf};

/// Errors from writing a PNG screenshot.
#[derive(Debug, thiserror::Error)]
pub enum PngWriteError {
    /// The frame's dimensions disagree with its bytes, or a dimension is zero.
    /// Carries what the caller handed over.
    #[error(
        "refusing a {width}x{height} frame with {len} bytes; 8-bit RGBA needs width * height * 4"
    )]
    BadFrame {
        /// The frame's width in texels.
        width: u32,
        /// The frame's height in texels.
        height: u32,
        /// The bytes handed over.
        len: usize,
    },
    /// The file could not be created or written.
    #[error("could not write the PNG at {path}: {source}")]
    Io {
        /// The path involved.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The encoder refused the frame.
    #[error("could not encode the PNG: {0}")]
    Encode(#[from] png::EncodingError),
}

/// Writes `rgba` — `width * height * 4` bytes of 8-bit RGBA, row-major, the
/// first row at the top — to `path` as a PNG.
///
/// A zero dimension or a byte count other than `width * height * 4` is
/// refused, never panicked over: the product is computed wide so no input
/// can overflow it.
pub fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<(), PngWriteError> {
    if width == 0 || height == 0 {
        return Err(PngWriteError::BadFrame {
            width,
            height,
            len: rgba.len(),
        });
    }
    let want = u64::from(width) * u64::from(height) * 4;
    if want != rgba.len() as u64 {
        return Err(PngWriteError::BadFrame {
            width,
            height,
            len: rgba.len(),
        });
    }
    let file = File::create(path).map_err(|source| PngWriteError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mut encoder = png::Encoder::new(file, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(rgba)?;
    writer.finish()?;
    Ok(())
}

/// Mints the screenshot file name for `now`, seconds since the Unix epoch in
/// UTC: `yyyy-MM-dd_HH.mm.ss.png`.
///
/// The calendar is computed here rather than read off a date crate — the
/// crate carries no date dependency — by the days-from-civil reckoning, so a
/// fixed stamp always mints the same name and tests inject the clock by
/// passing a stamp.
pub fn screenshot_file_name(now_unix_secs: u64) -> String {
    let days = now_unix_secs / 86_400;
    let clock = now_unix_secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour = clock / 3_600;
    let minute = (clock % 3_600) / 60;
    let second = clock % 60;
    format!("{year:04}-{month:02}-{day:02}_{hour:02}.{minute:02}.{second:02}.png")
}

/// The unused screenshot path in `dir` for `now`: the mint, then `_2`, `_3`,
/// … until the name is unused.
///
/// Only existence is probed — the caller creates the directory beside the
/// write — so a missing directory mints the bare name.
pub fn next_screenshot_path(dir: &Path, now_unix_secs: u64) -> PathBuf {
    let minted = screenshot_file_name(now_unix_secs);
    let stem = minted.strip_suffix(".png").unwrap_or(&minted);
    let mut candidate = dir.join(&minted);
    let mut suffix = 2u32;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}_{suffix}.png"));
        suffix = suffix.saturating_add(1);
    }
    candidate
}

/// The proleptic Gregorian date of `days` since the Unix epoch.
///
/// The days-from-civil reckoning: every value here is non-negative, so plain
/// division truncates the right way.
fn civil_from_days(days: u64) -> (i64, u32, u32) {
    let z = days as i64 + 719_468;
    let era = z / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_pair = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_pair + 2) / 5 + 1) as u32;
    let month = if month_pair < 10 {
        month_pair + 3
    } else {
        month_pair - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    //! The writer's round trip through the crate's own decoder, the hostile
    //! frame refusals, and the name mint with its collision loop.

    use std::fs;

    use super::*;
    use crate::texture::Texture;

    /// A frame whose bytes name their own coordinates: red carries x, green
    /// carries y, blue carries their sum, alpha stays opaque.
    fn patterned(width: u32, height: u32) -> Vec<u8> {
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[
                    (x % 256) as u8,
                    (y % 256) as u8,
                    ((x + y) % 256) as u8,
                    255,
                ]);
            }
        }
        rgba
    }

    #[test]
    fn the_writer_round_trips_through_the_crates_own_decoder() {
        // 5 by 4: a width no row stride divides evenly, so a stride
        // assumption would come back shifted.
        let dir = tempfile::tempdir().expect("a fixture directory is created");
        let path = dir.path().join("shot.png");
        let rgba = patterned(5, 4);
        write_png(&path, 5, 4, &rgba).expect("the frame writes");
        let back = Texture::from_png(&fs::read(&path).expect("the file reads"))
            .expect("the crate decodes its own encode");
        assert_eq!(back.width, 5);
        assert_eq!(back.height, 4);
        assert_eq!(back.rgba, rgba, "the texels come back byte-equal");
    }

    #[test]
    fn the_writer_round_trips_a_single_texel() {
        let dir = tempfile::tempdir().expect("a fixture directory is created");
        let path = dir.path().join("one.png");
        let rgba = [9, 20, 31, 255];
        write_png(&path, 1, 1, &rgba).expect("the frame writes");
        let back = Texture::from_png(&fs::read(&path).expect("the file reads"))
            .expect("the crate decodes its own encode");
        assert_eq!(back.rgba, rgba);
    }

    #[test]
    fn the_writer_refuses_a_zero_dimension() {
        let dir = tempfile::tempdir().expect("a fixture directory is created");
        let path = dir.path().join("zero.png");
        let empty: [u8; 0] = [];
        assert!(
            matches!(
                write_png(&path, 0, 4, &empty),
                Err(PngWriteError::BadFrame { .. })
            ),
            "zero width is refused"
        );
        assert!(
            matches!(
                write_png(&path, 4, 0, &empty),
                Err(PngWriteError::BadFrame { .. })
            ),
            "zero height is refused"
        );
        assert!(!path.exists(), "a refused frame writes nothing");
    }

    #[test]
    fn the_writer_refuses_a_byte_count_that_disagrees_with_the_dimensions() {
        let dir = tempfile::tempdir().expect("a fixture directory is created");
        let path = dir.path().join("short.png");
        let short = patterned(3, 2);
        assert!(
            matches!(
                write_png(&path, 3, 2, &short[..short.len() - 1]),
                Err(PngWriteError::BadFrame { .. })
            ),
            "one byte short is refused"
        );
        let mut long = short.clone();
        long.push(0);
        assert!(
            matches!(
                write_png(&path, 3, 2, &long),
                Err(PngWriteError::BadFrame { .. })
            ),
            "one byte over is refused"
        );
        assert!(!path.exists(), "a refused frame writes nothing");
    }

    #[test]
    fn the_mint_renders_hours_minutes_seconds() {
        // The epoch mints the zero date; the middle field is minutes, not
        // months — January at 10:30 renders `.30.`, never `.01.`.
        assert_eq!(screenshot_file_name(0), "1970-01-01_00.00.00.png");
        assert_eq!(
            screenshot_file_name(1_705_314_645),
            "2024-01-15_10.30.45.png"
        );
    }

    #[test]
    fn the_collision_loop_starts_at_two() {
        let dir = tempfile::tempdir().expect("a fixture directory is created");
        let stamp = 1_705_314_645;
        let bare = dir.path().join("2024-01-15_10.30.45.png");
        assert_eq!(next_screenshot_path(dir.path(), stamp), bare);
        fs::write(&bare, [0]).expect("the fixture file writes");
        assert_eq!(
            next_screenshot_path(dir.path(), stamp),
            dir.path().join("2024-01-15_10.30.45_2.png")
        );
        fs::write(dir.path().join("2024-01-15_10.30.45_2.png"), [0])
            .expect("the fixture file writes");
        assert_eq!(
            next_screenshot_path(dir.path(), stamp),
            dir.path().join("2024-01-15_10.30.45_3.png")
        );
    }
}
