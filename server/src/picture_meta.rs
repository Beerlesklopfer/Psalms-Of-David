// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! What the EXIF data of a picture says (docs/spec-pictures.md): when it was taken and where. JPEG, HEIC/HEIF, PNG, WebP and TIFF carry it;
//! a file without it has none (the caller takes the time of the file).

use std::io::BufReader;
use std::path::Path;

use exif::{In, Reader, Tag, Value};

/// When a picture was taken, as the camera wrote it: the time on its clock and, if it said so, its offset from UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Taken {
    pub civil: jiff::civil::DateTime,
    /// Seconds east of UTC (`OffsetTimeOriginal`), if the camera wrote it.
    pub offset_s: Option<i32>,
}

impl Taken {
    /// The moment in ms; without an offset in the time zone `tz` (the server's).
    pub fn millis(&self, tz: &jiff::tz::TimeZone) -> Option<i64> {
        let ts = match self.offset_s {
            Some(s) => self.civil.to_zoned(jiff::tz::TimeZone::fixed(jiff::tz::Offset::from_seconds(s).ok()?)).ok()?.timestamp(),
            None => self.civil.to_zoned(tz.clone()).ok()?.timestamp(),
        };
        Some(ts.as_millisecond())
    }
}

/// Where a picture was taken, in degrees (north and east are positive).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    pub lat: f64,
    pub lon: f64,
}

/// What the EXIF data of a file says.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Meta {
    pub taken: Option<Taken>,
    pub place: Option<Place>,
}

/// Reads the EXIF data of a file; everything is `None` when it has none or it cannot be read.
pub fn read(path: &Path) -> Meta {
    let Ok(file) = std::fs::File::open(path) else { return Meta::default() };
    let Ok(exif) = Reader::new().read_from_container(&mut BufReader::new(file)) else { return Meta::default() };
    let ascii = |tag: Tag| -> Option<String> {
        match &exif.get_field(tag, In::PRIMARY)?.value {
            Value::Ascii(v) => v.first().map(|b| String::from_utf8_lossy(b).trim().to_owned()),
            _ => None,
        }
    };
    let taken = [Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime].into_iter().find_map(|t| {
        let text = ascii(t)?;
        let civil = civil(&text)?;
        let offset = [Tag::OffsetTimeOriginal, Tag::OffsetTime].into_iter().find_map(|o| ascii(o).and_then(|s| offset_seconds(&s)));
        Some(Taken { civil, offset_s: offset })
    });
    let degrees = |tag: Tag, sign: Tag, negative: &str| -> Option<f64> {
        let Value::Rational(parts) = &exif.get_field(tag, In::PRIMARY)?.value else { return None };
        let [d, m, s] = parts.as_slice() else { return None };
        let v = d.to_f64() + m.to_f64() / 60.0 + s.to_f64() / 3600.0;
        let v = if ascii(sign).is_some_and(|r| r.eq_ignore_ascii_case(negative)) { -v } else { v };
        v.is_finite().then_some(v)
    };
    let place = match (degrees(Tag::GPSLatitude, Tag::GPSLatitudeRef, "S"), degrees(Tag::GPSLongitude, Tag::GPSLongitudeRef, "W")) {
        // 0/0 is what some cameras write when they had no fix.
        (Some(lat), Some(lon)) if (lat != 0.0 || lon != 0.0) && lat.abs() <= 90.0 && lon.abs() <= 180.0 => Some(Place { lat, lon }),
        _ => None,
    };
    Meta { taken, place }
}

/// `2019:07:14 15:03:22` (EXIF), also with `-` in the date.
fn civil(text: &str) -> Option<jiff::civil::DateTime> {
    let t = text.get(..19)?;
    let b = t.as_bytes();
    if !(matches!(b[4], b':' | b'-') && matches!(b[7], b':' | b'-') && b[10] == b' ' && b[13] == b':' && b[16] == b':') {
        return None;
    }
    let n = |r: std::ops::Range<usize>| t.get(r)?.parse::<i16>().ok();
    let (y, mo, d, h, mi, s) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    // Cameras without a set clock write zeros.
    if y < 1900 {
        return None;
    }
    jiff::civil::DateTime::new(y, i8::try_from(mo).ok()?, i8::try_from(d).ok()?, i8::try_from(h).ok()?, i8::try_from(mi).ok()?, i8::try_from(s).ok()?, 0).ok()
}

/// `+02:00` → 7200.
fn offset_seconds(text: &str) -> Option<i32> {
    let (sign, rest) = match text.as_bytes().first()? {
        b'+' => (1, &text[1..]),
        b'-' => (-1, &text[1..]),
        _ => return None,
    };
    let (h, m) = rest.split_once(':')?;
    Some(sign * (h.parse::<i32>().ok()? * 3600 + m.parse::<i32>().ok()? * 60))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exif_times_and_offsets_are_read() {
        let c = civil("2019:07:14 15:03:22").unwrap();
        assert_eq!(c.to_string(), "2019-07-14T15:03:22");
        assert!(civil("0000:00:00 00:00:00").is_none() && civil("garbage").is_none());
        assert_eq!(offset_seconds("+02:00"), Some(7200));
        assert_eq!(offset_seconds("-05:30"), Some(-19800));
        let t = Taken { civil: c, offset_s: Some(7200) };
        assert_eq!(t.millis(&jiff::tz::TimeZone::UTC), Some(1_563_109_402_000));
        let t = Taken { civil: c, offset_s: None };
        assert_eq!(t.millis(&jiff::tz::TimeZone::UTC), Some(1_563_116_602_000));
    }

    #[test]
    fn a_file_without_exif_has_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.png");
        image::RgbImage::from_pixel(4, 4, image::Rgb([0, 0, 0])).save(&p).unwrap();
        assert_eq!(read(&p), Meta::default());
        assert_eq!(read(&dir.path().join("missing.jpg")), Meta::default());
    }
}
