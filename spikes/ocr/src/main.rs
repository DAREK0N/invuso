//! OCR spike (AP-S1): runs several on-device OCR candidates over the sample
//! receipts and writes reconstructed lines plus timings to `results/`.
//!
//! Usage: `cargo run --release -- <engine> [<engine> ...]`
//! Engines: ocrs, v5-ort, v5-rten, v5latin-ort, v6s-ort, v6s-rten, v6t-ort

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use image::{DynamicImage, ImageDecoder, ImageReader, RgbImage};
use rten_tensor::prelude::*;

/// A recognized text fragment with its axis-aligned box in image pixels.
#[derive(Clone, Debug)]
struct Fragment {
    x0: f32,
    y0: f32,
    y1: f32,
    text: String,
    score: f32,
}

struct Timing {
    load_ms: u128,
    infer_ms: u128,
}

fn load_image(path: &Path) -> Result<RgbImage> {
    let mut decoder = ImageReader::open(path)?
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut img = DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    Ok(img.into_rgb8())
}

// ---------------------------------------------------------------- ocrs

struct OcrsEngine {
    engine: ocrs::OcrEngine,
}

impl OcrsEngine {
    fn new(models: &Path) -> Result<Self> {
        let det = rten::Model::load_file(models.join("ocrs-detection.rten"))?;
        let rec = rten::Model::load_file(models.join("ocrs-recognition.rten"))?;
        let engine = ocrs::OcrEngine::new(ocrs::OcrEngineParams {
            detection_model: Some(det),
            recognition_model: Some(rec),
            ..Default::default()
        })?;
        Ok(Self { engine })
    }

    fn run(&self, img: &RgbImage) -> Result<Vec<Fragment>> {
        use ocrs::TextItem;
        let src = ocrs::ImageSource::from_bytes(img.as_raw(), img.dimensions())?;
        let input = self.engine.prepare_input(src)?;
        let words = self.engine.detect_words(&input)?;
        let lines = self.engine.find_text_lines(&input, &words);
        let texts = self.engine.recognize_text(&input, &lines)?;
        Ok(texts
            .into_iter()
            .flatten()
            .filter(|l| l.to_string().trim().len() > 1)
            .map(|l| {
                let r = l.bounding_rect();
                Fragment {
                    x0: r.left() as f32,
                    y0: r.top() as f32,
                    y1: r.bottom() as f32,
                    text: l.to_string(),
                    score: 1.0,
                }
            })
            .collect())
    }
}

// ---------------------------------------------------------------- Paddle

/// Inference backend for an ONNX model with one float input and one float output.
enum Backend {
    #[cfg(not(target_os = "android"))]
    Ort(Box<ort::session::Session>),
    Rten(Box<rten::Model>),
}

impl Backend {
    fn load(path: &Path, use_ort: bool) -> Result<Self> {
        #[cfg(not(target_os = "android"))]
        if use_ort {
            let session = ort::session::Session::builder()
                .map_err(|e| anyhow!("{e}"))?
                .with_intra_threads(4)
                .map_err(|e| anyhow!("{e}"))?
                .commit_from_file(path)
                .map_err(|e| anyhow!("{e}"))?;
            return Ok(Backend::Ort(Box::new(session)));
        }
        if use_ort {
            bail!("ort is not available on this target");
        }
        Ok(Backend::Rten(Box::new(rten::Model::load_file(path)?)))
    }

    /// Runs the model; returns (shape, data) of the first output.
    fn run(&mut self, shape: [usize; 4], data: Vec<f32>) -> Result<(Vec<usize>, Vec<f32>)> {
        match self {
            #[cfg(not(target_os = "android"))]
            Backend::Ort(session) => {
                let shape_i: Vec<i64> = shape.iter().map(|&d| d as i64).collect();
                let tensor = ort::value::Tensor::from_array((shape_i, data))?;
                let outputs = session.run(ort::inputs![tensor])?;
                let (s, d) = outputs[0].try_extract_tensor::<f32>()?;
                Ok((s.iter().map(|&x| x as usize).collect(), d.to_vec()))
            }
            Backend::Rten(model) => {
                let input = rten_tensor::Tensor::from_data(&shape, data);
                let out = model.run_one(input.view().into(), None)?;
                let out: rten_tensor::Tensor<f32> = out
                    .try_into()
                    .map_err(|_| anyhow!("unexpected output type"))?;
                Ok((out.shape().to_vec(), out.to_vec()))
            }
        }
    }
}

