//! Text ↔ token ids of an Opus-MT model (AP-21b).
//!
//! Marian models cut the source text with a SentencePiece unigram model and
//! look the pieces up in their own vocabulary. The export script writes both
//! as plain tables (`source.tsv`, `vocab.txt`), so neither protobuf nor the
//! C++ library is needed: segmentation is the unigram Viterbi search of
//! SentencePiece over NFKC-normalized text.

use std::collections::HashMap;

/// Marks the start of a word in SentencePiece pieces.
const WORD_START: char = '▁';

/// SentencePiece scores an unknown character this far below the lowest
/// piece (`kUnkPenalty`).
const UNKNOWN_PENALTY: f32 = 10.0;

/// Piece types of `source.tsv`, as SentencePiece numbers them.
const NORMAL: u8 = 1;
const USER_DEFINED: u8 = 4;

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum TokenizerError {
    #[error("line {0} of the piece table is malformed")]
    BadPiece(usize),
}

/// Source side: text → model ids.
#[derive(Debug)]
pub struct SourceTokenizer {
    /// Score of every piece that may appear in a segmentation.
    pieces: HashMap<String, f32>,
    /// Longest piece, in characters.
    max_chars: usize,
    unknown_score: f32,
}

impl SourceTokenizer {
    /// Reads `source.tsv`: `piece <TAB> score <TAB> type` per line.
    pub fn from_table(table: &str) -> Result<Self, TokenizerError> {
        let mut pieces = HashMap::new();
        let mut min_score = 0.0_f32;
        for (index, line) in table.lines().enumerate() {
            let mut fields = line.split('\t');
            let (Some(piece), Some(score), Some(kind)) =
                (fields.next(), fields.next(), fields.next())
            else {
                return Err(TokenizerError::BadPiece(index + 1));
            };
            let score: f32 = score
                .parse()
                .map_err(|_| TokenizerError::BadPiece(index + 1))?;
            let kind: u8 = kind
                .parse()
                .map_err(|_| TokenizerError::BadPiece(index + 1))?;
            if kind == NORMAL || kind == USER_DEFINED {
                min_score = min_score.min(score);
                pieces.insert(piece.to_string(), score);
            }
        }
        let max_chars = pieces.keys().map(|p| p.chars().count()).max().unwrap_or(1);
        Ok(Self {
            pieces,
            max_chars,
            unknown_score: min_score - UNKNOWN_PENALTY,
        })
    }

    /// The pieces of `text`; characters no piece covers come out as they
    /// are, runs of them merged, the way SentencePiece reports unknowns.
    pub fn pieces(&self, text: &str) -> Vec<String> {
        let normalized = normalize(text);
        if normalized.is_empty() {
            return Vec::new();
        }
        let chars: Vec<char> = normalized.chars().collect();
        // best[i]: score of the best segmentation of chars[..i], with the
        // start of its last piece and whether that piece is unknown.
        let mut best: Vec<Option<(f32, usize, bool)>> = vec![None; chars.len() + 1];
        best[0] = Some((0.0, 0, false));
        let mut candidate = String::new();
        for start in 0..chars.len() {
            let Some((base, _, _)) = best[start] else {
                continue;
            };
            let mut covered = false;
            candidate.clear();
            for end in start + 1..=(start + self.max_chars).min(chars.len()) {
                candidate.push(chars[end - 1]);
                if let Some(&score) = self.pieces.get(candidate.as_str()) {
                    covered |= end == start + 1;
                    let total = base + score;
                    if best[end].is_none_or(|(old, _, _)| total > old) {
                        best[end] = Some((total, start, false));
                    }
                }
            }
            if !covered {
                let total = base + self.unknown_score;
                if best[start + 1].is_none_or(|(old, _, _)| total > old) {
                    best[start + 1] = Some((total, start, true));
                }
            }
        }

        let mut spans = Vec::new();
        let mut end = chars.len();
        while end > 0 {
            let Some((_, start, unknown)) = best[end] else {
                // Unreachable: every character can be an unknown piece.
                break;
            };
            spans.push((start, end, unknown));
            end = start;
        }
        spans.reverse();

        let mut pieces: Vec<String> = Vec::with_capacity(spans.len());
        let mut previous_unknown = false;
        for (start, end, unknown) in spans {
            let text: String = chars[start..end].iter().collect();
            match pieces.last_mut() {
                Some(last) if unknown && previous_unknown => last.push_str(&text),
                _ => pieces.push(text),
            }
            previous_unknown = unknown;
        }
        pieces
    }
}

