// SPDX-License-Identifier: GPL-3.0-or-later

//! Message chunking + HTML escaping for Telegram (v0.2, ADR-013).
//!
//! Telegram counts message length in **characters** (not bytes) and caps
//! a message at 4096 characters. `split_message` cuts long text at line
//! or word boundaries, keeps code fences intact (a chunk that ends inside
//! a fence is closed synthetically and the next one re-opens it), and
//! tags each chunk `(i/n)` per DESIGN §3. All splitting is done on
//! `char` boundaries — never mid-code-point. Cut points are only ever
//! chosen at line/space boundaries or at exact character counts, so a
//! multi-`char` grapheme (emoji + variation selector) is not split apart
//! unless the text is one unbroken run longer than the limit, in which
//! case the cut still falls on a `char` boundary.

/// Telegram's per-message character limit.
pub const MAX_MESSAGE_CHARS: usize = 4096;

/// Reserve kept free in every chunk of a multi-chunk message for the
/// `(i/n)` continuation marker (up to 3-digit counts: `"\n(999/999)"` is
/// 10 characters) AND, when a chunk closes a code fence, the synthetic
/// ```` ``` ```` (4 characters: newline + 3 backticks). A chunk cut at
/// the budget can carry both at once: 10 + 4 = 14. (A reserve of 10 was
/// proven insufficient by the build checker: a ~440 KB fenced message
/// produced chunks up to 4100 characters, which the real Bot API
/// rejects with 400.)
const CHUNK_BUDGET_RESERVE: usize = 14;

/// Escape text for Telegram `parse_mode: HTML` (`&`, `<`, `>`).
/// Model output is sent as plain text (no parse_mode) in v0.2; this is
/// for the short strings the gateway constructs itself (card titles,
/// tool lines) when it does use HTML.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Split `text` into chunks of at most [`MAX_MESSAGE_CHARS`] characters.
/// Chunks are cut at a line boundary when possible, otherwise at a word
/// boundary, otherwise at the exact character limit. A chunk that ends
/// inside a ``` code fence is closed with a synthetic ``` and the next
/// chunk re-opens it, so no chunk ever contains an unbalanced fence.
/// Returns at least one chunk, even for empty text.
pub fn split_message(text: &str) -> Vec<String> {
    let chunks = split_raw(text);
    let n = chunks.len();
    if n > 1 {
        return tag_markers(chunks, 1, n);
    }
    chunks
}

/// Like [`split_message`], but the chunks are numbered as part of a
/// larger message: the first chunk here is part `start` of `total`
/// (`start` is 1-based). Used by the streaming renderer, which may
/// already have sent earlier parts of the same answer before the total
/// was knowable. Markers are appended whenever `total > 1`, even when
/// this slice is a single chunk (it is still part of a multi-part
/// message).
pub fn split_message_numbered(text: &str, start: usize, total: usize) -> Vec<String> {
    let chunks = split_raw(text);
    if total > 1 {
        return tag_markers(chunks, start, total);
    }
    chunks
}

/// The chunking itself, without `(i/n)` markers.
fn split_raw(text: &str) -> Vec<String> {
    let total_chars = text.chars().count();
    if total_chars <= MAX_MESSAGE_CHARS {
        return vec![text.to_string()];
    }
    // Multi-chunk message: every chunk is cut at the budget below the
    // limit, so the (i/n) marker (+ fence closing) always fits.
    let budget = MAX_MESSAGE_CHARS - CHUNK_BUDGET_RESERVE;
    let mut chunks: Vec<String> = Vec::new();
    let mut rest = text;
    let mut in_fence = false;
    loop {
        let rest_chars = rest.chars().count();
        if rest_chars <= budget {
            let mut chunk = String::new();
            if in_fence {
                chunk.push_str("```\n");
            }
            chunk.push_str(rest);
            chunks.push(chunk);
            break;
        }
        // Find a cut point at or before the budget.
        let cut = best_cut(rest, budget);
        let (head, tail) = split_at_char(rest, cut);
        let mut chunk = head.to_string();
        let head_in_fence = fence_state_after(head, in_fence);
        if head_in_fence {
            // The cut lands inside a fence: close it synthetically.
            if !chunk.ends_with('\n') {
                chunk.push('\n');
            }
            chunk.push_str("```");
            in_fence = true;
        } else {
            in_fence = false;
        }
        chunks.push(chunk);
        rest = tail;
    }
    chunks
}

