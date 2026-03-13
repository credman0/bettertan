use std::{fs, io::Write, path::Path};

use anyhow::Result;
use paddle_ocr_rs::{
    EngineConfig, LangCls, LangDet, LangRec, ModelType, OcrCallOptions, OcrInput, OcrVersion,
    RapidOcr,
};

use crate::storage::data_dir;

fn model_dir() -> std::path::PathBuf {
    data_dir().join("ocr_models")
}

const MODELS: &[(&str, &str)] = &[
    (
        "ch_PP-OCRv5_mobile_det.onnx",
        "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.6.0/onnx/PP-OCRv5/det/ch_PP-OCRv5_mobile_det.onnx",
    ),
    (
        "ch_ppocr_mobile_v2.0_cls_infer.onnx",
        "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.6.0/onnx/PP-OCRv4/cls/ch_ppocr_mobile_v2.0_cls_infer.onnx",
    ),
    (
        "en_PP-OCRv5_rec_mobile_infer.onnx",
        "https://www.modelscope.cn/models/RapidAI/RapidOCR/resolve/v3.6.0/onnx/PP-OCRv5/rec/en_PP-OCRv5_rec_mobile_infer.onnx",
    ),
];

fn download_models_if_missing() -> Result<()> {
    let dir = model_dir();
    fs::create_dir_all(&dir)?;

    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36")
        .build()?;

    for (filename, url) in MODELS {
        let dest = dir.join(filename);
        if dest.exists() {
            continue;
        }
        eprintln!("Downloading OCR model: {filename}");
        download_file(&client, url, &dest)
            .map_err(|e| anyhow::anyhow!("Failed to download {filename}: {e:#}"))?;
    }

    Ok(())
}

fn download_file(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
) -> Result<()> {
    let tmp = dest.with_extension("part");

    let mut response = client.get(url).send()?;
    if !response.status().is_success() {
        anyhow::bail!("HTTP {}", response.status());
    }

    {
        let mut file = fs::File::create(&tmp)?;
        response.copy_to(&mut file)?;
        file.flush()?;
    }

    if dest.exists() {
        fs::remove_file(dest)?;
    }
    fs::rename(&tmp, dest)?;

    Ok(())
}

/// Wraps an initialised `RapidOcr` session.
///
/// Build once with [`OcrEngine::new`]; reuse for multiple images.
/// Models are downloaded automatically to `~/.image_tagger/ocr_models/` on
/// first use if they are not already present.
pub struct OcrEngine {
    ocr: RapidOcr,
}

impl OcrEngine {
    pub fn new() -> Result<Self> {
        download_models_if_missing()?;

        let dir = model_dir();
        let mut config = EngineConfig::default();

        config.det.ocr_version = OcrVersion::PPocrV5;
        config.det.lang = LangDet::Ch;
        config.det.model_type = ModelType::Mobile;
        config.det.model_path = Some(dir.join("ch_PP-OCRv5_mobile_det.onnx"));
        config.det.allow_download = false;

        config.cls.ocr_version = OcrVersion::PPocrV4;
        config.cls.lang = LangCls::Ch;
        config.cls.model_type = ModelType::Mobile;
        config.cls.model_path = Some(dir.join("ch_ppocr_mobile_v2.0_cls_infer.onnx"));
        config.cls.allow_download = false;

        config.rec.model.ocr_version = OcrVersion::PPocrV5;
        config.rec.model.lang = LangRec::En;
        config.rec.model.model_type = ModelType::Mobile;
        config.rec.model.model_path = Some(dir.join("en_PP-OCRv5_rec_mobile_infer.onnx"));
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