struct PaddleEngine {
    det: Backend,
    rec: Backend,
    chars: Vec<String>,
    thresh: f32,
    box_thresh: f32,
    unclip: f32,
}

impl PaddleEngine {
    fn new(
        models: &Path,
        det: &str,
        rec: &str,
        dict: &str,
        use_ort: bool,
        (thresh, box_thresh, unclip): (f32, f32, f32),
    ) -> Result<Self> {
        let mut chars = vec![String::new()]; // CTC blank
        chars.extend(
            fs::read_to_string(models.join(dict))?
                .lines()
                .map(|l| l.trim_end_matches('\r').to_string()),
        );
        chars.push(" ".into());
        Ok(Self {
            det: Backend::load(&models.join(det), use_ort)?,
            rec: Backend::load(&models.join(rec), use_ort)?,
            chars,
            thresh,
            box_thresh,
            unclip,
        })
    }

    fn run(&mut self, img: &RgbImage) -> Result<Vec<Fragment>> {
        let boxes = self.detect(img)?;
        let mut out = Vec::new();
        for (x0, y0, x1, y1) in boxes {
            let (text, score) = self.recognize(img, x0, y0, x1, y1)?;
            if !text.trim().is_empty() {
                out.push(Fragment {
                    x0: x0 as f32,
                    y0: y0 as f32,
                    y1: y1 as f32,
                    text,
                    score,
                });
            }
        }
        Ok(out)
    }

