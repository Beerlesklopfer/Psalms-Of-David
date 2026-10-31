// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! A small writer for ZIP archives that are produced as a stream: every file is stored (method 0, no compression;
//! audio is compressed already) and its CRC and size follow the data in a data descriptor, so that nothing has to be
//! read twice or held in memory. It writes ZIP64 structures where a file, the offset of an entry or the archive
//! needs them (4 GiB and more, 65 535 entries and more); an album of lossless files can easily pass 4 GiB.
//!
//! Why not the `zip` crate: it needs a seekable writer or buffers entries, pulls in the compression libraries
//! (which this never uses) and is a heavy dependency for forty lines of fixed structures. The format is the one of
//! PKWARE's APPNOTE 6.3 (local header 4.3.7, data descriptor 4.3.9, central directory 4.3.12, ZIP64 4.5.3 and 4.3.14).
//!
//! Use: `start_entry` gives the bytes of the local header, then the caller writes the file's bytes (and tells the
//! writer how many with `data_written`), then `finish_entry` gives the descriptor; `finish` gives the central
//! directory and the end record. The caller counts the bytes it sends itself.

/// A file of this size or more (and an offset of this size or more) cannot be described in 32 bits.
pub const ZIP64_LIMIT: u64 = 0xFFFF_FFFF;
/// More entries than this need the ZIP64 end record.
const MAX_ENTRIES_32: usize = 0xFFFF;

/// General purpose flags: bit 3 (sizes and CRC follow the data) and bit 11 (names are UTF-8).
const FLAGS: u16 = 0x0008 | 0x0800;
/// 1980-01-01 00:00, the first date MS-DOS can store: the archive is the same bytes whenever it is made.
const DOS_DATE: u16 = (1 << 5) | 1;
const DOS_TIME: u16 = 0;

struct Central {
    name: Vec<u8>,
    crc: u32,
    size: u64,
    offset: u64,
    /// The sizes of this entry are ZIP64 sizes (known when its header was written).
    big: bool,
}

pub struct ZipWriter {
    /// Bytes written so far (header, data, descriptors): the offset of the next entry.
    offset: u64,
    entries: Vec<Central>,
    /// The size from which an entry gets ZIP64 sizes. `ZIP64_LIMIT` except in tests, which lower it.
    limit: u64,
    open: Option<(Vec<u8>, u64, bool)>,
}

impl Default for ZipWriter {
    fn default() -> Self {
        ZipWriter::new()
    }
}

impl ZipWriter {
    pub fn new() -> ZipWriter {
        ZipWriter::with_limit(ZIP64_LIMIT)
    }

    /// Like `new`, with the size from which ZIP64 structures are used lowered, so that tests can reach them with small files.
    pub fn with_limit(limit: u64) -> ZipWriter {
        ZipWriter { offset: 0, entries: Vec::new(), limit, open: None }
    }

