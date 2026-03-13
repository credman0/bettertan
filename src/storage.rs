use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

// ── Data directory ────────────────────────────────────────────────────────────

/// Returns `~/.image_tagger/` (cross-platform via HOME / USERPROFILE).
pub fn data_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".image_tagger")
}

fn ensure_data_dir() -> Result<PathBuf> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create data dir: {}", dir.display()))?;
    Ok(dir)
}

// ── .idx path convention ──────────────────────────────────────────────────────

/// Returns the `.idx` sidecar path for a given image.
///
/// `photo.jpg` → `photo.jpg.idx`
pub fn idx_path_for(image_path: &Path) -> PathBuf {
    let mut s = image_path.as_os_str().to_os_string();
    s.push(".idx");
    PathBuf::from(s)
}

// ── Public types ──────────────────────────────────────────────────────────────

/// One entry in the library: image file + its parsed idx sidecar.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryEntry {
    /// Absolute path to the image inside the data dir.
    pub image_path: PathBuf,
    /// Model tags saved at tagging time (name, score).
    pub tags: Vec<(String, f32)>,
    /// User-supplied custom tags.
    pub custom_tags: Vec<String>,
    /// OCR text extracted from the image, if available.
    pub ocr_text: Option<String>,
}

impl LibraryEntry {
    /// All tag names concatenated (model tags first, then custom).
    pub fn all_tag_names(&self) -> Vec<String> {
        self.tags
            .iter()
            .map(|(t, _)| t.clone())
            .chain(self.custom_tags.iter().cloned())
            .collect()
    }

    pub fn image_file_name(&self) -> String {
        self.image_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }
}

// ── Save ──────────────────────────────────────────────────────────────────────

/// Copy `src_image` into the data dir and write its `.idx` sidecar.
///
/// If a file with the same name already exists, the image is renamed with a
/// numeric suffix (the idx always matches the image that was actually stored).
///
/// Returns the path of the image in the data dir.
pub fn save_entry(
    src_image: &Path,
    model_tags: &[(String, f32)],
    custom_tags: &[String],
    ocr_text: Option<&str>,
) -> Result<PathBuf> {
    let dir = ensure_data_dir()?;

    let file_name = src_image
        .file_name()
        .context("image path has no file name")?;
    let dest = dedup_path(&dir.join(file_name));

    std::fs::copy(src_image, &dest)
        .with_context(|| format!("failed to copy image to {}", dest.display()))?;

    write_idx(&idx_path_for(&dest), model_tags, custom_tags, ocr_text)?;

    Ok(dest)
}

/// Update the `.idx` sidecar for an image that is **already in the data dir**,
/// or copy it in and write a fresh sidecar if it is not.
///
/// Returns the path of the image in the data dir (unchanged when in-place).
pub fn save_or_update_entry(
    src_image: &Path,
    model_tags: &[(String, f32)],
    custom_tags: &[String],
    ocr_text: Option<&str>,
) -> Result<PathBuf> {
    // If the image already lives inside the data dir, just rewrite its idx.
    if src_image.starts_with(data_dir()) {
        write_idx(&idx_path_for(src_image), model_tags, custom_tags, ocr_text)?;
        return Ok(src_image.to_path_buf());
    }
    // Otherwise do the full copy + idx write.
    save_entry(src_image, model_tags, custom_tags, ocr_text)
}

// ── Load ──────────────────────────────────────────────────────────────────────

/// Scan the data dir and return every (image, idx) pair that is well-formed.
/// Also scans the `memes/` subdirectory so generated memes appear here too.
pub fn load_all_entries() -> Vec<LibraryEntry> {
    let dir = data_dir();
    if !dir.exists() {
        return vec![];
    }

    let mut entries: Vec<LibraryEntry> = Vec::new();
    scan_dir_for_entries(&dir, &mut entries);
    let memes_subdir = dir.join("memes");
    if memes_subdir.exists() {
        scan_dir_for_entries(&memes_subdir, &mut entries);
    }

    // Stable order: sort alphabetically by file name
    entries.sort_by(|a, b| a.image_file_name().cmp(&b.image_file_name()));
    entries
}