    /// DBNet detection with axis-aligned boxes (good enough for upright receipts).
    fn detect(&mut self, img: &RgbImage) -> Result<Vec<(u32, u32, u32, u32)>> {
        let (w, h) = img.dimensions();
        let min_side = w.min(h) as f32;
        let mut scale = if min_side < 736.0 {
            736.0 / min_side
        } else {
            1.0
        };
        let det_max: f32 = std::env::var("DET_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4000.0);
        let max_side = w.max(h) as f32 * scale;
        if max_side > det_max {
            scale *= det_max / max_side;
        }
        let rw = (((w as f32 * scale) / 32.0).round().max(1.0) * 32.0) as u32;
        let rh = (((h as f32 * scale) / 32.0).round().max(1.0) * 32.0) as u32;
        let resized = image::imageops::resize(img, rw, rh, image::imageops::FilterType::Triangle);
        let mean = [0.485f32, 0.456, 0.406];
        let std = [0.229f32, 0.224, 0.225];
        let plane = (rw * rh) as usize;
        let mut data = vec![0f32; 3 * plane];
        for (i, p) in resized.pixels().enumerate() {
            // Paddle feeds BGR.
            let bgr = [p[2], p[1], p[0]];
            for c in 0..3 {
                data[c * plane + i] = (bgr[c] as f32 / 255.0 - mean[c]) / std[c];
            }
        }
        let (shape, prob) = self.det.run([1, 3, rh as usize, rw as usize], data)?;
        let (mh, mw) = (shape[shape.len() - 2], shape[shape.len() - 1]);
        let sx = w as f32 / mw as f32;
        let sy = h as f32 / mh as f32;

        let mut seen = vec![false; mh * mw];
        let mut boxes = Vec::new();
        let mut stack = Vec::new();
        for start in 0..mh * mw {
            if seen[start] || prob[start] <= self.thresh {
                continue;
            }
            seen[start] = true;
            stack.push(start);
            let (mut bx0, mut by0, mut bx1, mut by1) = (mw, mh, 0usize, 0usize);
            let (mut sum, mut n) = (0f32, 0usize);
            while let Some(i) = stack.pop() {
                let (x, y) = (i % mw, i / mw);
                bx0 = bx0.min(x);
                by0 = by0.min(y);
                bx1 = bx1.max(x);
                by1 = by1.max(y);
                sum += prob[i];
                n += 1;
                let mut push = |j: usize| {
                    if !seen[j] && prob[j] > self.thresh {
                        seen[j] = true;
                        stack.push(j);
                    }
                };
                if x > 0 {
                    push(i - 1);
                }
                if x + 1 < mw {
                    push(i + 1);
                }
                if y > 0 {
                    push(i - mw);
                }
                if y + 1 < mh {
                    push(i + mw);
                }
            }
            let bw = (bx1 - bx0 + 1) as f32;
            let bh = (by1 - by0 + 1) as f32;
            if bw.min(bh) < 3.0 || sum / (n as f32) < self.box_thresh {
                continue;
            }
            // Unclip like Paddle's DBPostProcess: offset = area * ratio / perimeter.
            let d = bw * bh * self.unclip / (2.0 * (bw + bh));
            let x0 = ((bx0 as f32 - d) * sx).max(0.0) as u32;
            let y0 = ((by0 as f32 - d) * sy).max(0.0) as u32;
            let x1 = (((bx1 as f32 + 1.0 + d) * sx) as u32).min(w);
            let y1 = (((by1 as f32 + 1.0 + d) * sy) as u32).min(h);
            if x1 > x0 + 2 && y1 > y0 + 2 {
                boxes.push((x0, y0, x1, y1));
            }
        }
        Ok(boxes)
    }

    fn recognize(
        &mut self,
        img: &RgbImage,
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
    ) -> Result<(String, f32)> {
        let crop = image::imageops::crop_imm(img, x0, y0, x1 - x0, y1 - y0).to_image();
        let th = 48u32;
        let tw = ((crop.width() as f32 * th as f32 / crop.height() as f32).ceil() as u32)
            .clamp(16, 3200);
        let resized = image::imageops::resize(&crop, tw, th, image::imageops::FilterType::Triangle);
        let plane = (tw * th) as usize;
        let mut data = vec![0f32; 3 * plane];
        for (i, p) in resized.pixels().enumerate() {
            let bgr = [p[2], p[1], p[0]];
            for c in 0..3 {
                data[c * plane + i] = (bgr[c] as f32 / 255.0 - 0.5) / 0.5;
            }
        }
        let (shape, out) = self.rec.run([1, 3, th as usize, tw as usize], data)?;
        let (steps, classes) = (shape[1], shape[2]);
        if classes != self.chars.len() {
            bail!(
                "model has {classes} classes, dictionary {}",
                self.chars.len()
            );
        }
        let mut text = String::new();
        let mut last = 0usize;
        let (mut psum, mut pn) = (0f32, 0usize);
        for t in 0..steps {
            let row = &out[t * classes..(t + 1) * classes];
            let (idx, p) =
                row.iter()
                    .enumerate()
                    .fold((0, f32::MIN), |a, (i, &v)| if v > a.1 { (i, v) } else { a });
            if idx != 0 && idx != last {
                text.push_str(&self.chars[idx]);
                psum += p;
                pn += 1;
            }
            last = idx;
        }
        Ok((text, if pn > 0 { psum / pn as f32 } else { 0.0 }))
    }
}

// ---------------------------------------------------------------- lines

/// Groups fragments into visual lines (same height band), left to right.
fn group_lines(mut frags: Vec<Fragment>) -> Vec<String> {
    frags.sort_by(|a, b| (a.y0 + a.y1).total_cmp(&(b.y0 + b.y1)));
    let mut lines: Vec<Vec<Fragment>> = Vec::new();
    for f in frags {
        let fc = (f.y0 + f.y1) / 2.0;
        let fh = f.y1 - f.y0;
        if let Some(line) = lines.last_mut() {
            let ly0 = line.iter().map(|g| g.y0).sum::<f32>() / line.len() as f32;
            let ly1 = line.iter().map(|g| g.y1).sum::<f32>() / line.len() as f32;
            let lc = (ly0 + ly1) / 2.0;
            if (fc - lc).abs() < 0.5 * fh.min(ly1 - ly0) {
                line.push(f);
                continue;
            }
        }
        lines.push(vec![f]);
    }
    lines
        .into_iter()
        .map(|mut l| {
            l.sort_by(|a, b| a.x0.total_cmp(&b.x0));
            l.iter()
                .map(|f| f.text.trim())
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect()
}

// ---------------------------------------------------------------- main

enum Engine {
    Ocrs(Box<OcrsEngine>),
    Paddle(Box<PaddleEngine>),
}

impl Engine {
    fn create(name: &str, models: &Path) -> Result<Self> {
        let v5 = (0.3, 0.6, 1.5);
        let v6 = (0.2, 0.45, 1.4);
        Ok(match name {
            "ocrs" => Engine::Ocrs(Box::new(OcrsEngine::new(models)?)),
            "v5-ort" | "v5-rten" => Engine::Paddle(Box::new(PaddleEngine::new(
                models,
                "ch_PP-OCRv5_det_mobile.onnx",
                "ch_PP-OCRv5_rec_mobile.onnx",
                "ppocrv5_dict.txt",
                name.ends_with("ort"),
                v5,
            )?)),
            "v5latin-ort" => Engine::Paddle(Box::new(PaddleEngine::new(
                models,
                "ch_PP-OCRv5_det_mobile.onnx",
                "latin_PP-OCRv5_rec_mobile.onnx",
                "ppocrv5_latin_dict.txt",
                true,
                v5,
            )?)),
            "v6s-ort" | "v6s-rten" => Engine::Paddle(Box::new(PaddleEngine::new(
                models,
                "PP-OCRv6_det_small.onnx",
                "PP-OCRv6_rec_small.onnx",
                "ppocrv6_dict.txt",
                name.ends_with("ort"),
                v6,
            )?)),
            "v6ts-rten" => Engine::Paddle(Box::new(PaddleEngine::new(
                models,
                "PP-OCRv6_det_tiny.onnx",
                "PP-OCRv6_rec_small.onnx",
                "ppocrv6_dict.txt",
                false,
                v6,
            )?)),
            "v6t-ort" => Engine::Paddle(Box::new(PaddleEngine::new(
                models,
                "PP-OCRv6_det_tiny.onnx",
                "PP-OCRv6_rec_tiny.onnx",
                "ppocrv6_tiny_dict.txt",
                true,
                v6,
            )?)),
            _ => bail!("unknown engine {name}"),
        })
    }

    fn run(&mut self, img: &RgbImage) -> Result<Vec<Fragment>> {
        match self {
            Engine::Ocrs(e) => e.run(img),
            Engine::Paddle(e) => e.run(img),
        }
    }
}

fn main() -> Result<()> {
    let root = std::env::var("SPIKE_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let models = root.join("models");
    let samples = root.join("samples");
    let filter = std::env::var("SAMPLE").ok();
    let mut images: Vec<PathBuf> = fs::read_dir(&samples)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("jpg" | "png")))
        .filter(|p| {
            filter
                .as_deref()
                .is_none_or(|f| p.to_string_lossy().contains(f))
        })
        .collect();
    images.sort();

    for name in std::env::args().skip(1) {
        let t = Instant::now();
        let mut engine =
            Engine::create(&name, &models).with_context(|| format!("loading {name}"))?;
        let model_ms = t.elapsed().as_millis();
        let out_dir = root.join("results").join(&name);
        fs::create_dir_all(&out_dir)?;
        let mut csv = fs::File::create(out_dir.join("timings.csv"))?;
        writeln!(
            csv,
            "image,width,height,load_ms,infer_ms,lines,model_load_ms"
        )?;
        println!("== {name} (models loaded in {model_ms} ms)");
        for path in &images {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("img");
            let t = Instant::now();
            let img = load_image(path)?;
            let load_ms = t.elapsed().as_millis();
            let t = Instant::now();
            let frags = engine
                .run(&img)
                .with_context(|| format!("{name} on {stem}"))?;
            let timing = Timing {
                load_ms,
                infer_ms: t.elapsed().as_millis(),
            };
            let avg = frags.iter().map(|f| f.score).sum::<f32>() / frags.len().max(1) as f32;
            let lines = group_lines(frags);
            fs::write(out_dir.join(format!("{stem}.txt")), lines.join("\n") + "\n")?;
            writeln!(
                csv,
                "{stem},{},{},{},{},{},{model_ms}",
                img.width(),
                img.height(),
                timing.load_ms,
                timing.infer_ms,
                lines.len()
            )?;
            println!(
                "{stem:22} {:>6} ms  {:>3} lines  score {avg:.2}",
                timing.infer_ms,
                lines.len()
            );
        }
    }
    Ok(())
}
