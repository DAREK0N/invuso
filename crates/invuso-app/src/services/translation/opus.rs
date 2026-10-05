//! Opus-MT (Helsinki-NLP, Marian) on the device with `rten` (AP-21b,
//! decision 10.2): the fallback when the device brings no translator.
//!
//! The pack's encoder reads the source once; the decoder then predicts one
//! token after the other (greedy). It has no key/value cache: receipt lines
//! are a handful of tokens, so recomputing the short prefix is cheap and
//! keeps the graph simple.

use std::path::Path;

use rten::Model;
use rten_tensor::prelude::*;
use rten_tensor::{NdTensor, Tensor};
use serde::Deserialize;

use super::tokenizer::{SourceTokenizer, TokenizerError, Vocabulary};

use super::TranslationConfidence;

/// How sure the model must be for each level: minimum mean log probability
/// of the tokens and minimum of any single token. Opus-MT is trained on
/// sentences; for receipt abbreviations it often makes up a fluent
/// sentence ("おにぎり 鮭" → "Ich hab's."), and those come with low token
/// probabilities. Measured on 30 receipt words (`scripts/opus-mt`): at
/// "balanced" the good translations pass and most made-up ones do not;
/// "strict" also drops near misses like "お釣り" → "Angeln", but "パン" →
/// "Brot" too.
const STRICT: (f32, f32) = (-0.6, -1.0);
const BALANCED: (f32, f32) = (-1.0, -2.0);

#[derive(Debug, thiserror::Error)]
pub enum OpusError {
    #[error("translation pack incomplete: {0}")]
    Pack(String),
    #[error(transparent)]
    Tokenizer(#[from] TokenizerError),
    #[error("translation model failed: {0}")]
    Model(String),
}

#[derive(Debug, Clone, Deserialize)]
struct PackConfig {
    eos_id: u32,
    pad_id: u32,
    unk_id: u32,
    decoder_start_id: u32,
    max_length: usize,
}

/// One loaded language pair.
pub struct OpusMt {
    encoder: Model,
    decoder: Model,
    tokenizer: SourceTokenizer,
    vocab: Vocabulary,
    config: PackConfig,
}

/// A translation with how sure the model was.
#[derive(Debug, Clone, PartialEq)]
pub struct Translated {
    pub text: String,
    pub mean_log_prob: f32,
    pub min_log_prob: f32,
}

impl Translated {
    /// Whether the translation is sure enough for `level`.
    pub fn passes(&self, level: TranslationConfidence) -> bool {
        let (mean, min) = match level {
            TranslationConfidence::Strict => STRICT,
            TranslationConfidence::Balanced => BALANCED,
            TranslationConfidence::All => return true,
        };
        self.mean_log_prob >= mean && self.min_log_prob >= min
    }
}

impl OpusMt {
    /// Loads a pack directory as written by `scripts/opus-mt/export.py`.
    pub fn load(dir: &Path) -> Result<Self, OpusError> {
        let read = |name: &str| {
            std::fs::read_to_string(dir.join(name))
                .map_err(|e| OpusError::Pack(format!("{name}: {e}")))
        };
        let model = |name: &str| {
            Model::load_file(dir.join(name)).map_err(|e| OpusError::Pack(format!("{name}: {e}")))
        };
        let config: PackConfig = serde_json::from_str(&read("config.json")?)
            .map_err(|e| OpusError::Pack(format!("config.json: {e}")))?;
        let vocab = Vocabulary::from_list(&read("vocab.txt")?);
        if [
            config.eos_id,
            config.pad_id,
            config.unk_id,
            config.decoder_start_id,
        ]
        .iter()
        .any(|&id| id as usize >= vocab.len())
        {
            return Err(OpusError::Pack(
                "config.json: id outside the vocabulary".into(),
            ));
        }
        Ok(Self {
            encoder: model("encoder.onnx")?,
            decoder: model("decoder.onnx")?,
            tokenizer: SourceTokenizer::from_table(&read("source.tsv")?)?,
            vocab,
            config,
        })
    }

