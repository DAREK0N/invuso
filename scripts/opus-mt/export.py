"""Builds the downloadable translation packs of Invuso (AP-21b, TRL-01).

Each pack is one Opus-MT model (Helsinki-NLP, Marian) as two int8-quantized
ONNX graphs plus plain-text tables the app reads without Python or
protobuf:

  encoder.onnx   input_ids, attention_mask -> last_hidden_state
  decoder.onnx   input_ids, encoder_hidden_states, encoder_attention_mask
                 -> logits of the last position (no key/value cache: receipt
                 lines are a few tokens long, recomputing is cheap)
  source.tsv     SentencePiece pieces of the source language:
                 piece <TAB> score <TAB> type (1 normal, 2 unknown, 3 control,
                 4 user defined, 5 unused, 6 byte)
  vocab.txt      model vocabulary, one token per line, line number = id
  config.json    special ids and limits
  samples.tsv    text <TAB> ids <TAB> translation, greedy with onnxruntime on
                 the quantized graphs, for the app's (ignored) end-to-end test

Run in a throwaway virtualenv with torch (CPU), transformers, sentencepiece,
sacremoses, onnx and onnxruntime:

  python export.py <out_dir> [pair ...]     (default pairs: ja-de ja-en)

It prints the file sizes and SHA-256 sums that go into
`crates/invuso-app/src/services/translation/packs.rs`.
"""

import hashlib
import json
import pathlib
import sys

import numpy
import onnxruntime
import sentencepiece
import torch
from onnxruntime.quantization import QuantType, quantize_dynamic
from transformers import MarianMTModel, MarianTokenizer

SAMPLES = {
    "ja": ["牛乳", "生ビール", "おにぎり 鮭", "小計", "レジ袋", "日用雑貨", "鹿角ごみ収集袋小"],
}


class Encoder(torch.nn.Module):
    def __init__(self, model):
        super().__init__()
        self.encoder = model.get_encoder()

    def forward(self, input_ids, attention_mask):
        return self.encoder(input_ids=input_ids, attention_mask=attention_mask).last_hidden_state


class Decoder(torch.nn.Module):
    def __init__(self, model):
        super().__init__()
        self.decoder = model.get_decoder()
        self.lm_head = model.lm_head
        self.register_buffer("bias", model.final_logits_bias)

    def forward(self, input_ids, encoder_hidden_states, encoder_attention_mask):
        hidden = self.decoder(
            input_ids=input_ids,
            encoder_hidden_states=encoder_hidden_states,
            encoder_attention_mask=encoder_attention_mask,
            use_cache=False,
        ).last_hidden_state
        return self.lm_head(hidden[:, -1, :]) + self.bias


def greedy(out: pathlib.Path, ids: list[int], config: dict) -> list[int]:
    """The decoding loop of the app (`opus.rs`), on onnxruntime."""
    encoder = onnxruntime.InferenceSession(str(out / "encoder.onnx"))
    decoder = onnxruntime.InferenceSession(str(out / "decoder.onnx"))
    input_ids = numpy.array([ids], dtype=numpy.int64)
    mask = numpy.ones_like(input_ids)
    (hidden,) = encoder.run(None, {"input_ids": input_ids, "attention_mask": mask})
    generated = [config["decoder_start_id"]]
    limit = min(config["max_length"], 2 * len(ids) + 8)
    while len(generated) <= limit:
        (logits,) = decoder.run(
            None,
            {
                "input_ids": numpy.array([generated], dtype=numpy.int64),
                "encoder_hidden_states": hidden,
                "encoder_attention_mask": mask,
            },
        )
        logits = logits[0]
        logits[config["pad_id"]] = -numpy.inf
        token = int(logits.argmax())
        if token == config["eos_id"]:
            break
        generated.append(token)
    return generated[1:]