/// NFKC as SentencePiece's `nmt_nfkc` rule, whitespace squeezed and turned
/// into word-start marks, with one in front.
fn normalize(text: &str) -> String {
    let nfkc = icu_normalizer::ComposingNormalizerBorrowed::new_nfkc().normalize(text);
    let words: Vec<&str> = nfkc
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for word in words {
        out.push(WORD_START);
        out.push_str(word);
    }
    out
}

/// The model's vocabulary: piece ↔ id.
#[derive(Debug)]
pub struct Vocabulary {
    tokens: Vec<String>,
    ids: HashMap<String, u32>,
}

impl Vocabulary {
    /// Reads `vocab.txt`: one token per line, the line number is its id.
    pub fn from_list(list: &str) -> Self {
        let tokens: Vec<String> = list.lines().map(str::to_string).collect();
        let ids = tokens
            .iter()
            .enumerate()
            .map(|(id, token)| (token.clone(), id as u32))
            .collect();
        Self { tokens, ids }
    }

    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Ids of `pieces`, `unknown` for those the model does not know.
    pub fn ids(&self, pieces: &[String], unknown: u32) -> Vec<u32> {
        pieces
            .iter()
            .map(|piece| self.ids.get(piece).copied().unwrap_or(unknown))
            .collect()
    }

    /// Text of generated ids, skipping `special` ones.
    pub fn text(&self, ids: &[u32], special: &[u32]) -> String {
        let joined: String = ids
            .iter()
            .filter(|id| !special.contains(id))
            .filter_map(|&id| self.tokens.get(id as usize))
            .map(String::as_str)
            .collect();
        joined.replace(WORD_START, " ").trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "<unk>\t0\t2\n<s>\t0\t3\n</s>\t0\t3\n▁\t-2\t1\n▁牛\t-5\t1\n牛乳\t-4\t1\n▁牛乳\t-6\t1\n乳\t-6\t1\nビール\t-3\t1\n▁ビール\t-4\t1\n生\t-5\t1\n";

    #[test]
    fn segments_with_the_best_scores() {
        let tokenizer = SourceTokenizer::from_table(TABLE).unwrap();
        // "▁牛乳" (-5.5) beats "▁" + "牛乳" (-6) and "▁牛" + "乳" (-11).
        assert_eq!(tokenizer.pieces("牛乳"), ["▁牛乳"]);
        assert_eq!(tokenizer.pieces("  生ビール "), ["▁", "生", "ビール"]);
    }

    #[test]
    fn normalizes_and_marks_words() {
        let tokenizer = SourceTokenizer::from_table(TABLE).unwrap();
        // Half-width katakana become full-width (NFKC).
        assert_eq!(tokenizer.pieces("ﾋﾞｰﾙ"), ["▁ビール"]);
        assert_eq!(tokenizer.pieces("牛乳 ビール"), ["▁牛乳", "▁ビール"]);
        assert!(tokenizer.pieces(" \t ").is_empty());
    }

    #[test]
    fn unknown_characters_merge_into_one_piece() {
        let tokenizer = SourceTokenizer::from_table(TABLE).unwrap();
        assert_eq!(tokenizer.pieces("牛乳xyz"), ["▁牛乳", "xyz"]);
    }

    #[test]
    fn rejects_broken_tables() {
        assert_eq!(
            SourceTokenizer::from_table("▁a\tnot-a-number\t1").unwrap_err(),
            TokenizerError::BadPiece(1)
        );
    }

    #[test]
    fn vocabulary_maps_both_ways() {
        let vocab = Vocabulary::from_list("</s>\n<unk>\n▁Milch\n▁fr\nisch\n<pad>");
        assert_eq!(vocab.len(), 6);
        assert_eq!(vocab.ids(&["▁Milch".into(), "▁Käse".into()], 1), [2, 1]);
        assert_eq!(vocab.text(&[5, 3, 4, 2, 0], &[0, 5]), "frisch Milch");
    }
}