    /// Model ids of `text`, ending with the end-of-sentence id.
    pub fn source_ids(&self, text: &str) -> Vec<u32> {
        let pieces = self.tokenizer.pieces(text);
        let mut ids = self.vocab.ids(&pieces, self.config.unk_id);
        ids.push(self.config.eos_id);
        ids
    }

    /// Translates one line; `None` for a line without text.
    pub fn translate(&self, text: &str) -> Result<Option<Translated>, OpusError> {
        let source = self.source_ids(text);
        if source.len() <= 1 {
            return Ok(None);
        }
        let failed = |e: rten::RunError| OpusError::Model(e.to_string());
        let n = source.len();
        let input_ids = NdTensor::from_data(
            [1, n],
            source.iter().map(|&id| id as i32).collect::<Vec<_>>(),
        );
        let mask = NdTensor::from_data([1, n], vec![1_i32; n]);

        let enc_ids = self.encoder.node_id("input_ids").map_err(failed)?;
        let enc_mask = self.encoder.node_id("attention_mask").map_err(failed)?;
        let enc_out = self.encoder.node_id("last_hidden_state").map_err(failed)?;
        let [hidden] = self
            .encoder
            .run_n(
                vec![
                    (enc_ids, input_ids.view().into()),
                    (enc_mask, mask.view().into()),
                ],
                [enc_out],
                None,
            )
            .map_err(failed)?;
        let hidden: Tensor<f32> = hidden
            .try_into()
            .map_err(|_| OpusError::Model("unexpected encoder output".into()))?;

        let dec_ids = self.decoder.node_id("input_ids").map_err(failed)?;
        let dec_hidden = self
            .decoder
            .node_id("encoder_hidden_states")
            .map_err(failed)?;
        let dec_mask = self
            .decoder
            .node_id("encoder_attention_mask")
            .map_err(failed)?;
        let dec_out = self.decoder.node_id("logits").map_err(failed)?;

        // Receipt lines translate to about as many tokens as they have.
        let limit = self.config.max_length.min(2 * n + 8);
        let mut generated = vec![self.config.decoder_start_id];
        let mut log_probs = Vec::new();
        while generated.len() <= limit {
            let prefix = NdTensor::from_data(
                [1, generated.len()],
                generated.iter().map(|&id| id as i32).collect::<Vec<_>>(),
            );
            let [logits] = self
                .decoder
                .run_n(
                    vec![
                        (dec_ids, prefix.view().into()),
                        (dec_hidden, hidden.view().into()),
                        (dec_mask, mask.view().into()),
                    ],
                    [dec_out],
                    None,
                )
                .map_err(failed)?;
            let logits: Tensor<f32> = logits
                .try_into()
                .map_err(|_| OpusError::Model("unexpected decoder output".into()))?;
            let (next, log_prob) = best_token(&logits.to_vec(), self.config.pad_id)
                .ok_or_else(|| OpusError::Model("empty decoder output".into()))?;
            log_probs.push(log_prob);
            if next == self.config.eos_id {
                break;
            }
            generated.push(next);
        }

        let special = [self.config.eos_id, self.config.pad_id, self.config.unk_id];
        let text = clean(&self.vocab.text(&generated[1..], &special));
        if text.is_empty() {
            return Ok(None);
        }
        let mean_log_prob = log_probs.iter().sum::<f32>() / log_probs.len() as f32;
        let min_log_prob = log_probs.iter().copied().fold(f32::INFINITY, f32::min);
        Ok(Some(Translated {
            text,
            mean_log_prob,
            min_log_prob,
        }))
    }
}

/// The most likely token and its log probability; `banned` (padding) is
/// never chosen, as Marian's generation config says.
fn best_token(logits: &[f32], banned: u32) -> Option<(u32, f32)> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if !max.is_finite() {
        return None;
    }
    let log_sum = logits.iter().map(|&l| (l - max).exp()).sum::<f32>().ln() + max;
    logits
        .iter()
        .enumerate()
        .filter(|&(id, _)| id as u32 != banned)
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(id, &logit)| (id as u32, logit - log_sum))
}

