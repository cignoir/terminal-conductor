//! Picks OSC 7501 ("program status") sequences out of a PTY byte stream.
//!
//! AI CLIs such as Codex CLI and Antigravity CLI report their state with
//! `ESC ] 7501 ; <payload> (BEL | ESC \)`. Sequences can be split across
//! reads, so the scanner keeps a small tail between calls.

const START: &[u8] = b"\x1b]7501;";
/// An unterminated sequence longer than this is dropped.
const MAX_PENDING: usize = 8 * 1024;

#[derive(Default)]
pub struct OscScanner {
    pending: Vec<u8>,
}

impl OscScanner {
    /// Feeds the next chunk; returns the payloads of completed sequences.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(chunk);
        let mut out = Vec::new();
        let mut i = 0;
        loop {
            let Some(rel) = find(&data[i..], START) else {
                // Keep a possible partial start sequence at the end.
                let keep = START.len() - 1;
                let from = data.len().saturating_sub(keep).max(i);
                if data[from..].contains(&0x1b) {
                    let esc = from + data[from..].iter().position(|b| *b == 0x1b).unwrap();
                    self.pending = data[esc..].to_vec();
                }
                break;
            };
            let start = i + rel + START.len();
            match terminator(&data[start..]) {
                Some((end, len)) => {
                    out.push(String::from_utf8_lossy(&data[start..start + end]).to_string());
                    i = start + end + len;
                }
                None => {
                    if data.len() - (start - START.len()) <= MAX_PENDING {
                        self.pending = data[start - START.len()..].to_vec();
                    }
                    break;
                }
            }
        }
        out
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Position and length of the BEL or ST that ends a sequence.
fn terminator(data: &[u8]) -> Option<(usize, usize)> {
    for (i, b) in data.iter().enumerate() {
        if *b == 0x07 {
            return Some((i, 1));
        }
        if *b == 0x1b && data.get(i + 1) == Some(&b'\\') {
            return Some((i, 2));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_sequences_with_both_terminators() {
        let mut s = OscScanner::default();
        let out = s.feed(b"hi\x1b]7501;state=working:app=codex\x07mid\x1b]7501;state=idle:app=codex\x1b\\end");
        assert_eq!(out, vec!["state=working:app=codex", "state=idle:app=codex"]);
    }

    #[test]
    fn ignores_other_osc() {
        let mut s = OscScanner::default();
        assert!(s.feed(b"\x1b]0;title\x07\x1b]9;note\x07").is_empty());
    }

    #[test]
    fn joins_sequences_split_across_reads() {
        let mut s = OscScanner::default();
        assert!(s.feed(b"text\x1b]75").is_empty());
        assert!(s.feed(b"01;state=do").is_empty());
        assert_eq!(s.feed(b"ne:app=antigravity\x07more"), vec!["state=done:app=antigravity"]);
        // Nothing left over afterwards.
        assert!(s.feed(b"plain").is_empty());
        assert!(s.pending.is_empty());
    }

    #[test]
    fn split_inside_the_terminator() {
        let mut s = OscScanner::default();
        assert!(s.feed(b"\x1b]7501;state=idle\x1b").is_empty());
        assert_eq!(s.feed(b"\\"), vec!["state=idle"]);
    }

    #[test]
    fn drops_runaway_sequences() {
        let mut s = OscScanner::default();
        s.feed(b"\x1b]7501;");
        s.feed(&vec![b'x'; MAX_PENDING * 2]);
        assert!(s.pending.len() <= MAX_PENDING + START.len());
    }
}
