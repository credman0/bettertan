use std::collections::HashSet;
use std::path::PathBuf;

use dioxus::prelude::*;

use crate::{image_to_data_url, image_to_thumbnail_url, storage};

// ── Blanks directory helpers ─────────────────────────────────────────────────

fn blanks_dir() -> PathBuf {
    storage::data_dir().join("blanks")
}

fn ensure_blanks_dir() -> std::io::Result<PathBuf> {
    let dir = blanks_dir();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// A blank image entry — just a path, no config.
#[derive(Debug, Clone, PartialEq)]
pub struct BlankEntry {
    pub image_path: PathBuf,
    pub file_name: String,
}

/// Scan the blanks directory for image files.
fn load_blanks() -> Vec<BlankEntry> {
    let dir = blanks_dir();
    if !dir.exists() {
        return vec![];
    }
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let mut out: Vec<BlankEntry> = rd
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let ext = p.extension()?.to_str()?.to_lowercase();
            if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif" | "tiff") {
                return None;
            }
            let file_name = p.file_name()?.to_string_lossy().to_string();
            Some(BlankEntry { image_path: p, file_name })
        })
        .collect();
    out.sort_by(|a, b| a.file_name.to_lowercase().cmp(&b.file_name.to_lowercase()));
    out
}

/// Search blanks by filename.
fn search_blanks(entries: &[BlankEntry], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (0..entries.len()).collect();
    }
    entries.iter().enumerate()
        .filter(|(_, e)| e.file_name.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect()
}

// ── Favorites ────────────────────────────────────────────────────────────────

fn favorites_path() -> PathBuf {
    storage::data_dir().join("blank_favorites")
}

fn load_favorites() -> HashSet<String> {
    let Ok(text) = std::fs::read_to_string(favorites_path()) else {
        return HashSet::new();
    };
    text.lines()
        .map(|l| l.trim().to_owned())
        .filter(|l| !l.is_empty())
        .collect()
}

fn save_favorites(favorites: &HashSet<String>) {
    let mut ids: Vec<&str> = favorites.iter().map(|s| s.as_str()).collect();
    ids.sort();
    let _ = std::fs::write(favorites_path(), ids.join("\n"));
}

fn toggle_favorite(name: &str) -> HashSet<String> {
    let mut favs = load_favorites();
    if !favs.remove(name) {
        favs.insert(name.to_owned());
    }
    save_favorites(&favs);
    favs
}

// ── Generate image with bottom text ──────────────────────────────────────────

fn generate_blank_with_text(image_path: &std::path::Path, text: &str) -> anyhow::Result<PathBuf> {
    use ab_glyph::PxScale;
    use imageproc::drawing::{draw_text_mut, text_size};

    let img = image::open(image_path)
        .map_err(|e| anyhow::anyhow!("cannot open {}: {e}", image_path.display()))?;
    let mut canvas = img.to_rgba8();
    let (img_w, img_h) = (canvas.width(), canvas.height());

    if text.trim().is_empty() {
        anyhow::bail!("No text entered.");
    }

    let font = load_default_font()
        .ok_or_else(|| anyhow::anyhow!("Failed to load font"))?;

    let display_text = text.to_uppercase();

    // Use a fixed font size proportional to image width, word-wrap on
    // whitespace, and draw overlaid on the bottom of the image.
    let font_size = (img_w as f32 * 0.07).clamp(20.0, 120.0);
    let scale = PxScale::from(font_size);
    let line_h = font_size * 1.25;
    let padding = 10u32;
    let usable_w = img_w.saturating_sub(padding * 2);

    let lines = word_wrap(&font, &display_text, scale, usable_w);
    let total_h = lines.len() as f32 * line_h;

    // Position the text block so its bottom edge sits at the bottom of the
    // image (with a small padding).
    let block_top = img_h as f32 - total_h - padding as f32;

    let fg = image::Rgba([255, 255, 255, 255]);
    let stroke = image::Rgba([0, 0, 0, 255]);

    for (li, line) in lines.iter().enumerate() {
        let (lw, _) = text_size(scale, &font, line);
        let draw_x = (img_w as i32 - lw as i32) / 2;
        let draw_y = (block_top + li as f32 * line_h) as i32;

        // Stroke
        for ox in -2i32..=2 {
            for oy in -2i32..=2 {
                if ox != 0 || oy != 0 {
                    draw_text_mut(&mut canvas, stroke, draw_x + ox, draw_y + oy, scale, &font, line);
                }
            }
        }
        draw_text_mut(&mut canvas, fg, draw_x, draw_y, scale, &font, line);
    }

    let dir = storage::data_dir();
    std::fs::create_dir_all(&dir)?;
    let stem = image_path.file_stem().unwrap_or_default().to_string_lossy();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let out = dir.join(format!("blank_{}_{}.png", stem, ts));
    canvas.save(&out).map_err(|e| anyhow::anyhow!("cannot save: {e}"))?;

    // Write a library .idx sidecar so it appears in the image library.
    let idx_path = storage::idx_path_for(&out);
    let _ = std::fs::write(
        &idx_path,
        "# image tagger index v1\n[tags]\nmeme=1.0000\n[custom]\nblank\n",
    );

    Ok(out)
}