    /// The local file header of the next entry. `size` is the size the file has now: it decides whether the entry
    /// carries ZIP64 sizes, so the caller has to send exactly that many bytes.
    pub fn start_entry(&mut self, name: &str, size: u64) -> Vec<u8> {
        let name = name.as_bytes().to_vec();
        let big = size >= self.limit;
        let mut h = Vec::with_capacity(30 + name.len() + 20);
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&(if big { 45u16 } else { 20 }).to_le_bytes());
        h.extend_from_slice(&FLAGS.to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes()); // method 0: stored
        h.extend_from_slice(&DOS_TIME.to_le_bytes());
        h.extend_from_slice(&DOS_DATE.to_le_bytes());
        h.extend_from_slice(&0u32.to_le_bytes()); // CRC: in the descriptor
        let size32 = if big { 0xFFFF_FFFFu32 } else { 0 };
        h.extend_from_slice(&size32.to_le_bytes());
        h.extend_from_slice(&size32.to_le_bytes());
        h.extend_from_slice(&(name.len() as u16).to_le_bytes());
        h.extend_from_slice(&(if big { 20u16 } else { 0 }).to_le_bytes());
        h.extend_from_slice(&name);
        if big {
            // ZIP64 extra field with both sizes; the real ones follow in the descriptor.
            h.extend_from_slice(&1u16.to_le_bytes());
            h.extend_from_slice(&16u16.to_le_bytes());
            h.extend_from_slice(&[0; 16]);
        }
        self.open = Some((name, self.offset, big));
        self.offset += h.len() as u64;
        h
    }

    /// Counts bytes of file data that the caller sends after the header.
    pub fn data_written(&mut self, n: u64) {
        self.offset += n;
    }

    /// The data descriptor that ends the entry begun last, with the CRC and the size of what was sent.
    pub fn finish_entry(&mut self, crc: u32, size: u64) -> Vec<u8> {
        let (name, offset, big) = self.open.take().expect("finish_entry without start_entry");
        let mut d = Vec::with_capacity(24);
        d.extend_from_slice(&0x0807_4b50u32.to_le_bytes());
        d.extend_from_slice(&crc.to_le_bytes());
        if big {
            d.extend_from_slice(&size.to_le_bytes());
            d.extend_from_slice(&size.to_le_bytes());
        } else {
            d.extend_from_slice(&(size as u32).to_le_bytes());
            d.extend_from_slice(&(size as u32).to_le_bytes());
        }
        self.offset += d.len() as u64;
        self.entries.push(Central { name, crc, size, offset, big });
        d
    }

    /// The central directory and the end record (with the ZIP64 records where the archive needs them).
    pub fn finish(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        let cd_offset = self.offset;
        for e in &self.entries {
            let far = e.offset >= self.limit;
            let mut extra = Vec::new();
            if e.big {
                extra.extend_from_slice(&e.size.to_le_bytes());
                extra.extend_from_slice(&e.size.to_le_bytes());
            }
            if far {
                extra.extend_from_slice(&e.offset.to_le_bytes());
            }
            let wide = e.big || far;
            out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            out.extend_from_slice(&(if wide { 45u16 } else { 20 }).to_le_bytes()); // made by
            out.extend_from_slice(&(if wide { 45u16 } else { 20 }).to_le_bytes()); // needed
            out.extend_from_slice(&FLAGS.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&DOS_TIME.to_le_bytes());
            out.extend_from_slice(&DOS_DATE.to_le_bytes());
            out.extend_from_slice(&e.crc.to_le_bytes());
            let size32 = if e.big { 0xFFFF_FFFFu32 } else { e.size as u32 };
            out.extend_from_slice(&size32.to_le_bytes());
            out.extend_from_slice(&size32.to_le_bytes());
            out.extend_from_slice(&(e.name.len() as u16).to_le_bytes());
            out.extend_from_slice(&(if extra.is_empty() { 0u16 } else { extra.len() as u16 + 4 }).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // comment
            out.extend_from_slice(&0u16.to_le_bytes()); // disk
            out.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
            out.extend_from_slice(&0u32.to_le_bytes()); // external attributes
            out.extend_from_slice(&(if far { 0xFFFF_FFFFu32 } else { e.offset as u32 }).to_le_bytes());
            out.extend_from_slice(&e.name);
            if !extra.is_empty() {
                out.extend_from_slice(&1u16.to_le_bytes());
                out.extend_from_slice(&(extra.len() as u16).to_le_bytes());
                out.extend_from_slice(&extra);
            }
        }
        let cd_size = out.len() as u64;
        let count = self.entries.len();
        if count > MAX_ENTRIES_32 || cd_offset >= self.limit || cd_size >= self.limit {
            let record_at = cd_offset + cd_size;
            out.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
            out.extend_from_slice(&44u64.to_le_bytes());
            out.extend_from_slice(&45u16.to_le_bytes());
            out.extend_from_slice(&45u16.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(count as u64).to_le_bytes());
            out.extend_from_slice(&(count as u64).to_le_bytes());
            out.extend_from_slice(&cd_size.to_le_bytes());
            out.extend_from_slice(&cd_offset.to_le_bytes());
            out.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&record_at.to_le_bytes());
            out.extend_from_slice(&1u32.to_le_bytes());
        }
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        let count16 = if count > MAX_ENTRIES_32 { 0xFFFF } else { count as u16 };
        out.extend_from_slice(&count16.to_le_bytes());
        out.extend_from_slice(&count16.to_le_bytes());
        out.extend_from_slice(&(if cd_size >= self.limit { 0xFFFF_FFFFu32 } else { cd_size as u32 }).to_le_bytes());
        out.extend_from_slice(&(if cd_offset >= self.limit { 0xFFFF_FFFFu32 } else { cd_offset as u32 }).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        self.offset += out.len() as u64;
        out
    }
}

/// The running CRC-32 (IEEE, the one of ZIP) of the bytes of a file.
pub struct Crc32(u32);

const TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

impl Default for Crc32 {
    fn default() -> Self {
        Crc32::new()
    }
}

impl Crc32 {
    pub fn new() -> Crc32 {
        Crc32(0xFFFF_FFFF)
    }

    pub fn update(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = TABLE[((self.0 ^ u32::from(*b)) & 0xFF) as usize] ^ (self.0 >> 8);
        }
    }

    pub fn finish(&self) -> u32 {
        !self.0
    }
}

/// One entry of the central directory, as a reader sees it (used by the tests of the archive).
#[derive(Debug, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    pub crc: u32,
    pub size: u64,
    pub offset: u64,
}

