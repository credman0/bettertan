use std::path::PathBuf;
use std::sync::Arc;

use dioxus::prelude::*;

use crate::{
    image_to_data_url,
    storage,
    tagger::{TagOptions, TagOutput, TagResult},
    SharedTagger, Tab,
};

// ── Screen ────────────────────────────────────────────────────────────────────

#[allow(non_snake_case)]
pub fn TaggerView() -> Element {
    let tagger = use_context::<SharedTagger>();

    // Shared context signals written by LibraryView's "Open in Tagger" button.
    let mut pending_image = use_context::<Signal<Option<PathBuf>>>();

    // Local state
    let mut image_path: Signal<Option<PathBuf>> = use_signal(|| None);
    let mut image_src: Signal<Option<String>>   = use_signal(|| None);
    let mut raw_output: Signal<Option<Result<TagOutput, String>>> = use_signal(|| None);
    let mut is_loading  = use_signal(|| false);
    let mut threshold   = use_signal(|| 0.68_f32);

    // Custom tag chip state
    let mut tag_input: Signal<String>    = use_signal(String::new);
    let mut custom_tags: Signal<Vec<String>> = use_signal(Vec::new);

    // Save result notification
    let mut save_status: Signal<Option<Result<String, String>>> = use_signal(|| None);

    // ── Consume pending_image set by LibraryView ───────────────────────────────
    // Runs whenever `pending_image` changes. Loads the image display and
    // restores any previously saved tags. Does NOT trigger new inference —
    // the user must press "Run Tagger" explicitly.
    use_effect(move || {
        let maybe = pending_image.read().clone();
        if let Some(path) = maybe {
            image_src.set(image_to_data_url(&path));
            tag_input.set(String::new());
            save_status.set(None);

            // Restore tags from the existing library entry, if present.
            let existing = storage::load_all_entries()
                .into_iter()
                .find(|e| e.image_path == path);

            if let Some(entry) = existing {
                let tag_results: Vec<TagResult> = entry
                    .tags
                    .iter()
                    .map(|(t, s)| TagResult { tag: t.clone(), score: *s })
                    .collect();
                let output = crate::tagger::TagOutput {
                    above_threshold: tag_results.clone(),
                    topk: tag_results,
                };
                raw_output.set(Some(Ok(output)));
                custom_tags.set(entry.custom_tags.clone());
            } else {
                raw_output.set(None);
                custom_tags.write().clear();
            }

            image_path.set(Some(path));
            // Consume — clear so re-navigating to Tagger doesn't reload.
            pending_image.set(None);
        }
    });

    // ── Run inference ──────────────────────────────────────────────────────────
    let mut run_inference = move |path: PathBuf| {
        is_loading.set(true);
        raw_output.set(None);
        save_status.set(None);

        let tagger_arc = Arc::clone(&tagger);
        let path_str   = path.to_string_lossy().to_string();
        let opts = TagOptions { threshold: 0.0, topk: 6000 };

        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let mut guard = tagger_arc.lock().unwrap();
                match guard.as_mut() {
                    Some(t) => t.tag_image(&path_str, opts).map_err(|e| e.to_string()),
                    None    => Err("Tagger still initialising — please try again.".into()),
                }
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));

            // Auto-save to library on success.
            if let Ok(ref output) = result {
                let thresh = *threshold.read();
                let model_tags: Vec<(String, f32)> = output
                    .topk.iter()
                    .filter(|t| t.score >= thresh)
                    .map(|t| (t.tag.clone(), t.score))
                    .collect();
                let custom: Vec<String> = custom_tags.read().clone();
                match storage::save_or_update_entry(&path, &model_tags, &custom) {
                    Ok(dest) => {
                        // Point image_path at the data-dir copy so future saves
                        // update in-place rather than duplicating the file.
                        image_path.set(Some(dest.clone()));
                        save_status.set(Some(Ok(format!("Saved → {}", dest.display()))));
                    }
                    Err(e) => save_status.set(Some(Err(e.to_string()))),
                }
            }

            raw_output.set(Some(result));
            is_loading.set(false);
        });
    };

    // ── File picker ────────────────────────────────────────────────────────────
    let open_file = move |_| {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tiff"])
            .pick_file()
        {
            image_src.set(image_to_data_url(&path));
            image_path.set(Some(path));
            raw_output.set(None);
            save_status.set(None);
            custom_tags.write().clear();
            tag_input.set(String::new());
        }
    };

    // ── Save to library ────────────────────────────────────────────────────────
    let save_entry = move |_| {
        let Some(path) = image_path.read().clone() else { return };
        let Some(Ok(output)) = raw_output.read().clone() else { return };

        let thresh = *threshold.read();
        let model_tags: Vec<(String, f32)> = output
            .topk.iter()
            .filter(|t| t.score >= thresh)
            .map(|t| (t.tag.clone(), t.score))
            .collect();

        let custom: Vec<String> = custom_tags.read().clone();

        match storage::save_entry(&path, &model_tags, &custom) {
            Ok(dest) => save_status.set(Some(Ok(format!("Saved → {}", dest.display())))),
            Err(e)   => save_status.set(Some(Err(e.to_string()))),
        }
    };

    // ── Derived flags ──────────────────────────────────────────────────────────
    let has_image  = image_path.read().is_some();
    let can_run    = has_image && !*is_loading.read();
    let can_save   = has_image && raw_output.read().as_ref().map_or(false, |r| r.is_ok());

    let run_style = if can_run {
        "padding:7px 16px; background:#1a1a26; color:#aaa; border:1px solid #2a2a38; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.08em; cursor:pointer;"
    } else {
        "padding:7px 16px; background:#13131a; color:#333; border:1px solid #1e1e26; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.08em; cursor:not-allowed;"
    };
    let save_style = if can_save {
        "padding:7px 16px; background:#5b8dee; color:#fff; border:none; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.08em; cursor:pointer;"
    } else {
        "padding:7px 16px; background:#13131a; color:#333; border:1px solid #1e1e26; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.08em; cursor:not-allowed;"
    };

    rsx! {
        div {
            style: "display:flex; flex-direction:column; height:100%; overflow:hidden;",

            // ── Controls bar ──────────────────────────────────────────────────
            div {
                style: "display:flex; align-items:center; gap:14px; padding:9px 20px; border-bottom:1px solid #1e1e26; background:#13131a; flex-shrink:0;",

                div { style: "flex:1;" }

                label {
                    style: "font-size:10px; color:#555; letter-spacing:0.12em; text-transform:uppercase;",
                    "Threshold"
                }
                input {
                    r#type: "range", min: "0.0", max: "1.0", step: "0.01",
                    value: "{threshold}",
                    style: "width:90px; accent-color:#5b8dee; cursor:pointer;",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<f32>() { threshold.set(v); }
                    },
                }
                span {
                    style: "font-size:11px; color:#888; width:34px; font-variant-numeric:tabular-nums;",
                    "{threshold:.2}"
                }

                button {
                    style: "padding:7px 16px; background:#1a1a26; color:#999; border:1px solid #2a2a38; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.08em; cursor:pointer;",
                    onclick: open_file,
                    "Open Image"
                }

                button {
                    style: "{run_style}",
                    disabled: !can_run,
                    onclick: move |_| {
                        if let Some(p) = image_path.read().clone() {
                            run_inference(p);
                        }
                    },
                    "Run Tagger"
                }

                button {
                    style: "{save_style}",
                    disabled: !can_save,
                    onclick: save_entry,
                    "Save to Library"
                }
            }

            // ── Split content ─────────────────────────────────────────────────
            div {
                style: "display:flex; flex:1; overflow:hidden;",

                // Left: image preview
                div {
                    style: "width:50%; display:flex; align-items:center; justify-content:center; background:#0c0c0e; border-right:1px solid #1e1e26; overflow:hidden;",
                    if let Some(src) = image_src.read().as_ref() {
                        img {
                            src: "{src}",
                            style: "max-width:100%; max-height:100%; object-fit:contain; padding:24px;"
                        }
                    } else {
                        EmptyImagePlaceholder {}
                    }
                }

                // Right: tags + custom input
                div {
                    style: "width:50%; display:flex; flex-direction:column; overflow:hidden;",

                    // Tag results panel
                    div {
                        style: "flex:1; display:flex; flex-direction:column; overflow:hidden; min-height:0;",

                        if *is_loading.read() {
                            LoadingSpinner {}
                        } else if let Some(result) = raw_output.read().as_ref() {
                            match result {
                                Err(msg) => rsx! {
                                    div {
                                        style: "flex:1; display:flex; align-items:center; justify-content:center; padding:32px;",
                                        span { style: "color:#c0392b; font-size:12px; line-height:1.6; text-align:center;", "⚠  {msg}" }
                                    }
                                },
                                Ok(output) => rsx! {
                                    TagPanel { output: output.clone(), threshold: *threshold.read() }
                                },
                            }
                        } else if has_image {
                            div {
                                style: "flex:1; display:flex; flex-direction:column; align-items:center; justify-content:center; gap:14px; color:#2a2a3a;",
                                span { style: "font-size:11px; letter-spacing:0.12em; text-transform:uppercase;", "Image loaded" }
                                span { style: "font-size:11px; color:#3a3a4a; letter-spacing:0.08em;", "Press Run Tagger to analyse" }
                            }
                        } else {
                            div {
                                style: "flex:1; display:flex; align-items:center; justify-content:center; color:#252530;",
                                span { style: "font-size:11px; letter-spacing:0.12em; text-transform:uppercase;", "Tags will appear here" }
                            }
                        }
                    }

                    // Custom tags footer
                    div {
                        style: "border-top:1px solid #1e1e26; padding:12px 16px; flex-shrink:0; background:#0d0d10;",

                        div {
                            style: "font-size:10px; letter-spacing:0.12em; text-transform:uppercase; color:#555; margin-bottom:8px;",
                            "Custom Tags"
                        }

                        // Chip list
                        if !custom_tags.read().is_empty() {
                            div {
                                style: "display:flex; flex-wrap:wrap; gap:6px; margin-bottom:8px;",
                                for (i, tag) in custom_tags.read().iter().enumerate() {
                                    TagChip {
                                        key: "{i}-{tag}",
                                        label: tag.clone(),
                                        on_remove: move |_| {
                                            custom_tags.write().remove(i);
                                            // Auto-save the updated tag list.
                                            if let Some(path) = image_path.read().clone() {
                                                if let Some(Ok(ref output)) = *raw_output.read() {
                                                    let thresh = *threshold.read();
                                                    let model_tags: Vec<(String, f32)> = output
                                                        .topk.iter()
                                                        .filter(|t| t.score >= thresh)
                                                        .map(|t| (t.tag.clone(), t.score))
                                                        .collect();
                                                    let custom: Vec<String> = custom_tags.read().clone();
                                                    match storage::save_or_update_entry(&path, &model_tags, &custom) {
                                                        Ok(dest) => save_status.set(Some(Ok(format!("Saved → {}", dest.display())))),
                                                        Err(e)   => save_status.set(Some(Err(e.to_string()))),
                                                    }
                                                }
                                            }
                                        },
                                    }
                                }
                            }
                        }

                        // Input — Enter commits a chip
                        input {
                            r#type: "text",
                            style: "width:100%; background:#13131a; border:1px solid #2a2a38; border-radius:4px; color:#ccc; font-family:inherit; font-size:12px; padding:6px 10px; transition:border-color 0.15s;",
                            placeholder: "Type a tag and press Enter…",
                            value: "{tag_input}",
                            oninput: move |e| tag_input.set(e.value()),
                            onkeydown: move |e| {
                                if e.key() == Key::Enter {
                                    let t = tag_input.read().trim().to_owned();
                                    if !t.is_empty() && !custom_tags.read().contains(&t) {
                                        custom_tags.write().push(t);
                                    }
                                    tag_input.set(String::new());
                                    // Auto-save whenever we have inference results.
                                    if let Some(path) = image_path.read().clone() {
                                        if let Some(Ok(ref output)) = *raw_output.read() {
                                            let thresh = *threshold.read();
                                            let model_tags: Vec<(String, f32)> = output
                                                .topk.iter()
                                                .filter(|t| t.score >= thresh)
                                                .map(|t| (t.tag.clone(), t.score))
                                                .collect();
                                            let custom: Vec<String> = custom_tags.read().clone();
                                            match storage::save_or_update_entry(&path, &model_tags, &custom) {
                                                Ok(dest) => save_status.set(Some(Ok(format!("Saved → {}", dest.display())))),
                                                Err(e)   => save_status.set(Some(Err(e.to_string()))),
                                            }
                                        }
                                    }
                                }
                            },
                        }

                        // Save status
                        if let Some(status) = save_status.read().as_ref() {
                            match status {
                                Ok(msg)  => rsx! { div { style: "margin-top:7px; font-size:11px; color:#7ecba1;", "✓  {msg}" } },
                                Err(msg) => rsx! { div { style: "margin-top:7px; font-size:11px; color:#c0392b;", "✗  {msg}" } },
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Custom tag chip ───────────────────────────────────────────────────────────

#[component]
fn TagChip(label: String, on_remove: EventHandler<MouseEvent>) -> Element {
    rsx! {
        div {
            style: "display:inline-flex; align-items:center; gap:5px; padding:3px 8px 3px 10px; background:#1e1e30; border:1px solid #2e2e46; border-radius:20px;",
            span { style: "font-size:11px; color:#9b8dd4; letter-spacing:0.04em;", "{label}" }
            button {
                style: "background:none; border:none; color:#4a4a6a; cursor:pointer; font-size:13px; line-height:1; padding:0; display:flex; align-items:center;",
                onclick: move |e| on_remove.call(e),
                "×"
            }
        }
    }
}

// ── Tag panel (tabs: above-threshold / top-k) ─────────────────────────────────

#[component]
fn TagPanel(output: TagOutput, threshold: f32) -> Element {
    let mut show_topk = use_signal(|| false);

    let above_threshold: Vec<TagResult> = output
        .topk.iter()
        .filter(|t| t.score >= threshold)
        .cloned()
        .collect();

    let display_tags: Vec<TagResult> =
        if *show_topk.read() { output.topk.clone() } else { above_threshold.clone() };

    let max_score = display_tags.iter().map(|t| t.score).fold(0.001_f32, f32::max);

    let thresh_label = format!("≥ {:.2}  ({})", threshold, above_threshold.len());
    let topk_label   = format!("Top {}  by score", output.topk.len());

    rsx! {
        // Tab bar
        div {
            style: "display:flex; border-bottom:1px solid #1e1e26; flex-shrink:0; padding:0 12px; background:#0f0f11;",
            TabButton { label: thresh_label, active: !*show_topk.read(), onclick: move |_| show_topk.set(false) }
            TabButton { label: topk_label,   active: *show_topk.read(),  onclick: move |_| show_topk.set(true)  }
        }

        // Tag list
        div {
            style: "flex:1; overflow-y:auto; padding:4px 0;",
            if display_tags.is_empty() {
                div {
                    style: "padding:40px; text-align:center; color:#333; font-size:12px; letter-spacing:0.1em;",
                    "No tags above threshold"
                }
            } else {
                for tag in display_tags.iter() {
                    TagRow { key: "{tag.tag}", tag: tag.tag.clone(), score: tag.score, max_score }
                }
            }
        }
    }
}

// ── Sub-components ────────────────────────────────────────────────────────────

#[component]
pub fn TabButton(label: String, active: bool, onclick: EventHandler<MouseEvent>) -> Element {
    let s = if active {
        "padding:9px 12px; background:none; border:none; border-bottom:2px solid #5b8dee; color:#e8e6e3; font-family:inherit; font-size:10px; letter-spacing:0.1em; text-transform:uppercase; cursor:pointer;"
    } else {
        "padding:9px 12px; background:none; border:none; border-bottom:2px solid transparent; color:#555; font-family:inherit; font-size:10px; letter-spacing:0.1em; text-transform:uppercase; cursor:pointer;"
    };
    rsx! {
        button { style: "{s}", onclick: move |e| onclick.call(e), "{label}" }
    }
}

#[component]
fn TagRow(tag: String, score: f32, max_score: f32) -> Element {
    let bar_pct = (score / max_score * 100.0) as u32;
    let color   = score_color(score);
    rsx! {
        div {
            style: "display:flex; align-items:center; gap:12px; padding:5px 16px;",
            span { style: "font-size:11px; color:{color}; width:44px; text-align:right; flex-shrink:0; font-variant-numeric:tabular-nums;", "{score:.3}" }
            div {
                style: "flex:1; height:3px; background:#1e1e26; border-radius:2px; overflow:hidden;",
                div { style: "height:100%; width:{bar_pct}%; background:{color}; border-radius:2px;" }
            }
            span { style: "font-size:12px; color:#ccc; min-width:140px; letter-spacing:0.02em;", "{tag}" }
        }
    }
}

#[allow(non_snake_case)]
fn EmptyImagePlaceholder() -> Element {
    rsx! {
        div {
            style: "display:flex; flex-direction:column; align-items:center; gap:12px; color:#2e2e3a;",
            svg {
                width: "64", height: "64", view_box: "0 0 24 24",
                fill: "none", stroke: "currentColor", stroke_width: "1",
                stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }
                polyline { points: "17 8 12 3 7 8" }
                line { x1: "12", y1: "3", x2: "12", y2: "15" }
            }
            span { style: "font-size:12px; letter-spacing:0.12em; text-transform:uppercase;", "No image selected" }
        }
    }
}

#[allow(non_snake_case)]
fn LoadingSpinner() -> Element {
    rsx! {
        div {
            style: "flex:1; display:flex; align-items:center; justify-content:center;",
            div {
                style: "display:flex; flex-direction:column; align-items:center; gap:16px; color:#444;",
                div { style: "width:28px; height:28px; border:2px solid #282830; border-top-color:#5b8dee; border-radius:50%; animation:spin 0.8s linear infinite;" }
                span { style: "font-size:11px; letter-spacing:0.15em; text-transform:uppercase;", "Running inference…" }
            }
        }
    }
}

// ── Utilities re-exported for library_view ────────────────────────────────────

pub fn score_color(score: f32) -> &'static str {
    if score >= 0.85      { "#5b8dee" }
    else if score >= 0.70 { "#7ecba1" }
    else if score >= 0.50 { "#d4a853" }
    else                  { "#8a6a6a" }
}
