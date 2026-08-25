//! PTY wire-protocol helpers.
//!
//! Mirrors the anureo `pty-protocol.ts` contract exactly — the pure,
//! transport-free helpers used to adapt `Pty.attach` to WebSocket
//! transports. No axum/tokio; no I/O. Only logic + UTF-8.
//!
//! ## Wire format
//!
//! Outbound frames are raw UTF-8 terminal chunks. One control frame — a
//! [`CONTROL_MARKER`] byte followed by UTF-8 JSON `{"cursor":N}` — carries
//! the absolute output cursor after a replay so clients can resume later.
//! Inbound client frames are UTF-8 text (or binary that must be UTF-8);
//! invalid UTF-8 input is dropped (see [`decode_input`]).
//!
//! `pty-protocol.ts` reference:
//! ```text
//! export const REPLAY_CHUNK = 64 * 1024
//! export function metaFrame(cursor) { [0x00, ...JSON({"cursor":N})] }
//! export function chunks(data)      { slice every REPLAY_CHUNK }
//! export function decodeInput(msg)  { fatal UTF-8 decode; undefined on err }
//! ```

/// Maximum byte length of a single replay frame. Replays can be megabytes,
/// so the buffered output is sent in bounded chunks of this size. Mirrors
/// `REPLAY_CHUNK` in `pty-protocol.ts`.
pub const REPLAY_CHUNK: usize = 64 * 1024;

/// Leading byte of a control frame. Any frame whose first byte equals this
/// is a metadata frame built by [`meta_frame`]; all other frames are raw
/// terminal output. Mirrors the `0x00` sentinel in `pty-protocol.ts`.
pub const CONTROL_MARKER: u8 = 0x00;

/// Build a control frame carrying the absolute output `cursor`.
///
/// Wire layout: `[CONTROL_MARKER, ...UTF-8 bytes of compact JSON
/// {"cursor":N}]` — identical byte-for-byte to JS
/// `metaFrame(cursor)` = `[0x00, ...TextEncoder.encode(JSON.stringify({cursor}))]`.
/// The JSON is emitted compact (no whitespace) to match `JSON.stringify`.
pub fn meta_frame(cursor: u64) -> Vec<u8> {
    // Compact JSON, exactly what `JSON.stringify({ cursor })` produces.
    let json = format!("{{\"cursor\":{cursor}}}");
    let mut out = Vec::with_capacity(json.len() + 1);
    out.push(CONTROL_MARKER);
    out.extend_from_slice(json.as_bytes());
    out
}

/// Split `data` into chunks of at most [`REPLAY_CHUNK`] bytes, never
/// breaking a UTF-8 character across a frame boundary. Mirrors
/// `chunks(data)` in `pty-protocol.ts`; the Rust port chunks on byte length
/// snapped to character boundaries (UTF-8 safe).
///
/// Each returned slice is a `&str` borrowing from `data`, so every chunk is
/// guaranteed valid UTF-8. Empty input yields an empty vector (matches the
/// TS `chunks("")`).
pub fn chunk_replay(data: &str) -> Vec<&str> {
    let bytes = data.as_bytes();
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        // Tentative end: REPLAY_CHUNK bytes forward, clamped to the buffer.
        let mut end = (start + REPLAY_CHUNK).min(bytes.len());
        // Snap back to the nearest UTF-8 char boundary so we never split a
        // multibyte sequence across frames.
        while end < bytes.len() && !data.is_char_boundary(end) {
            end -= 1;
        }
        // A single character is at most 4 bytes, so it can never exceed
        // REPLAY_CHUNK (64 KiB) — `end > start` always holds here. Guard
        // defensively regardless.
        if end <= start {
            end = next_char_boundary(data, start);
        }
        out.push(&data[start..end]);
        start = end;
    }
    out
}

/// Byte index immediately after the character starting at `idx`.
fn next_char_boundary(data: &str, idx: usize) -> usize {
    match data[idx..].char_indices().nth(1) {
        Some((off, _)) => idx + off,
        None => data.len(),
    }
}

/// Decode an inbound client frame to a UTF-8 string.
///
/// Mirrors `decodeInput(message)` in `pty-protocol.ts` with the binary path:
/// the message is UTF-8-decoded in "fatal" mode; if it is not valid UTF-8
/// the frame is dropped (`None`). A valid (including empty) frame decodes to
/// `Some(String)`.
pub fn decode_input(msg: &[u8]) -> Option<String> {
    std::str::from_utf8(msg).ok().map(|s| s.to_string())
}