/// Reads the entries of an archive from its central directory, ZIP64 included. Meant to check what the writer made
/// (the tests of the album download use it); it is not a general reader.
pub fn list(archive: &[u8]) -> Result<Vec<Listed>, String> {
    let u16_at = |i: usize| archive.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |i: usize| archive.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let u64_at = |i: usize| archive.get(i..i + 8).map(|b| u64::from_le_bytes(b.try_into().expect("eight bytes")));
    let eocd = (0..archive.len().saturating_sub(21)).rev().find(|&i| u32_at(i) == Some(0x0605_4b50)).ok_or("no end record")?;
    let mut count = u64::from(u16_at(eocd + 10).ok_or("short")?);
    let mut cd_offset = u64::from(u32_at(eocd + 16).ok_or("short")?);
    if cd_offset == 0xFFFF_FFFF || count == 0xFFFF {
        let locator = eocd.checked_sub(20).filter(|&i| u32_at(i) == Some(0x0706_4b50)).ok_or("no ZIP64 locator")?;
        let record = u64_at(locator + 8).ok_or("short")? as usize;
        if u32_at(record) != Some(0x0606_4b50) {
            return Err("no ZIP64 end record".into());
        }
        count = u64_at(record + 32).ok_or("short")?;
        cd_offset = u64_at(record + 48).ok_or("short")?;
    }
    let mut at = cd_offset as usize;
    let mut out = Vec::new();
    for _ in 0..count {
        if u32_at(at) != Some(0x0201_4b50) {
            return Err(format!("no central header at {at}"));
        }
        let crc = u32_at(at + 16).ok_or("short")?;
        let (mut size, mut offset) = (u64::from(u32_at(at + 24).ok_or("short")?), u64::from(u32_at(at + 42).ok_or("short")?));
        let (name_len, extra_len, comment_len) = (u16_at(at + 28).ok_or("short")? as usize, u16_at(at + 30).ok_or("short")? as usize, u16_at(at + 32).ok_or("short")? as usize);
        let name = String::from_utf8(archive.get(at + 46..at + 46 + name_len).ok_or("short")?.to_vec()).map_err(|_| "name is not UTF-8")?;
        let mut extra = &archive[at + 46 + name_len..at + 46 + name_len + extra_len];
        while extra.len() >= 4 {
            let (id, len) = (u16::from_le_bytes([extra[0], extra[1]]), u16::from_le_bytes([extra[2], extra[3]]) as usize);
            let body = extra.get(4..4 + len).ok_or("short extra")?;
            if id == 1 {
                let mut k = 0;
                if size == 0xFFFF_FFFF {
                    size = u64::from_le_bytes(body.get(k..k + 8).ok_or("short")?.try_into().expect("eight bytes"));
                    k += 16; // uncompressed and compressed size
                }
                if offset == 0xFFFF_FFFF {
                    offset = u64::from_le_bytes(body.get(k..k + 8).ok_or("short")?.try_into().expect("eight bytes"));
                }
            }
            extra = &extra[4 + len..];
        }
        out.push(Listed { name, crc, size, offset });
        at += 46 + name_len + extra_len + comment_len;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(limit: u64, files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = ZipWriter::with_limit(limit);
        let mut out = Vec::new();
        for (name, data) in files {
            out.extend(z.start_entry(name, data.len() as u64));
            out.extend_from_slice(data);
            z.data_written(data.len() as u64);
            let mut crc = Crc32::new();
            crc.update(data);
            out.extend(z.finish_entry(crc.finish(), data.len() as u64));
        }
        out.extend(z.finish());
        out
    }

    #[test]
    fn the_crc_is_the_one_of_zip() {
        let mut c = Crc32::new();
        c.update(b"123456789");
        assert_eq!(c.finish(), 0xCBF4_3926);
    }

    #[test]
    fn the_central_directory_lists_what_was_written() {
        let archive = build(ZIP64_LIMIT, &[("a/one.mp3", b"hello"), ("Ü ä.txt", b""), ("b", &[7; 1000])]);
        let listed = list(&archive).unwrap();
        assert_eq!(listed.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["a/one.mp3", "Ü ä.txt", "b"]);
        assert_eq!(listed.iter().map(|e| e.size).collect::<Vec<_>>(), [5, 0, 1000]);
        // The data lies where the central directory says (after a header of 30 bytes and the name).
        let first = &listed[0];
        let start = first.offset as usize + 30 + first.name.len();
        assert_eq!(&archive[start..start + 5], b"hello");
        // The end record is the plain one: nothing here needs ZIP64.
        assert!(!archive.windows(4).any(|w| w == 0x0606_4b50u32.to_le_bytes()));
    }

    #[test]
    fn small_limits_reach_the_zip64_structures_and_they_read_back() {
        // Entries of 100 bytes and more get ZIP64 sizes; the offset of the later entries and the directory pass 300.
        let big = [9u8; 150];
        let archive = build(100, &[("small", b"abc"), ("big1", &big), ("big2", &big), ("after", b"xyz")]);
        let listed = list(&archive).unwrap();
        assert_eq!(listed.iter().map(|e| (e.name.as_str(), e.size)).collect::<Vec<_>>(), [("small", 3), ("big1", 150), ("big2", 150), ("after", 3)]);
        assert!(listed[3].offset >= 300, "an offset behind the limit is written in the extra field");
        assert!(archive.windows(4).any(|w| w == 0x0606_4b50u32.to_le_bytes()), "the ZIP64 end record is there");
    }

    #[test]
    fn many_entries_need_the_zip64_end_record() {
        let mut z = ZipWriter::new();
        let mut out = Vec::new();
        for i in 0..70_000u32 {
            out.extend(z.start_entry(&format!("{i}"), 0));
            out.extend(z.finish_entry(0, 0));
        }
        out.extend(z.finish());
        let listed = list(&out).unwrap();
        assert_eq!(listed.len(), 70_000);
        assert_eq!(listed[69_999].name, "69999");
    }
}