/// Append `(i/n)` continuation markers to already-split chunks.
/// The chunk budget reserve guarantees they fit.
fn tag_markers(mut chunks: Vec<String>, start: usize, total: usize) -> Vec<String> {
    for (i, chunk) in chunks.iter_mut().enumerate() {
        chunk.push_str(&format!("\n({}/{})", start + i, total));
    }
    chunks
}

/// Number of chunks [`split_message`] would produce for `text`.
pub fn split_count(text: &str) -> usize {
    split_raw(text).len()
}

/// Character length of the first chunk's content — used by the
/// streaming renderer to slice a growing draft. This is the cut point
/// itself (`best_cut`), before the `(i/n)` marker and any synthetic
/// fence closing are appended, so slicing the draft at this length
/// loses no characters. The cut is capped at the chunk budget
/// (`MAX_MESSAGE_CHARS - CHUNK_BUDGET_RESERVE`), not the raw limit:
/// a finalized head gains an `(i/n)` marker at flush time, and the
/// reserve guarantees the marker still fits under 4096 characters.
pub fn first_chunk_content_len(text: &str) -> usize {
    let total = text.chars().count();
    if total <= MAX_MESSAGE_CHARS {
        total
    } else {
        best_cut(text, MAX_MESSAGE_CHARS - CHUNK_BUDGET_RESERVE)
    }
}
/// Choose a cut index (in characters) at or before `limit`: prefer the
/// last newline, then the last space, otherwise the limit itself.
fn best_cut(text: &str, limit: usize) -> usize {
    let mut last_newline: Option<usize> = None;
    let mut last_space: Option<usize> = None;
    for (i, ch) in text.chars().enumerate() {
        if i >= limit {
            break;
        }
        match ch {
            '\n' => last_newline = Some(i + 1),
            ' ' | '\t' => last_space = Some(i + 1),
            _ => {}
        }
    }
    if let Some(pos) = last_newline {
        if pos > 0 {
            return pos;
        }
    }
    if let Some(pos) = last_space {
        if pos > 0 {
            return pos;
        }
    }
    limit
}

/// Split a string at a character index (never mid-`char`).
fn split_at_char(text: &str, char_idx: usize) -> (&str, &str) {
    let byte_idx = text
        .char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len());
    text.split_at(byte_idx)
}