fn load_default_font() -> Option<ab_glyph::FontVec> {
    let bytes = google_fonts::league_gothic_regular_variable().ok()?;
    ab_glyph::FontVec::try_from_vec(bytes).ok()
}

fn word_wrap(
    font: &ab_glyph::FontVec,
    text: &str,
    scale: ab_glyph::PxScale,
    max_w: u32,
) -> Vec<String> {
    use imageproc::drawing::text_size;

    let mut out = Vec::new();
    for para in text.split('\n') {
        let words: Vec<&str> = para.split_whitespace().collect();
        if words.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in words {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if text_size(scale, font, &candidate).0 > max_w && !current.is_empty() {
                out.push(current);
                current = word.to_string();
            } else {
                current = candidate;
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
}

// ── Main view ────────────────────────────────────────────────────────────────

#[allow(non_snake_case)]
pub fn BlanksView() -> Element {
    let mut blanks: Signal<Vec<BlankEntry>> = use_signal(load_blanks);
    let mut selected: Signal<Option<usize>> = use_signal(|| None);
    let mut favorites: Signal<HashSet<String>> = use_signal(load_favorites);
    let mut query: Signal<String> = use_signal(String::new);
    let mut import_status: Signal<Option<Result<String, String>>> = use_signal(|| None);

    let refresh = move |_| {
        blanks.set(load_blanks());
        selected.set(None);
    };

    let add_image = move |_| {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tiff"])
            .pick_file()
        {
            match ensure_blanks_dir() {
                Ok(dir) => {
                    let fname = path.file_name().unwrap_or_default();
                    let dest = dir.join(fname);
                    if dest.exists() {
                        import_status.set(Some(Err(format!(
                            "An image named '{}' already exists in blanks.",
                            fname.to_string_lossy()
                        ))));
                    } else {
                        match std::fs::copy(&path, &dest) {
                            Ok(_) => {
                                import_status.set(Some(Ok(format!("Added {}", fname.to_string_lossy()))));
                                blanks.set(load_blanks());
                            }
                            Err(e) => import_status.set(Some(Err(e.to_string()))),
                        }
                    }
                }
                Err(e) => import_status.set(Some(Err(e.to_string()))),
            }
        }
    };

    let all_blanks = blanks.read().clone();
    let fav_set = favorites.read().clone();
    let query_str = query.read().clone();
    let searching = !query_str.trim().is_empty();
    let sorted_indices = search_blanks(&all_blanks, &query_str);

    let fav_blanks: Vec<(usize, BlankEntry)> = if !searching {
        sorted_indices.iter()
            .filter(|&&i| fav_set.contains(&all_blanks[i].file_name))
            .map(|&i| (i, all_blanks[i].clone()))
            .collect()
    } else {
        vec![]
    };
    let rest_blanks: Vec<(usize, BlankEntry)> = if !searching {
        sorted_indices.iter()
            .filter(|&&i| !fav_set.contains(&all_blanks[i].file_name))
            .map(|&i| (i, all_blanks[i].clone()))
            .collect()
    } else {
        sorted_indices.iter()
            .map(|&i| (i, all_blanks[i].clone()))
            .collect()
    };

    let selected_idx = selected.read().clone();
    let count = all_blanks.len();
    let vis_count = sorted_indices.len();
    let count_label = if searching {
        format!("{} / {} blank{}", vis_count, count, if count == 1 { "" } else { "s" })
    } else {
        format!("{} blank{}", count, if count == 1 { "" } else { "s" })
    };
    let has_favs = !fav_blanks.is_empty();

    let selected_blank: Option<BlankEntry> = selected_idx
        .and_then(|i| all_blanks.get(i).cloned());

    rsx! {
        div {
            style: "display:flex; height:100%; overflow:hidden;",

            // Left: grid
            div {
                style: "flex:1; display:flex; flex-direction:column; overflow:hidden;",

                // Toolbar
                div {
                    style: "display:flex; align-items:center; gap:12px; padding:9px 20px; border-bottom:1px solid #1e1e26; background:#13131a; flex-shrink:0;",
                    span {
                        style: "font-size:10px; color:#555; letter-spacing:0.12em; text-transform:uppercase; flex-shrink:0;",
                        "{count_label}"
                    }
                    input {
                        r#type: "text",
                        placeholder: "Search blanks…",
                        value: "{query_str}",
                        oninput: move |e| {
                            query.set(e.value());
                            selected.set(None);
                        },
                        style: "flex:1; min-width:0; background:#0d0d14; border:1px solid #2a2a38; border-radius:4px; padding:5px 10px; color:#ccc; font-family:inherit; font-size:11px; letter-spacing:0.04em; outline:none;",
                    }
                    button {
                        style: "padding:6px 14px; background:#1e1e30; color:#9b8dd4; border:1px solid #2e2e46; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.08em; cursor:pointer; flex-shrink:0;",
                        onclick: add_image,
                        "+  Add Image"
                    }
                    button {
                        style: "padding:6px 14px; background:#1a1a26; color:#888; border:1px solid #2a2a38; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.08em; cursor:pointer; flex-shrink:0;",
                        onclick: refresh,
                        "↺  Refresh"
                    }
                }

                // Import status
                if let Some(ref status) = *import_status.read() {
                    div {
                        style: "padding:6px 20px; flex-shrink:0;",
                        match status {
                            Ok(msg) => rsx! { span { style: "font-size:11px; color:#7ecba1;", "✓  {msg}" } },
                            Err(msg) => rsx! { span { style: "font-size:11px; color:#c0392b;", "✗  {msg}" } },
                        }
                    }
                }

                // Duplicate import popup
                if let Some(ref status) = *import_status.read() {
                    if let Err(msg) = status {
                        if msg.contains("already exists") {
                            div {
                                style: "position:fixed; top:0; left:0; right:0; bottom:0; background:rgba(0,0,0,0.6); display:flex; align-items:center; justify-content:center; z-index:1000;",
                                onclick: move |_| import_status.set(None),
                                div {
                                    style: "background:#1a1a26; border:1px solid #c0392b; border-radius:8px; padding:24px 32px; max-width:420px; text-align:center;",
                                    onclick: move |e| e.stop_propagation(),
                                    div {
                                        style: "font-size:14px; color:#e74c3c; margin-bottom:12px; font-weight:bold;",
                                        "Duplicate Image"
                                    }
                                    div {
                                        style: "font-size:12px; color:#ccc; line-height:1.6; margin-bottom:16px;",
                                        "{msg}"
                                    }
                                    button {
                                        style: "padding:7px 24px; background:#c0392b; color:#fff; border:none; border-radius:4px; font-family:inherit; font-size:11px; cursor:pointer;",
                                        onclick: move |_| import_status.set(None),
                                        "OK"
                                    }
                                }
                            }
                        }
                    }
                }

                // Grid
                div {
                    style: "flex:1; overflow-y:auto; padding:20px;",
                    if all_blanks.is_empty() {
                        EmptyBlanks {}
                    } else if searching {
                        if rest_blanks.is_empty() {
                            div {
                                style: "font-size:11px; color:#2e2e3a; text-align:center; padding-top:40px; letter-spacing:0.08em;",
                                "No matching blanks"
                            }
                        } else {
                            div {
                                style: "display:grid; grid-template-columns:repeat(auto-fill,minmax(170px,1fr)); gap:14px;",
                                for (orig_i, entry) in rest_blanks.iter() {
                                    BlankCard {
                                        key: "{orig_i}",
                                        entry: entry.clone(),
                                        selected: selected_idx == Some(*orig_i),
                                        favorited: fav_set.contains(&entry.file_name),
                                        onclick: {
                                            let oi = *orig_i;
                                            move |_| {
                                                let new = if *selected.read() == Some(oi) { None } else { Some(oi) };
                                                selected.set(new);
                                            }
                                        },
                                        on_toggle_favorite: {
                                            let name = entry.file_name.clone();
                                            move |_| { favorites.set(toggle_favorite(&name)); }
                                        },
                                    }
                                }
                            }
                        }
                    } else {
                        // Favorites section
                        if has_favs {
                            div {
                                style: "font-size:10px; color:#7a6a30; letter-spacing:0.12em; text-transform:uppercase; margin-bottom:10px;",
                                "★  Favorites"
                            }
                            div {
                                style: "display:grid; grid-template-columns:repeat(auto-fill,minmax(170px,1fr)); gap:14px; margin-bottom:24px;",
                                for (orig_i, entry) in fav_blanks.iter() {
                                    BlankCard {
                                        key: "fav-{orig_i}",
                                        entry: entry.clone(),
                                        selected: selected_idx == Some(*orig_i),
                                        favorited: true,
                                        onclick: {
                                            let oi = *orig_i;
                                            move |_| {
                                                let new = if *selected.read() == Some(oi) { None } else { Some(oi) };
                                                selected.set(new);
                                            }
                                        },
                                        on_toggle_favorite: {
                                            let name = entry.file_name.clone();
                                            move |_| { favorites.set(toggle_favorite(&name)); }
                                        },
                                    }
                                }
                            }
                            div {
                                style: "font-size:10px; color:#3a3a50; letter-spacing:0.12em; text-transform:uppercase; margin-bottom:10px;",
                                "All Blanks"
                            }
                        }
                        // Non-favorite blanks
                        div {
                            style: "display:grid; grid-template-columns:repeat(auto-fill,minmax(170px,1fr)); gap:14px;",
                            for (orig_i, entry) in rest_blanks.iter() {
                                BlankCard {
                                    key: "{orig_i}",
                                    entry: entry.clone(),
                                    selected: selected_idx == Some(*orig_i),
                                    favorited: false,
                                    onclick: {
                                        let oi = *orig_i;
                                        move |_| {
                                            let new = if *selected.read() == Some(oi) { None } else { Some(oi) };
                                            selected.set(new);
                                        }
                                    },
                                    on_toggle_favorite: {
                                        let name = entry.file_name.clone();
                                        move |_| { favorites.set(toggle_favorite(&name)); }
                                    },
                                }
                            }
                        }
                    }
                }
            }

            // Right: editor panel
            if let Some(entry) = selected_blank {
                BlankEditor {
                    key: "{entry.file_name}",
                    entry,
                    favorites,
                    on_delete: move |_| {
                        selected.set(None);
                        blanks.set(load_blanks());
                    },
                }
            }
        }
    }
}

// ── Blank card ───────────────────────────────────────────────────────────────

#[component]
#[allow(non_snake_case)]
fn BlankCard(
    entry: BlankEntry,
    selected: bool,
    favorited: bool,
    onclick: EventHandler<MouseEvent>,
    on_toggle_favorite: EventHandler<MouseEvent>,
) -> Element {
    let thumb_path = entry.image_path.clone();
    let thumb_res = use_resource(move || {
        let p = thumb_path.clone();
        async move {
            tokio::task::spawn_blocking(move || image_to_thumbnail_url(&p, 200))
                .await
                .ok()
                .flatten()
        }
    });
    let data_url: Option<String> = thumb_res.read().as_ref().and_then(|v| v.clone());
    let name = entry.file_name.clone();
    let border = if selected { "#5b8dee" } else { "#1e1e26" };
    let star = if favorited { "★" } else { "☆" };
    let star_color = if favorited { "#f0c040" } else { "#3a3a50" };

    rsx! {
        div {
            style: "background:#13131a; border:1px solid {border}; border-radius:6px; overflow:hidden; cursor:pointer; transition:border-color 0.15s; position:relative;",
            onclick: move |e| onclick.call(e),

            div {
                style: "width:100%; aspect-ratio:1/1; overflow:hidden; background:#0c0c0e; position:relative;",
                if let Some(src) = data_url {
                    img { src: "{src}", style: "width:100%; height:100%; object-fit:cover; display:block;" }
                } else {
                    div {
                        style: "width:100%; height:100%; display:flex; align-items:center; justify-content:center; color:#2e2e3a; font-size:10px;",
                        "no preview"
                    }
                }
                // Star button overlaid on image
                div {
                    style: "position:absolute; top:5px; right:5px; cursor:pointer; font-size:13px; color:{star_color}; padding:2px 5px; background:rgba(0,0,0,0.6); border-radius:3px; line-height:1;",
                    onclick: move |e| {
                        e.stop_propagation();
                        on_toggle_favorite.call(e);
                    },
                    "{star}"
                }
            }

            div {
                style: "padding:8px 10px;",
                div {
                    style: "font-size:11px; color:#bbb; white-space:nowrap; overflow:hidden; text-overflow:ellipsis;",
                    "{name}"
                }
            }
        }
    }
}

// ── Blank editor ─────────────────────────────────────────────────────────────

#[component]
#[allow(non_snake_case)]
fn BlankEditor(entry: BlankEntry, favorites: Signal<HashSet<String>>, on_delete: EventHandler<MouseEvent>) -> Element {
    let mut text: Signal<String> = use_signal(String::new);
    let mut generating: Signal<bool> = use_signal(|| false);
    let mut result: Signal<Option<Result<PathBuf, String>>> = use_signal(|| None);

    let name = entry.file_name.clone();
    let id = entry.file_name.clone();

    let favorited = favorites.read().contains(&id);

    // Load the base image asynchronously.
    let img_path = entry.image_path.clone();
    let img_res = use_resource(move || {
        let p = img_path.clone();
        async move {
            tokio::task::spawn_blocking(move || image_to_data_url(&p))
                .await
                .ok()
                .flatten()
        }
    });
    let base_url: Option<String> = img_res.read().as_ref().and_then(|v| v.clone());

    // After a successful generate, show the output image.
    let preview_src: Option<String> = {
        let r = result.read();
        if let Some(res) = r.as_ref() {
            if let Ok(path) = res {
                image_to_data_url(path)
            } else {
                base_url.clone()
            }
        } else {
            base_url.clone()
        }
    };

    let status_ok: Option<String> = result.read().as_ref().and_then(|r| {
        r.as_ref().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
    });
    let status_err: Option<String> = result.read().as_ref().and_then(|r| {
        r.as_ref().err().map(|e| e.clone())
    });

    let btn_disabled = *generating.read();
    let btn_label = if *generating.read() { "Generating…" } else { "Generate" };
    let btn_opacity = if btn_disabled { "0.5" } else { "1" };

    let fav_label = if favorited { "★" } else { "☆" };
    let fav_color = if favorited { "#f0c040" } else { "#555" };
    let fav_border = if favorited { "#8a7030" } else { "#2a2a38" };

    let library_dir = storage::data_dir();

    rsx! {
        div {
            style: "width:320px; border-left:1px solid #1e1e26; display:flex; flex-direction:column; background:#0d0d10; flex-shrink:0; overflow:hidden;",

            // Preview
            div {
                style: "width:100%; aspect-ratio:4/3; background:#0c0c0e; flex-shrink:0; overflow:hidden;",
                if let Some(src) = preview_src {
                    img { src: "{src}", style: "width:100%; height:100%; object-fit:contain; display:block;" }
                }
            }

            // File name + favorite
            div {
                style: "padding:11px 14px; border-bottom:1px solid #1a1a22; flex-shrink:0; display:flex; align-items:center; gap:8px;",
                div {
                    style: "flex:1; min-width:0;",
                    div {
                        style: "font-size:12px; color:#ddd; word-break:break-all;",
                        "{name}"
                    }
                }
                button {
                    style: "flex-shrink:0; padding:3px 8px; background:transparent; border:1px solid {fav_border}; border-radius:4px; color:{fav_color}; font-size:15px; cursor:pointer; line-height:1;",
                    onclick: {
                        let id = id.clone();
                        move |_| { favorites.set(toggle_favorite(&id)); }
                    },
                    "{fav_label}"
                }
            }

            // Text input
            div {
                style: "flex:1; overflow-y:auto; padding:12px 14px 6px;",
                div {
                    style: "font-size:10px; letter-spacing:0.12em; text-transform:uppercase; color:#3a3a50; margin-bottom:5px;",
                    "Bottom Text"
                }
                textarea {
                    style: "width:100%; background:#0c0c14; border:1px solid #1e1e2e; border-radius:4px; padding:7px 9px; color:#ccc; font-family:inherit; font-size:11px; resize:vertical; min-height:80px; letter-spacing:0.03em; line-height:1.5;",
                    placeholder: "Enter text for the bottom of the image…",
                    value: "{text}",
                    oninput: move |e| text.set(e.value()),
                }
            }

            // Actions
            div {
                style: "padding:10px 14px 14px; border-top:1px solid #1a1a22; flex-shrink:0;",

                if let Some(ref filename) = status_ok {
                    div {
                        style: "font-size:10px; color:#5a9a6a; letter-spacing:0.04em; margin-bottom:8px; line-height:1.6; word-break:break-all;",
                        "✓  {filename}"
                    }
                }
                if let Some(ref msg) = status_err {
                    div {
                        style: "font-size:10px; color:#9a4a4a; letter-spacing:0.04em; margin-bottom:8px; line-height:1.6; word-break:break-all;",
                        "✗  {msg}"
                    }
                }

                button {
                    style: "width:100%; padding:8px 0; background:#1e1e30; color:#9b8dd4; border:1px solid #2e2e46; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.1em; cursor:pointer; margin-bottom:6px; opacity:{btn_opacity};",
                    disabled: btn_disabled,
                    onclick: {
                        let img_path = entry.image_path.clone();
                        move |_| {
                            let path = img_path.clone();
                            let txt = text.read().clone();
                            generating.set(true);
                            result.set(None);
                            spawn(async move {
                                let r = tokio::task::spawn_blocking(move || {
                                    generate_blank_with_text(&path, &txt)
                                        .map_err(|e| e.to_string())
                                })
                                .await
                                .unwrap_or_else(|e| Err(e.to_string()));
                                result.set(Some(r));
                                generating.set(false);
                            });
                        }
                    },
                    "{btn_label}"
                }

                button {
                    style: "width:100%; padding:6px 0; background:transparent; color:#444; border:1px solid #1e1e28; border-radius:4px; font-family:inherit; font-size:10px; letter-spacing:0.1em; cursor:pointer; margin-bottom:6px;",
                    onclick: move |_| {
                        let _ = open_in_file_manager(&library_dir);
                    },
                    "Open Library Folder"
                }

                button {
                    style: "width:100%; padding:6px 0; background:transparent; color:#9a4a4a; border:1px solid #3a2020; border-radius:4px; font-family:inherit; font-size:10px; letter-spacing:0.1em; cursor:pointer;",
                    onclick: {
                        let path = entry.image_path.clone();
                        move |e: MouseEvent| {
                            let _ = std::fs::remove_file(&path);
                            on_delete.call(e);
                        }
                    },
                    "Delete Blank"
                }
            }
        }
    }
}

// ── Empty state ──────────────────────────────────────────────────────────────

#[allow(non_snake_case)]
fn EmptyBlanks() -> Element {
    let dir = blanks_dir().to_string_lossy().to_string();

    rsx! {
        div {
            style: "display:flex; flex-direction:column; align-items:center; justify-content:center; height:100%; gap:14px; color:#2e2e3a;",

            svg {
                width: "56", height: "56", view_box: "0 0 24 24",
                fill: "none", stroke: "currentColor", stroke_width: "1",
                stroke_linecap: "round", stroke_linejoin: "round",
                rect { x: "3", y: "3", width: "18", height: "18", rx: "2", ry: "2" }
                line { x1: "12", y1: "8", x2: "12", y2: "16" }
                line { x1: "8", y1: "12", x2: "16", y2: "12" }
            }

            span {
                style: "font-size:12px; letter-spacing:0.12em; text-transform:uppercase;",
                "No blank images"
            }
            div {
                style: "display:flex; flex-direction:column; align-items:center; gap:4px; max-width:280px; text-align:center;",
                span {
                    style: "font-size:11px; color:#2a2a40; letter-spacing:0.06em; line-height:1.7;",
                    "Use the \"+ Add Image\" button to add images, or place them in:"
                }
                span {
                    style: "font-size:10px; color:#3a3a58; letter-spacing:0.04em; word-break:break-all; line-height:1.6;",
                    "{dir}"
                }
            }
        }
    }
}

// ── OS file-manager helper ───────────────────────────────────────────────────

fn open_in_file_manager(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer").arg(path).spawn()?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(path).spawn()?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(path).spawn()?;
    Ok(())
}
