/// Split text into ~`target_tokens` chunks with `overlap_tokens` overlap.
/// Token approximation: 1 token ≈ 4 chars (English heuristic). We chunk on
/// paragraph/sentence boundaries when possible.
pub fn chunk_text(text: &str, target_tokens: usize, overlap_tokens: usize) -> Vec<String> {
    let target_chars = target_tokens * 4;
    let overlap_chars = overlap_tokens * 4;
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() { return vec![]; }

    let bytes: Vec<char> = normalized.chars().collect();
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        let end = (start + target_chars).min(bytes.len());
        // Try to back up to a sentence boundary if not at the end of the text
        let mut split = end;
        if end < bytes.len() {
            let window_start = start + (target_chars * 2 / 3);
            for i in (window_start..end).rev() {
                if matches!(bytes[i], '.' | '!' | '?' | '\n') {
                    split = i + 1;
                    break;
                }
            }
        }
        let chunk: String = bytes[start..split].iter().collect();
        chunks.push(chunk.trim().to_string());
        if split >= bytes.len() { break; }
        start = split.saturating_sub(overlap_chars);
    }
    chunks.into_iter().filter(|c| !c.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_returns_empty() {
        assert!(chunk_text("", 100, 10).is_empty());
        assert!(chunk_text("   \n\n  ", 100, 10).is_empty());
    }

    #[test]
    fn short_text_is_one_chunk() {
        let c = chunk_text("Hello world. This is short.", 500, 50);
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn long_text_splits_with_overlap() {
        let para = "Lorem ipsum dolor sit amet. ".repeat(200); // ~5400 chars
        let c = chunk_text(&para, 500, 50); // target ~2000 chars per chunk
        assert!(c.len() >= 2);
        // Ensure each chunk is below a generous bound
        for chunk in &c {
            assert!(chunk.len() <= 500 * 4 + 100, "chunk too big: {}", chunk.len());
        }
    }

    #[test]
    fn prefers_sentence_boundary() {
        let s = "First sentence here. Second one is here. Third lands here.";
        let c = chunk_text(s, 5, 0); // tiny target forces splits
        // Each chunk should not start mid-sentence ideally; just check we split
        assert!(c.len() >= 2);
    }
}