/// Whether a code fence is open after consuming `text`, given the fence
/// state before it. A fence toggles on a line that starts with ``` (the
/// common case; inline ``` on a non-fence line is rare enough that the
/// approximation is safe for chunking).
fn fence_state_after(text: &str, mut in_fence: bool) -> bool {
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
    }
    in_fence
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_message_is_one_chunk() {
        let chunks = split_message("hello");
        assert_eq!(chunks, vec!["hello".to_string()]);
    }

    #[test]
    fn test_empty_message_is_one_empty_chunk() {
        let chunks = split_message("");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "");
    }

    #[test]
    fn test_exactly_4096_chars_is_one_chunk() {
        let text = "x".repeat(MAX_MESSAGE_CHARS);
        let chunks = split_message(&text);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chars().count(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn test_4097_chars_becomes_two_chunks_with_markers() {
        let text = "y".repeat(MAX_MESSAGE_CHARS + 1);
        let chunks = split_message(&text);
        assert_eq!(chunks.len(), 2);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= MAX_MESSAGE_CHARS);
        }
        assert!(chunks[0].ends_with("(1/2)"));
        assert!(chunks[1].ends_with("(2/2)"));
    }

    #[test]
    fn test_long_message_never_exceeds_limit_and_keeps_all_chars() {
        let text = "word ".repeat(3000); // 15000 chars, spaces everywhere
        let chunks = split_message(&text);
        assert!(chunks.len() >= 4);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= MAX_MESSAGE_CHARS);
        }
        // Joining the chunks (minus their (i/n) markers) reproduces the input.
        let joined: String = chunks
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let marker = format!("\n({}/{})", i + 1, chunks.len());
                c.strip_suffix(&marker).unwrap_or(c.as_str()).to_string()
            })
            .collect();
        assert_eq!(joined, text);
    }

    #[test]
    fn test_fenced_440kb_message_never_exceeds_limit() {
        // Regression test for the build checker's reserve finding: a
        // long fenced message (100+ chunks, so 3-digit `(i/n)` markers)
        // must keep every chunk — marker + synthetic fence close
        // included — at or under 4096 characters.
        let mut text = String::from("intro line\n```rust\n");
        text.push_str(&"let x = 1;\n".repeat(40_000)); // ~440 KB inside the fence
        text.push_str("```\nlast line\n");
        let chunks = split_message(&text);
        assert!(
            chunks.len() >= 100,
            "need 3-digit markers, got {}",
            chunks.len()
        );
        for chunk in &chunks {
            assert!(
                chunk.chars().count() <= MAX_MESSAGE_CHARS,
                "chunk of {} chars exceeds the limit",
                chunk.chars().count()
            );
        }
        assert!(chunks
            .last()
            .unwrap()
            .ends_with(&format!("({}/{})", chunks.len(), chunks.len())));
    }

    #[test]
    fn test_numbered_split_continues_an_earlier_part() {
        // A remainder that is itself one chunk still carries its marker
        // when it belongs to a 3-part message.
        let tail = "z".repeat(1000);
        let chunks = split_message_numbered(&tail, 3, 3);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].ends_with("(3/3)"));
        // A longer remainder is numbered from the given start.
        let long = "word ".repeat(3000);
        let chunks = split_message_numbered(&long, 2, 5);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].ends_with("(2/5)"));
        assert!(chunks[1].ends_with("(3/5)"));
        for chunk in &chunks {
            assert!(chunk.chars().count() <= MAX_MESSAGE_CHARS);
        }
        // Single-part message: no marker even when numbered.
        assert_eq!(split_message_numbered("hi", 1, 1), vec!["hi".to_string()]);
        assert_eq!(split_count(&long), split_message(&long).len());
    }

    #[test]
    fn test_fence_split_is_balanced_in_every_chunk() {
        // A fence that opens before the cut and closes after it: every
        // chunk must contain balanced fences.
        let mut text = String::from("intro line\n```rust\n");
        text.push_str(&"let x = 1;\n".repeat(600)); // ~6600 chars inside the fence
        text.push_str("```\nlast line\n");
        let chunks = split_message(&text);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            let opens = chunk.matches("```").count();
            assert_eq!(opens % 2, 0, "unbalanced fence in chunk: {chunk:?}");
            assert!(chunk.chars().count() <= MAX_MESSAGE_CHARS);
        }
    }

    #[test]
    fn test_multibyte_chars_are_never_split() {
        // Emoji are 1 char each here; a cut must never land mid-code-point.
        let text = "🐾".repeat(5000);
        let chunks = split_message(&text);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.chars().count() <= MAX_MESSAGE_CHARS);
            assert!(chunk.is_char_boundary(0));
        }
        assert!(chunks.concat().contains("🐾"));
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(
            escape_html("a <b> & \"c\" > d"),
            "a &lt;b&gt; &amp; \"c\" &gt; d"
        );
    }
}
