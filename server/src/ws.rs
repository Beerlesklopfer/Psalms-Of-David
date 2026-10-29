// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! The part of the WebSocket protocol (RFC 6455) that the event channel of the apps needs: the opening handshake,
//! frames in both directions, ping, pong and close. It is written here because the `ws` feature of axum would add
//! a library that is not on this machine; the upgrade itself is done by hyper, which axum already uses.
//! Messages are text; a message in pieces (fragments) is not supported by the apps and its pieces are ignored.

use axum::http::{HeaderMap, header};
use sha1::{Digest, Sha1};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
/// The largest frame the server reads from a client. The apps send nothing but pings and pongs.
pub const MAX_FRAME: u64 = 64 * 1024;

pub const CLOSE_NORMAL: u16 = 1000;
pub const CLOSE_GOING_AWAY: u16 = 1001;
pub const CLOSE_PROTOCOL: u16 = 1002;
pub const CLOSE_TOO_BIG: u16 = 1009;
/// The session behind the token ended (the device was revoked or signed out).
pub const CLOSE_REVOKED: u16 = 4401;
/// The client did not answer the heartbeat.
pub const CLOSE_TIMEOUT: u16 = 4408;

#[derive(Debug, PartialEq, Eq)]
pub enum Frame {
    Text(String),
    /// Binary data or a fragment: read and ignored.
    Other,
    Ping(Vec<u8>),
    Pong,
    Close(Option<u16>),
}

/// `Sec-WebSocket-Accept` for a key, or `None` if the request is no valid upgrade to version 13.
pub fn accept_value(headers: &HeaderMap) -> Option<String> {
    let has = |name, word: &str| {
        headers
            .get_all(name)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .any(|v| v.split(',').any(|p| p.trim().eq_ignore_ascii_case(word)))
    };
    if !has(header::CONNECTION, "upgrade") || !has(header::UPGRADE, "websocket") {
        return None;
    }
    if headers.get("sec-websocket-version").and_then(|v| v.to_str().ok()).map(str::trim) != Some("13") {
        return None;
    }
    let key = headers.get("sec-websocket-key")?.to_str().ok()?.trim();
    if key.is_empty() || key.len() > 64 {
        return None;
    }
    let digest = Sha1::digest(format!("{key}{GUID}").as_bytes());
    Some(base64(&digest))
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Reads one frame. `Ok(None)` is the end of the connection. A client must mask its frames; one that does not is a
/// protocol error, as is a control frame that is too long.
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Option<Frame>> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_owned());
    let mut head = [0u8; 2];
    match r.read_exact(&mut head).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let (fin, opcode, masked) = (head[0] & 0x80 != 0, head[0] & 0x0f, head[1] & 0x80 != 0);
    if head[0] & 0x70 != 0 || !masked {
        return Err(bad("reserved bits or unmasked frame"));
    }
    let len = match head[1] & 0x7f {
        126 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b).await?;
            u64::from(u16::from_be_bytes(b))
        }
        127 => {
            let mut b = [0u8; 8];
            r.read_exact(&mut b).await?;
            u64::from_be_bytes(b)
        }
        n => u64::from(n),
    };
    if len > MAX_FRAME {
        return Err(std::io::Error::new(std::io::ErrorKind::FileTooLarge, "frame too large"));
    }
    if opcode >= 8 && (len > 125 || !fin) {
        return Err(bad("bad control frame"));
    }
    let mut mask = [0u8; 4];
    r.read_exact(&mut mask).await?;
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload).await?;
    for (i, b) in payload.iter_mut().enumerate() {
        *b ^= mask[i % 4];
    }
    Ok(Some(match opcode {
        1 if fin => Frame::Text(String::from_utf8(payload).map_err(|_| bad("text is not UTF-8"))?),
        8 => Frame::Close(payload.get(..2).map(|c| u16::from_be_bytes([c[0], c[1]]))),
        9 => Frame::Ping(payload),
        10 => Frame::Pong,
        0..=2 => Frame::Other,
        _ => return Err(bad("unknown opcode")),
    }))
}

/// Writes one unmasked frame (a server never masks).
pub async fn write_frame<W: AsyncWrite + Unpin>(w: &mut W, opcode: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut out = vec![0x80 | opcode];
    match payload.len() {
        n if n < 126 => out.push(n as u8),
        n if n <= u16::MAX as usize => {
            out.push(126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    w.write_all(&out).await?;
    w.flush().await
}

pub async fn write_text<W: AsyncWrite + Unpin>(w: &mut W, text: &str) -> std::io::Result<()> {
    write_frame(w, 1, text.as_bytes()).await
}

pub async fn write_close<W: AsyncWrite + Unpin>(w: &mut W, code: u16, reason: &str) -> std::io::Result<()> {
    let mut payload = code.to_be_bytes().to_vec();
    // A close reason must fit the 125 bytes of a control frame together with the code.
    payload.extend(reason.bytes().take(120));
    write_frame(w, 8, &payload).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accept_value_of_the_rfc_example() {
        let mut h = HeaderMap::new();
        h.insert(header::CONNECTION, "keep-alive, Upgrade".parse().unwrap());
        h.insert(header::UPGRADE, "websocket".parse().unwrap());
        h.insert("sec-websocket-version", "13".parse().unwrap());
        h.insert("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==".parse().unwrap());
        assert_eq!(accept_value(&h).as_deref(), Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
        h.insert("sec-websocket-version", "8".parse().unwrap());
        assert_eq!(accept_value(&h), None);
    }

    #[tokio::test]
    async fn frames_round_trip_and_unmasked_client_frames_are_refused() {
        let mut out: Vec<u8> = Vec::new();
        write_text(&mut out, "hi").await.unwrap();
        assert_eq!(out, [0x81, 2, b'h', b'i']);
        let big = vec![b'x'; 300];
        let mut out: Vec<u8> = Vec::new();
        write_frame(&mut out, 1, &big).await.unwrap();
        assert_eq!(&out[..4], &[0x81, 126, 1, 44]);

        // A masked ping from a client.
        let mask = [1u8, 2, 3, 4];
        let mut msg = vec![0x89, 0x80 | 3];
        msg.extend_from_slice(&mask);
        msg.extend(b"abc".iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
        assert_eq!(read_frame(&mut &msg[..]).await.unwrap(), Some(Frame::Ping(b"abc".to_vec())));
        let unmasked = [0x81u8, 1, b'a'];
        assert!(read_frame(&mut &unmasked[..]).await.is_err());
        assert_eq!(read_frame(&mut &b""[..]).await.unwrap(), None);
    }
}