/// Parse the compact `{"cursor":N}` JSON emitted by [`meta_frame`] into the
/// cursor value, tolerating optional whitespace around the colon. Returns
/// `None` if the shape does not match.
fn parse_cursor_json(s: &str) -> Option<u64> {
    const KEY: &str = "\"cursor\"";
    let key_idx = s.find(KEY)?;
    let after_key = &s[key_idx + KEY.len()..];
    let colon_idx = after_key.find(':')?;
    let rest = after_key[colon_idx + 1..].trim_start();
    let digits: &str = &rest[..rest
        .char_indices()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0)];
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u64>().ok()
    }
}

/// A decoded wire frame on the PTY WebSocket transport.
///
/// `Output` and `Meta` frames are parsed from raw bytes via [`Frame::parse`];
/// `Close` carries the child exit code and is constructed by the server when
/// the PTY child terminates (the anureo contract defines no on-wire close
/// encoding, only the `0x00` metadata marker).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// Raw UTF-8 terminal output — any frame that does not begin with
    /// [`CONTROL_MARKER`].
    Output(String),
    /// Metadata control frame carrying the absolute output cursor
    /// (produced by [`meta_frame`]).
    Meta(u64),
    /// Close frame carrying the child exit code.
    Close(u16),
}

impl Frame {
    /// Parse a raw frame into [`Frame::Output`] or [`Frame::Meta`].
    ///
    /// A leading [`CONTROL_MARKER`] denotes a metadata frame whose body is
    /// the cursor JSON; anything else is terminal output. Frames that are
    /// not valid UTF-8 (or whose control JSON is malformed) yield `None`,
    /// matching the "drop invalid input" rule of [`decode_input`].
    pub fn parse(raw: &[u8]) -> Option<Frame> {
        let first = *raw.first()?;
        if first == CONTROL_MARKER {
            let body = std::str::from_utf8(&raw[1..]).ok()?;
            Some(Frame::Meta(parse_cursor_json(body)?))
        } else {
            let s = std::str::from_utf8(raw).ok()?;
            Some(Frame::Output(s.to_string()))
        }
    }
}

/// Accumulates terminal output and supports resumable replay by absolute
/// byte cursor.
///
/// Backs the WebSocket connect flow: a freshly connected client first
/// receives `replay_from(0)` (split into bounded pieces via
/// [`chunk_replay`]), then a [`meta_frame`] carrying
/// [`ReplayBuffer::current_cursor`], then continues streaming live output.
/// The client remembers the cursor and passes it back on reconnect so a
/// subsequent `replay_from(cursor)` returns only the tail it missed.
#[derive(Debug, Default, Clone)]
pub struct ReplayBuffer {
    output: String,
}

