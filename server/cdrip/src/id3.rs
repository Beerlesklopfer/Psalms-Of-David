// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! A small ID3v2.4 writer: text frames in UTF-8 and one front cover. ID3v2 is the binding tag
//! format of the library; ID3v1 is never written.

/// What goes into the tag of one track.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub genre: String,
    /// The value of the track number frame (e.g. `205`), already computed by the caller's scheme.
    pub track: u32,
    pub disc: u32,
    pub discs_total: Option<u32>,
    /// The cover picture (JPEG or PNG bytes).
    pub cover: Option<Vec<u8>>,
}

/// Size as four 7-bit bytes ("synchsafe"), as ID3v2.4 wants it.
fn syncsafe(n: u32) -> [u8; 4] {
    [((n >> 21) & 0x7f) as u8, ((n >> 14) & 0x7f) as u8, ((n >> 7) & 0x7f) as u8, (n & 0x7f) as u8]
}

fn frame(id: &str, body: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(body.len() + 10);
    f.extend_from_slice(id.as_bytes());
    f.extend_from_slice(&syncsafe(body.len() as u32));
    f.extend_from_slice(&[0, 0]);
    f.extend_from_slice(body);
    f
}

fn text_frame(id: &str, text: &str) -> Vec<u8> {
    let mut body = vec![3u8]; // UTF-8
    body.extend_from_slice(text.as_bytes());
    frame(id, &body)
}

/// MIME type of a picture from its first bytes, if it is a JPEG or a PNG.
pub fn picture_mime(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.starts_with(&[0x89, b'P', b'N', b'G']) {
        Some("image/png")
    } else {
        None
    }
}

/// The complete ID3v2.4 tag, to be put in front of the audio data.
pub fn build(tags: &Tags) -> Vec<u8> {
    let mut frames = Vec::new();
    frames.extend(text_frame("TIT2", &tags.title));
    frames.extend(text_frame("TPE1", &tags.artist));
    frames.extend(text_frame("TPE2", &tags.artist));
    frames.extend(text_frame("TALB", &tags.album));
    frames.extend(text_frame("TRCK", &tags.track.to_string()));
    let disc = match tags.discs_total {
        Some(t) => format!("{}/{}", tags.disc, t),
        None => tags.disc.to_string(),
    };
    frames.extend(text_frame("TPOS", &disc));
    if !tags.genre.is_empty() {
        frames.extend(text_frame("TCON", &tags.genre));
    }
    if let Some(cover) = &tags.cover
        && let Some(mime) = picture_mime(cover)
    {
        let mut body = vec![0u8]; // ISO-8859-1 for the MIME type and the (empty) description
        body.extend_from_slice(mime.as_bytes());
        body.extend_from_slice(&[0, 3, 0]); // end of MIME, picture type 3 = front cover, empty description
        body.extend_from_slice(cover);
        frames.extend(frame("APIC", &body));
    }
    let mut tag = Vec::with_capacity(frames.len() + 10);
    tag.extend_from_slice(b"ID3");
    tag.extend_from_slice(&[4, 0, 0]);
    tag.extend_from_slice(&syncsafe(frames.len() as u32));
    tag.extend(frames);
    tag
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Reads the text frames of a tag built here (test helper, also used by the pipeline tests).
    pub fn read_frames(tag: &[u8]) -> Vec<(String, Vec<u8>)> {
        assert_eq!(&tag[..3], b"ID3");
        assert_eq!(&tag[3..6], &[4, 0, 0], "version 2.4, no flags");
        let size = tag[6..10].iter().fold(0usize, |a, b| {
            assert!(*b < 0x80, "synchsafe");
            (a << 7) | *b as usize
        });
        let mut out = Vec::new();
        let mut p = 10;
        while p < 10 + size {
            let id = String::from_utf8(tag[p..p + 4].to_vec()).unwrap();
            let n = tag[p + 4..p + 8].iter().fold(0usize, |a, b| (a << 7) | *b as usize);
            out.push((id, tag[p + 10..p + 10 + n].to_vec()));
            p += 10 + n;
        }
        assert_eq!(p, 10 + size);
        out
    }

    fn sample() -> Tags {
        Tags { title: "Ünï_CD02_TR05".into(), artist: "Ken Follett".into(), album: "Sturz".into(), genre: "Audiobook".into(), track: 205, disc: 2, discs_total: Some(12), cover: None }
    }

    #[test]
    fn frames_and_values() {
        let f = read_frames(&build(&sample()));
        let get = |id: &str| String::from_utf8(f.iter().find(|(i, _)| i == id).unwrap().1[1..].to_vec()).unwrap();
        assert_eq!(get("TIT2"), "Ünï_CD02_TR05");
        assert_eq!(get("TPE1"), "Ken Follett");
        assert_eq!(get("TPE2"), "Ken Follett");
        assert_eq!(get("TALB"), "Sturz");
        assert_eq!(get("TRCK"), "205");
        assert_eq!(get("TPOS"), "2/12");
        assert_eq!(get("TCON"), "Audiobook");
        assert!(f.iter().all(|(_, b)| b[0] == 3), "UTF-8 encoding byte");
    }

    #[test]
    fn disc_without_total_and_without_genre() {
        let mut t = sample();
        t.discs_total = None;
        t.genre.clear();
        let f = read_frames(&build(&t));
        assert!(f.iter().any(|(i, b)| i == "TPOS" && &b[1..] == b"2"));
        assert!(f.iter().all(|(i, _)| i != "TCON"));
    }

    #[test]
    fn cover_only_if_it_is_a_picture() {
        let mut t = sample();
        t.cover = Some(vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3]);
        let f = read_frames(&build(&t));
        let apic = &f.iter().find(|(i, _)| i == "APIC").unwrap().1;
        assert!(apic.windows(10).any(|w| w == b"image/jpeg"));
        assert!(apic.ends_with(&[1, 2, 3]));
        t.cover = Some(b"not a picture".to_vec());
        assert!(read_frames(&build(&t)).iter().all(|(i, _)| i != "APIC"));
    }

    #[test]
    fn syncsafe_sizes() {
        assert_eq!(syncsafe(0x7f), [0, 0, 0, 0x7f]);
        assert_eq!(syncsafe(128), [0, 0, 1, 0]);
    }
}
