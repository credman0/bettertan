use anyhow::{anyhow, bail, Context, Result};
use image::{imageops::FilterType, DynamicImage};
use ndarray::{s, Array4, Ix1};
use ort::{
    session::{builder::GraphOptimizationLevel, Session},
    value::TensorRef,
};

const IMAGE_SIZE: u32 = 384;
const MODEL_PATH: &str = "resources/ram_plus_swin_large_14m.onnx";
const TAGS_PATH: &str = "resources/ram_tag_list.txt";

const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];

fn ort_err<E: std::fmt::Display>(e: E) -> anyhow::Error {
    anyhow!(e.to_string())
}

fn load_tags(path: &str) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read tag file: {path}"))?;

    let tags = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();

    Ok(tags)
}

fn preprocess_image(path: &str, image_size: u32) -> Result<Array4<f32>> {
    let img = image::open(path)
        .with_context(|| format!("failed to open image: {path}"))?;
    let rgb = to_resized_rgb8(img, image_size, image_size);

    let mut input = Array4::<f32>::zeros((1, 3, image_size as usize, image_size as usize));

    for (y, row) in rgb.rows().enumerate() {
        for (x, pixel) in row.enumerate() {
            let r = (pixel[0] as f32 / 255.0 - IMAGENET_MEAN[0]) / IMAGENET_STD[0];
            let g = (pixel[1] as f32 / 255.0 - IMAGENET_MEAN[1]) / IMAGENET_STD[1];
            let b = (pixel[2] as f32 / 255.0 - IMAGENET_MEAN[2]) / IMAGENET_STD[2];

            input[[0, 0, y, x]] = r;
            input[[0, 1, y, x]] = g;
            input[[0, 2, y, x]] = b;
        }
    }

    Ok(input)
}

fn to_resized_rgb8(img: DynamicImage, width: u32, height: u32) -> image::RgbImage {
    img.resize_exact(width, height, FilterType::CatmullRom).to_rgb8()
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

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        bail!("usage: cargo run -- <image.jpg> [threshold] [topk]");
    }

    let image_path = &args[0];
    let threshold: f32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.68);
    let topk: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(30);

    let tags = load_tags(TAGS_PATH)?;
    let input = preprocess_image(image_path, IMAGE_SIZE)?;

    let mut session = Session::builder()
        .map_err(ort_err)?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(ort_err)?
        .with_intra_threads(1)
        .map_err(ort_err)?
        .commit_from_file(MODEL_PATH)
        .map_err(ort_err)
        .with_context(|| format!("failed to load model: {MODEL_PATH}"))?;

    let input_tensor = TensorRef::from_array_view(&input).map_err(ort_err)?;
    let outputs = session
        .run(ort::inputs!["pixel_values" => input_tensor])
        .map_err(ort_err)?;

    let probs_vec: Vec<f32> = if let Some(value) = outputs.get("probs") {
        let arr = value.try_extract_array::<f32>().map_err(ort_err)?;
        arr.view()
            .into_dimensionality::<Ix1>()
            .map(|a| a.to_vec())
            .or_else(|_| {
                let arr2 = arr.view().into_dimensionality::<ndarray::Ix2>()?;
                Ok::<Vec<f32>, ndarray::ShapeError>(arr2.slice(s![0, ..]).to_vec())
            })
            .context("unexpected shape for probs output")?
    } else if let Some(value) = outputs.get("logits") {
        let arr = value.try_extract_array::<f32>().map_err(ort_err)?;
        let logits = arr
            .view()
            .into_dimensionality::<Ix1>()
            .map(|a| a.to_vec())
            .or_else(|_| {
                let arr2 = arr.view().into_dimensionality::<ndarray::Ix2>()?;
                Ok::<Vec<f32>, ndarray::ShapeError>(arr2.slice(s![0, ..]).to_vec())
            })
            .context("unexpected shape for logits output")?;

        logits.into_iter().map(sigmoid).collect()
    } else {
        bail!("model outputs did not include `probs` or `logits`");
    };

    if probs_vec.len() != tags.len() {
        bail!(
            "tag count mismatch: model produced {} scores, tag file has {} entries",
            probs_vec.len(),
            tags.len()
        );
    }

    println!("Image: {image_path}");
    println!("Model: {MODEL_PATH}");
    println!("Tags: {TAGS_PATH}");
    println!();
    println!("Predicted tags (threshold = {threshold:.3}):");

    let mut selected: Vec<(usize, f32)> = probs_vec
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, p)| *p >= threshold)
        .collect();

    selected.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if selected.is_empty() {
        println!("(none above threshold)");
    } else {
        for (idx, score) in &selected {
            println!("{score:.4}\t{}", tags[*idx]);
        }
    }

    println!();
    println!("Top {topk} tags:");
    for idx in topk_indices(&probs_vec, topk) {
        println!("{:.4}\t{}", probs_vec[idx], tags[idx]);
    }

    Ok(())
}