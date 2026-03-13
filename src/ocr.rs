use anyhow::{Context, Result};
use paddle_ocr_rs::{EngineConfig, OcrCallOptions, OcrInput, RapidOcr};

use crate::storage::data_dir;

// ── Model paths ───────────────────────────────────────────────────────────────

fn model_dir() -> std::path::PathBuf {
    data_dir().join("ocr_models")
}

/// Returns a multi-line string with the exact curl commands needed to populate
/// `~/.image_tagger/ocr_models/`.
pub fn model_setup_instructions() -> String {
    let dir = model_dir();
    let d = dir.display();
    format!(
        "OCR models not found in {d}\n\n\
         Run the following to download them:\n\n\
         mkdir -p {d}\n\n\
         curl -L -o {d}/ch_PP-OCRv5_mobile_det.onnx \\\n  \
           https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv5/det/ch_PP-OCRv5_mobile_det.onnx\n\n\
         curl -L -o {d}/ch_ppocr_mobile_v2.0_cls_infer.onnx \\\n  \
           https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv4/cls/ch_ppocr_mobile_v2.0_cls_infer.onnx\n\n\
         curl -L -o {d}/en_PP-OCRv5_rec_mobile_infer.onnx \\\n  \
           https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.7.0/onnx/PP-OCRv5/rec/en_PP-OCRv5_rec_mobile_infer.onnx"
    )
}

// ── OcrEngine ─────────────────────────────────────────────────────────────────

/// Wraps an initialised `RapidOcr` session.
///
/// Build once with [`OcrEngine::new`]; reuse for multiple images.
/// Returns `Err` with human-readable setup instructions when the model files
/// are absent so the UI can surface the message directly.
pub struct OcrEngine {
    ocr: RapidOcr,
}

impl OcrEngine {
    /// Load the three PP-OCRv5 models from `~/.image_tagger/ocr_models/`.
    pub fn new() -> Result<Self> {
        let dir = model_dir();
        let det = dir.join("ch_PP-OCRv5_mobile_det.onnx");
        let cls = dir.join("ch_ppocr_mobile_v2.0_cls_infer.onnx");
        let rec = dir.join("en_PP-OCRv5_rec_mobile_infer.onnx");

        // Surface a helpful error before attempting to load if any file is absent.
        for path in [&det, &cls, &rec] {
            if !path.exists() {
                anyhow::bail!("{}", model_setup_instructions());
            }
        }

        let mut config = EngineConfig::default();
        config.det.model_path = Some(det);
        config.det.allow_download = false;
        config.cls.model_path = Some(cls);
        config.cls.allow_download = false;
        config.rec.model.model_path = Some(rec);
        config.rec.model.allow_download = false;

        let ocr = RapidOcr::new(config)
            .map_err(|e| anyhow::anyhow!("OCR init failed: {e:#}"))?;

        Ok(Self { ocr })
    }

    /// Run OCR on the image at `image_path`.
    ///
    /// Text blocks are joined with a single space; empty blocks are skipped.
    /// Returns an empty string when no text is detected.
    pub fn extract_text(&mut self, image_path: &str) -> Result<String> {
        let result = self
            .ocr
            .run(
                OcrInput::Path(image_path.into()),
                OcrCallOptions::default(),
            )
            .map_err(|e| anyhow::anyhow!("OCR inference failed: {e:#}"))?;

        let text = result
            .txts
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");

        Ok(text)
    }
}