/// Sentence habits that do not belong on a receipt line: the dialogue dash
/// and the full stop of "Bier.".
fn clean(text: &str) -> String {
    let text = text.trim();
    let text = text.strip_prefix("- ").unwrap_or(text);
    let text = match text.strip_suffix('.') {
        // "Nr." and "z. B." keep their dot.
        Some(rest) if !rest.contains('.') && rest.chars().count() > 3 => rest,
        _ => text,
    };
    text.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn best_token_skips_padding_and_gives_log_probability() {
        let (id, log_prob) = best_token(&[0.0, 1.0, 5.0], 2).unwrap();
        assert_eq!(id, 1);
        let expected = 1.0 - (1.0_f32.exp() + 1.0 + 5.0_f32.exp()).ln();
        assert!((log_prob - expected).abs() < 1e-5);
        assert_eq!(best_token(&[], 0), None);
    }

    #[test]
    fn cleans_sentence_habits() {
        assert_eq!(clean("Bier."), "Bier");
        assert_eq!(clean("- Grüner Tee."), "Grüner Tee");
        assert_eq!(clean("Nr."), "Nr.");
        assert_eq!(clean("z. B."), "z. B.");
        assert_eq!(clean(" Milch "), "Milch");
    }

    #[test]
    fn confidence_levels() {
        use TranslationConfidence::{All, Balanced, Strict};
        let line = |mean, min| Translated {
            text: "x".into(),
            mean_log_prob: mean,
            min_log_prob: min,
        };
        // Milch, Brot, Angeln, "Ich hab's" as measured.
        let (milk, bread, fishing, made_up) = (
            line(-0.39, -0.44),
            line(-0.81, -1.14),
            line(-0.61, -1.09),
            line(-1.63, -3.1),
        );
        assert!(milk.passes(Strict) && milk.passes(Balanced) && milk.passes(All));
        assert!(!bread.passes(Strict) && bread.passes(Balanced));
        assert!(!fishing.passes(Strict) && fishing.passes(Balanced));
        assert!(!made_up.passes(Strict) && !made_up.passes(Balanced) && made_up.passes(All));
    }

    /// End to end against an exported pack; run with
    /// `INVUSO_OPUS_PACK=<dir>/ja-de cargo test -p invuso-app -- --ignored opus`.
    #[test]
    #[ignore = "needs an exported translation pack"]
    fn matches_the_reference_of_the_export_script() {
        let dir = std::env::var("INVUSO_OPUS_PACK").expect("INVUSO_OPUS_PACK");
        let dir = Path::new(&dir);
        let engine = OpusMt::load(dir).unwrap();
        let samples = std::fs::read_to_string(dir.join("samples.tsv")).unwrap();
        for line in samples.lines() {
            let [text, ids, reference] = line.split('\t').collect::<Vec<_>>()[..] else {
                panic!("bad sample line {line}");
            };
            let ids: Vec<u32> = ids.split(' ').map(|id| id.parse().unwrap()).collect();
            assert_eq!(engine.source_ids(text), ids, "ids of {text}");
            let started = std::time::Instant::now();
            let translated = engine.translate(text).unwrap().unwrap();
            println!(
                "{text} -> {} ({:.2}/{:.2}, {} ms; reference: {reference})",
                translated.text,
                translated.mean_log_prob,
                translated.min_log_prob,
                started.elapsed().as_millis()
            );
            // int8 arithmetic of rten and onnxruntime rounds slightly apart,
            // which can flip a close token choice; such unsure lines are
            // never shown, so only confident ones must agree exactly.
            if translated.passes(TranslationConfidence::Balanced) {
                assert_eq!(translated.text, clean(reference), "translation of {text}");
            }
        }
    }
}
