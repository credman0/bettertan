use anyhow::{anyhow, bail, Context, Result};
use image::{imageops::FilterType, DynamicImage};
use ndarray::{s, Array4, Ix1};
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::TensorRef,
};

// ── Embedded resources ────────────────────────────────────────────────────────

static MODEL_BYTES: &[u8] =
    include_bytes!("../resources/ram_plus_swin_large_14m.onnx");

static TAGS_TEXT: &str =
    include_str!("../resources/ram_tag_list.txt");

// ── Constants ─────────────────────────────────────────────────────────────────

const IMAGE_SIZE: u32 = 384;

const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];

// ── Public types ──────────────────────────────────────────────────────────────

/// A single predicted tag and its confidence score.
#[derive(Debug, Clone, PartialEq)]
pub struct TagResult {
    pub tag: String,
    pub score: f32,
}

/// Options controlling which tags are returned.
#[derive(Debug, Clone, Copy)]
pub struct TagOptions {
    /// Minimum confidence score for a tag to be included in `above_threshold`.
    pub threshold: f32,
    /// How many top-k tags to always return (regardless of threshold).
    pub topk: usize,
}

impl Default for TagOptions {
    fn default() -> Self {
        Self {
            threshold: 0.68,
            topk: 30,
        }
    }
}

/// Combined output of a tagging run.
#[derive(Debug, Clone, PartialEq)]
pub struct TagOutput {
    /// Tags whose confidence is ≥ `threshold`, sorted by score descending.
    pub above_threshold: Vec<TagResult>,
    /// The top-k tags by score, sorted descending.
    pub topk: Vec<TagResult>,
}

// ── Tagger ────────────────────────────────────────────────────────────────────

/// Wraps an ONNX inference session and the tag vocabulary.
///
/// Build once with [`Tagger::new`] and reuse for multiple images.
pub struct Tagger {
    session: Session,
    tags: Vec<String>,
}

impl Tagger {
    /// Initialise the tagger, loading the model and tags from the embedded bytes.
    pub fn new() -> Result<Self> {
        let tags = parse_tags(TAGS_TEXT);

        let session = Session::builder()
            .map_err(ort_err)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort_err)?
            .with_intra_threads(num_cpus::get().min(4) as usize)
            .map_err(ort_err)?
            .commit_from_memory(MODEL_BYTES)
            .map_err(ort_err)
            .context("failed to initialise ONNX session from embedded model")?;

        Ok(Self { session, tags })
    }

    /// Run inference on the image at `path` and return predicted tags.
    pub fn tag_image(&mut self, path: &str, opts: TagOptions) -> Result<TagOutput> {
        let input = preprocess_image(path, IMAGE_SIZE)?;
        let probs = self.run_session(&input)?;

        if probs.len() != self.tags.len() {
            bail!(
                "tag count mismatch: model produced {} scores, vocabulary has {} entries",
                probs.len(),
                self.tags.len()
            );
        }

        let above_threshold = {
            let mut v: Vec<TagResult> = probs
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, p)| *p >= opts.threshold)
                .map(|(i, score)| TagResult {
                    tag: self.tags[i].clone(),
                    score,
                })
                .collect();
            v.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
            v
        };

        let topk = {
            let mut idxs = topk_indices(&probs, opts.topk);
            idxs.iter()
                .map(|&i| TagResult {
                    tag: self.tags[i].clone(),
                    score: probs[i],
                })
                .collect()
        };

        Ok(TagOutput {
            above_threshold,
            topk,
        })
    }

    // ── Private ───────────────────────────────────────────────────────────────

    fn run_session(&mut self, input: &Array4<f32>) -> Result<Vec<f32>> {
        let tensor = TensorRef::from_array_view(input).map_err(ort_err)?;
        let outputs = self
            .session
            .run(ort::inputs!["pixel_values" => tensor])
            .map_err(ort_err)?;

        if let Some(value) = outputs.get("probs") {
            let arr = value.try_extract_array::<f32>().map_err(ort_err)?;
            extract_1d_vec(arr.view()).context("unexpected shape for `probs` output")
        } else if let Some(value) = outputs.get("logits") {
            let arr = value.try_extract_array::<f32>().map_err(ort_err)?;
            let logits =
                extract_1d_vec(arr.view()).context("unexpected shape for `logits` output")?;
            Ok(logits.into_iter().map(sigmoid).collect())
        } else {
            bail!("model outputs did not include `probs` or `logits`");
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn ort_err<E: std::fmt::Display>(e: E) -> anyhow::Error {
    anyhow!(e.to_string())
}

fn parse_tags(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn preprocess_image(path: &str, size: u32) -> Result<Array4<f32>> {
    let img = image::open(path)
        .with_context(|| format!("failed to open image: {path}"))?;
    let rgb = img
        .resize_exact(size, size, FilterType::CatmullRom)
        .to_rgb8();

    let mut tensor = Array4::<f32>::zeros((1, 3, size as usize, size as usize));

    for (y, row) in rgb.rows().enumerate() {
        for (x, pixel) in row.enumerate() {
            tensor[[0, 0, y, x]] =
                (pixel[0] as f32 / 255.0 - IMAGENET_MEAN[0]) / IMAGENET_STD[0];
            tensor[[0, 1, y, x]] =
                (pixel[1] as f32 / 255.0 - IMAGENET_MEAN[1]) / IMAGENET_STD[1];
            tensor[[0, 2, y, x]] =
                (pixel[2] as f32 / 255.0 - IMAGENET_MEAN[2]) / IMAGENET_STD[2];
        }
    }

    Ok(tensor)
}

fn extract_1d_vec(
    view: ndarray::ArrayViewD<'_, f32>,
) -> std::result::Result<Vec<f32>, ndarray::ShapeError> {
    // Try 1-D first; on failure try to treat as [1, N] and take row 0.
    // We must reborrow rather than consume `view` so it stays available for the
    // fallback path — `into_dimensionality` takes `self` by value.
    match view.view().into_dimensionality::<Ix1>() {
        Ok(a) => Ok(a.to_vec()),
        Err(_) => {
            let arr2 = view.into_dimensionality::<ndarray::Ix2>()?;
            Ok(arr2.slice(s![0, ..]).to_vec())
        }
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

fn topk_indices(values: &[f32], k: usize) -> Vec<usize> {
    let mut idxs: Vec<usize> = (0..values.len()).collect();
    idxs.sort_by(|&a, &b| {
        values[b]
            .partial_cmp(&values[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    idxs.truncate(k.min(idxs.len()));
    idxs
}