def export(pair: str, out: pathlib.Path) -> None:
    name = f"Helsinki-NLP/opus-mt-{pair}"
    out.mkdir(parents=True, exist_ok=True)
    tokenizer = MarianTokenizer.from_pretrained(name)
    model = MarianMTModel.from_pretrained(name).eval()

    ids = tokenizer("牛乳 2", return_tensors="pt")
    hidden = Encoder(model)(ids.input_ids, ids.attention_mask)
    start = torch.tensor([[model.config.decoder_start_token_id]])
    plain = out / "plain"
    plain.mkdir(exist_ok=True)
    torch.onnx.export(
        Encoder(model),
        (ids.input_ids, ids.attention_mask),
        plain / "encoder.onnx",
        input_names=["input_ids", "attention_mask"],
        output_names=["last_hidden_state"],
        dynamic_axes={"input_ids": {1: "n"}, "attention_mask": {1: "n"}, "last_hidden_state": {1: "n"}},
        opset_version=17,
        dynamo=False,
    )
    torch.onnx.export(
        Decoder(model),
        (start, hidden, ids.attention_mask),
        plain / "decoder.onnx",
        input_names=["input_ids", "encoder_hidden_states", "encoder_attention_mask"],
        output_names=["logits"],
        dynamic_axes={
            "input_ids": {1: "m"},
            "encoder_hidden_states": {1: "n"},
            "encoder_attention_mask": {1: "n"},
        },
        opset_version=17,
        dynamo=False,
    )
    for part in ["encoder", "decoder"]:
        # Gather too: the token embeddings are the largest weights.
        quantize_dynamic(
            plain / f"{part}.onnx",
            out / f"{part}.onnx",
            weight_type=QuantType.QInt8,
            op_types_to_quantize=["MatMul", "Gather"],
        )

    spm = sentencepiece.SentencePieceProcessor(model_file=str(pathlib.Path(tokenizer.spm_files[0])))
    with open(out / "source.tsv", "w", encoding="utf-8", newline="\n") as f:
        for i in range(spm.get_piece_size()):
            kind = 2 if spm.is_unknown(i) else 3 if spm.is_control(i) else 5 if spm.is_unused(i) else 6 if spm.is_byte(i) else 1
            f.write(f"{spm.id_to_piece(i)}\t{spm.get_score(i)}\t{kind}\n")

    vocab = tokenizer.get_vocab()
    by_id = sorted(vocab.items(), key=lambda item: item[1])
    assert [i for _, i in by_id] == list(range(len(by_id))), "vocabulary ids are not dense"
    with open(out / "vocab.txt", "w", encoding="utf-8", newline="\n") as f:
        for token, _ in by_id:
            assert "\n" not in token
            f.write(token + "\n")

    config = {
        "eos_id": tokenizer.eos_token_id,
        "pad_id": tokenizer.pad_token_id,
        "unk_id": tokenizer.unk_token_id,
        "decoder_start_id": model.config.decoder_start_token_id,
        "max_length": 64,
    }
    (out / "config.json").write_text(json.dumps(config, indent=2), encoding="utf-8")

    source = pair.split("-")[0]
    with open(out / "samples.tsv", "w", encoding="utf-8", newline="\n") as f:
        for text in SAMPLES.get(source, []):
            ids = tokenizer(text).input_ids
            translation = tokenizer.decode(greedy(out, ids, config), skip_special_tokens=True)
            f.write(f"{text}\t{' '.join(map(str, ids))}\t{translation}\n")

    for path in sorted(plain.iterdir()):
        path.unlink()
    plain.rmdir()


def main() -> None:
    out = pathlib.Path(sys.argv[1])
    pairs = sys.argv[2:] or ["ja-de", "ja-en"]
    for pair in pairs:
        export(pair, out / pair)
        for path in sorted((out / pair).iterdir()):
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            print(f"{pair}/{path.name}\t{path.stat().st_size}\t{digest}")


if __name__ == "__main__":
    main()