fn scan_dir_for_entries(dir: &Path, entries: &mut Vec<LibraryEntry>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let ext = match p.extension().and_then(|s| s.to_str()) {
            Some(e) => e.to_lowercase(),
            None => continue,
        };
        if !matches!(
            ext.as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif" | "tiff"
        ) {
            continue;
        }
        let idx = idx_path_for(&p);
        if !idx.exists() {
            continue;
        }
        if let Ok(entry) = parse_idx(&p, &idx) {
            entries.push(entry);
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// idx file format:
/// ```
/// # image tagger index v1
/// [tags]
/// person=0.9321
/// outdoor=0.8100
/// [custom]
/// golden hour
/// portrait session
/// [ocr]
/// Any text detected in the image lives here.
/// This section is always last; everything after [ocr] is raw text.
/// ```
fn write_idx(
    path: &Path,
    model_tags: &[(String, f32)],
    custom_tags: &[String],
    ocr_text: Option<&str>,
) -> Result<()> {
    let mut out = String::from("# image tagger index v1\n[tags]\n");
    for (tag, score) in model_tags {
        out.push_str(&format!("{}={:.4}\n", tag, score));
    }
    out.push_str("[custom]\n");
    for t in custom_tags {
        let t = t.trim();
        if !t.is_empty() {
            out.push_str(t);
            out.push('\n');
        }
    }
    // [ocr] is always written last; its content is raw text to EOF.
    if let Some(text) = ocr_text {
        let text = text.trim();
        if !text.is_empty() {
            out.push_str("[ocr]\n");
            out.push_str(text);
            out.push('\n');
        }
    }
    std::fs::write(path, out).with_context(|| format!("failed to write {}", path.display()))
}

fn parse_idx(image_path: &Path, idx_path: &Path) -> Result<LibraryEntry> {
    let text = std::fs::read_to_string(idx_path)
        .with_context(|| format!("failed to read {}", idx_path.display()))?;

    let mut tags = Vec::new();
    let mut custom_tags = Vec::new();
    let mut ocr_lines: Vec<&str> = Vec::new();
    let mut section = "";

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[tags]" {
            section = "tags";
            continue;
        }
        if line == "[custom]" {
            section = "custom";
            continue;
        }
        if line == "[ocr]" {
            section = "ocr";
            continue;
        }
        match section {
            "tags" => {
                if let Some((name, score_s)) = line.split_once('=') {
                    let score = score_s.trim().parse::<f32>().unwrap_or(0.0);
                    tags.push((name.trim().to_owned(), score));
                }
            }
            "custom" => custom_tags.push(line.to_owned()),
            "ocr"    => ocr_lines.push(line),
            _        => {}
        }
    }

    let ocr_text = if ocr_lines.is_empty() {
        None
    } else {
        Some(ocr_lines.join(" "))
    };

    Ok(LibraryEntry {
        image_path: image_path.to_path_buf(),
        tags,
        custom_tags,
        ocr_text,
    })
}

// ── UI state persistence ──────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct UiState {
    pub active_tab: String,
    pub tagger_image: Option<PathBuf>,
    pub library_selected: Option<PathBuf>,
}

pub fn load_ui_state() -> UiState {
    let path = data_dir().join("ui_state");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return UiState::default();
    };
    let mut state = UiState::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match k {
            "tab"               => state.active_tab = v.to_string(),
            "tagger_image"      => state.tagger_image = Some(PathBuf::from(v)),
            "library_selected"  => state.library_selected = Some(PathBuf::from(v)),
            _                   => {}
        }
    }
    state
}

pub fn save_ui_state(state: &UiState) -> Result<()> {
    let dir = ensure_data_dir()?;
    let mut out = String::new();
    if !state.active_tab.is_empty() {
        out.push_str(&format!("tab={}\n", state.active_tab));
    }
    if let Some(p) = &state.tagger_image {
        out.push_str(&format!("tagger_image={}\n", p.display()));
    }
    if let Some(p) = &state.library_selected {
        out.push_str(&format!("library_selected={}\n", p.display()));
    }
    std::fs::write(dir.join("ui_state"), out)
        .with_context(|| "failed to write ui_state")
}

/// Load, apply `f`, save. All UI state writes go through here.
pub fn update_ui_state(f: impl FnOnce(&mut UiState)) -> Result<()> {
    let mut state = load_ui_state();
    f(&mut state);
    save_ui_state(&state)
}

// ── Path deduplication ────────────────────────────────────────────────────────

/// Returns `path` unchanged if it doesn't exist; otherwise appends `_1`, `_2`, …
fn dedup_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new("."));
    for i in 1u32.. {
        let name = if ext.is_empty() {
            format!("{}_{}", stem, i)
        } else {
            format!("{}_{}.{}", stem, i, ext)
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}