impl ReplayBuffer {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self {
            output: String::new(),
        }
    }

    /// Append a chunk of terminal output. `data` must be valid UTF-8
    /// (enforced by `&str`).
    pub fn append(&mut self, data: &str) {
        self.output.push_str(data);
    }

    /// Current absolute cursor: the total number of output bytes seen so
    /// far. This is the value carried by [`meta_frame`] after a replay.
    pub fn current_cursor(&self) -> u64 {
        self.output.len() as u64
    }

    /// Return the buffered tail starting at byte offset `cursor`.
    ///
    /// A `cursor` obtained from a previous [`ReplayBuffer::current_cursor`]
    /// is guaranteed to sit on a UTF-8 character boundary, so slicing is
    /// always valid; an out-of-range cursor is clamped to the buffer end
    /// (returns an empty tail). Arbitrary mid-character cursors are snapped
    /// forward to the next boundary defensively.
    pub fn replay_from(&self, cursor: u64) -> &str {
        let len = self.output.len();
        let mut start = usize::try_from(cursor).unwrap_or(usize::MAX).min(len);
        while start < len && !self.output.is_char_boundary(start) {
            start += 1;
        }
        &self.output[start..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `meta_frame` must be `[0x00, ...compact JSON {"cursor":N}]`.
    #[test]
    fn meta_frame_bytes_exact() {
        // cursor = 5 -> {"cursor":5}
        let frame = meta_frame(5);
        assert_eq!(frame[0], CONTROL_MARKER);
        let json = std::str::from_utf8(&frame[1..]).unwrap();
        assert_eq!(json, "{\"cursor\":5}");
        // Full byte layout: 0x00, '{', '"', c,u,r,s,o,r, '"', ':', '5', '}'
        assert_eq!(
            frame,
            vec![0x00, 0x7B, 0x22, b'c', b'u', b'r', b's', b'o', b'r', 0x22, 0x3A, b'5', 0x7D]
        );

        // cursor = 0 edge case.
        let zero = meta_frame(0);
        assert_eq!(std::str::from_utf8(&zero[1..]).unwrap(), "{\"cursor\":0}");
    }

    /// `chunk_replay` must never split a multibyte UTF-8 character, even when
    /// the [`REPLAY_CHUNK`] boundary lands in the middle of one.
    #[test]
    fn chunk_boundary_on_multibyte_char() {
        // REPLAY_CHUNK-1 ASCII bytes, then '€' (3 UTF-8 bytes) so the
        // naive 64KiB cut falls inside '€' (bytes 65535..65538).
        let mut s = String::with_capacity(REPLAY_CHUNK + 3);
        for _ in 0..(REPLAY_CHUNK - 1) {
            s.push('a');
        }
        s.push('€'); // 0xE2 0x82 0xAC
        assert_eq!(s.len(), REPLAY_CHUNK + 2);

        let chunks = chunk_replay(&s);
        assert_eq!(chunks.len(), 2, "expected two chunks");
        // First chunk snapped back to the char boundary before '€'.
        assert_eq!(chunks[0].len(), REPLAY_CHUNK - 1);
        assert!(chunks[0].chars().all(|c| c == 'a'));
        // Second chunk is the intact multibyte char.
        assert_eq!(chunks[1], "€");
        // Round-trips to the original.
        assert_eq!(chunks.concat(), s);
        // Every chunk is within the bound.
        for c in &chunks {
            assert!(c.len() <= REPLAY_CHUNK);
        }
    }

    /// A single chunk that is exactly [`REPLAY_CHUNK`] of ASCII stays whole.
    #[test]
    fn chunk_exact_boundary_ascii() {
        let s: String = "a".repeat(REPLAY_CHUNK);
        let chunks = chunk_replay(&s);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), REPLAY_CHUNK);
    }

    /// Empty input yields zero chunks (matches TS `chunks("")`).
    #[test]
    fn chunk_empty() {
        assert!(chunk_replay("").is_empty());
    }

    /// `decode_input` must drop frames that are not valid UTF-8.
    #[test]
    fn decode_input_drops_invalid_utf8() {
        // Valid text round-trips.
        assert_eq!(decode_input(b"hello"), Some("hello".to_string()));
        // Multibyte UTF-8 is valid.
        assert_eq!(
            decode_input("héllowörld".as_bytes()),
            Some("héllowörld".to_string())
        );
        // Empty frame is valid UTF-8 -> Some("").
        assert_eq!(decode_input(b""), Some(String::new()));
        // Lone continuation / invalid lead byte -> None.
        assert_eq!(decode_input(&[0xFF]), None);
        // Truncated multibyte sequence -> None.
        assert_eq!(decode_input(&[0xE2, 0x82]), None);
    }

    /// `ReplayBuffer` resume: a snapshot cursor returns exactly the tail
    /// appended since that snapshot.
    #[test]
    fn replay_cursor_resume_returns_tail() {
        let mut buf = ReplayBuffer::new();
        buf.append("hello ");
        let mid = buf.current_cursor();
        assert_eq!(mid, 6);
        buf.append("world");
        // Cursor snapshot taken at byte 6 yields everything after it.
        assert_eq!(buf.replay_from(mid), "world");
        assert_eq!(buf.current_cursor(), 11);

        // Full replay from zero.
        assert_eq!(buf.replay_from(0), "hello world");
        // Re-snapshotting and appending resumes correctly.
        let snap = buf.current_cursor();
        buf.append("!!!");
        assert_eq!(buf.replay_from(snap), "!!!");
        assert_eq!(buf.replay_from(0), "hello world!!!");

        // Out-of-range cursor clamps to the end -> empty tail.
        assert_eq!(buf.replay_from(9_999), "");
    }

    /// `Frame::parse` round-trips a `meta_frame` and treats raw bytes as
    /// output.
    #[test]
    fn frame_parse_meta_and_output() {
        let raw = meta_frame(42);
        assert_eq!(Frame::parse(&raw), Some(Frame::Meta(42)));

        let out = b"some terminal output";
        assert_eq!(
            Frame::parse(out),
            Some(Frame::Output("some terminal output".to_string()))
        );

        // Invalid UTF-8 output frame is dropped.
        assert_eq!(Frame::parse(&[0xFF]), None);
    }
}
