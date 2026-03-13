use std::path::PathBuf;
use std::sync::Arc;

use dioxus::prelude::*;
use dioxus::desktop::use_window;
use dioxus::desktop::wry::WebViewExtUnix;
use futures_util::StreamExt;
use gtk::prelude::*;

use crate::{
    image_to_data_url,
    storage,
    tagger::{TagOptions, TagOutput, TagResult},
    SharedOcr, SharedTagger,
};

// ── Screen ────────────────────────────────────────────────────────────────────

#[allow(non_snake_case)]
pub fn TaggerView() -> Element {
    let tagger = use_context::<SharedTagger>();
    let ocr    = use_context::<SharedOcr>();

    // Shared context signals written by LibraryView's "Open in Tagger" button.
    let mut pending_image = use_context::<Signal<Option<PathBuf>>>();

    // Local state
    let mut image_path: Signal<Option<PathBuf>> = use_signal(|| None);
    let mut image_src: Signal<Option<String>>   = use_signal(|| None);
    let mut raw_output: Signal<Option<Result<TagOutput, String>>> = use_signal(|| None);
    let mut is_loading  = use_signal(|| false);
    let mut threshold   = use_signal(|| 0.68_f32);

    // Custom tag chip state
    let mut tag_input: Signal<String>         = use_signal(String::new);
    let mut custom_tags: Signal<Vec<String>>  = use_signal(Vec::new);

    // OCR text extracted from the current image (None = not yet run / no text)
    let mut ocr_text: Signal<Option<String>>  = use_signal(|| None);

    // Save result notification
    let mut save_status: Signal<Option<Result<String, String>>> = use_signal(|| None);

    // ── Run inference + OCR ────────────────────────────────────────────────────
    let run_inference = use_coroutine(move |mut rx: UnboundedReceiver<PathBuf>| {
        let tagger = Arc::clone(&tagger);
        let ocr    = Arc::clone(&ocr);
        async move {
        while let Some(path) = rx.next().await {
            is_loading.set(true);
            raw_output.set(None);
            ocr_text.set(None);
            save_status.set(None);

            let tagger_arc = Arc::clone(&tagger);
            let ocr_arc    = Arc::clone(&ocr);
            let path_str   = path.to_string_lossy().to_string();
            let opts = TagOptions { threshold: 0.0, topk: 6000 };

            let (tag_result, ocr_result) =
                tokio::task::spawn_blocking(move || {
                    // Run tagging
                    let tags = {
                        let mut guard = tagger_arc.lock().unwrap();
                        match guard.as_mut() {
                            Some(t) => t.tag_image(&path_str, opts).map_err(|e| e.to_string()),
                            None    => Err("Tagger still initialising — please try again.".into()),
                        }
                    };

                    // Run OCR (best-effort; failure just means no text is stored)
                    let ocr = {
                        let mut guard = ocr_arc.lock().unwrap();
                        guard
                            .as_mut()
                            .and_then(|eng| eng.extract_text(&path_str).ok())
                    };

                    (tags, ocr)
                })
                .await
                .unwrap_or_else(|e| (Err(e.to_string()), None));

            // Update OCR signal
            ocr_text.set(ocr_result.clone());

            // Auto-save to library on successful tagging.
            if let Ok(ref output) = tag_result {
                let thresh = *threshold.read();
                let model_tags: Vec<(String, f32)> = output
                    .topk.iter()
                    .filter(|t| t.score >= thresh)
                    .map(|t| (t.tag.clone(), t.score))
                    .collect();
                let custom: Vec<String> = custom_tags.read().clone();
                let ocr: Option<String> = ocr_result.clone();
                match storage::save_or_update_entry(
                    &path,
                    &model_tags,
                    &custom,
                    ocr.as_deref(),
                ) {
                    Ok(dest) => {
                        image_path.set(Some(dest.clone()));
                        save_status.set(Some(Ok(format!("Saved → {}", dest.display()))));
                    }
                    Err(e) => save_status.set(Some(Err(e.to_string()))),
                }
            }

            raw_output.set(Some(tag_result));
            is_loading.set(false);
        }
    }});

    // ── Consume pending_image set by LibraryView ───────────────────────────────
    use_effect(move || {
        let maybe = pending_image.read().clone();
        if let Some(path) = maybe {
            image_src.set(image_to_data_url(&path));
            let _ = storage::update_ui_state(|s| s.tagger_image = Some(path.clone()));
            tag_input.set(String::new());
            save_status.set(None);
            raw_output.set(None);
            ocr_text.set(None);

            // Restore custom tags from the existing library entry, if present.
            let existing = storage::load_all_entries()
                .into_iter()
                .find(|e| e.image_path == path);

            if let Some(entry) = existing {
                custom_tags.set(entry.custom_tags.clone());
            } else {
                custom_tags.write().clear();
            }

            image_path.set(Some(path.clone()));
            run_inference.send(path);
            pending_image.set(None);
        }
    });

    // ── File picker ────────────────────────────────────────────────────────────
    let open_file = move |_| {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tiff"])
            .pick_file()
        {
            image_src.set(image_to_data_url(&path));
            let _ = storage::update_ui_state(|s| s.tagger_image = Some(path.clone()));
            image_path.set(Some(path.clone()));
            raw_output.set(None);
            ocr_text.set(None);
            save_status.set(None);
            custom_tags.write().clear();
            tag_input.set(String::new());
            run_inference.send(path);
        }
    };

    // ── Native GTK drag-drop handler (Linux: attach our own signal handlers) ─
    use_effect(move || {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
        let image_exts = ["jpg", "jpeg", "png", "webp", "bmp", "gif", "tiff"];

        // Wire the channel receiver → Dioxus signals
        spawn(async move {
            while let Some(path) = rx.recv().await {
                let ext = path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if image_exts.contains(&ext.as_str()) {
                    image_src.set(image_to_data_url(&path));
                    let _ = storage::update_ui_state(|s| s.tagger_image = Some(path.clone()));
                    image_path.set(Some(path.clone()));
                    raw_output.set(None);
                    ocr_text.set(None);
                    save_status.set(None);
                    custom_tags.write().clear();
                    tag_input.set(String::new());
                    run_inference.send(path);
                }
            }
        });

        // Attach GTK signal handlers to the underlying webkit2gtk WebView widget.
        // These fire reliably with real file paths on Linux XDnD drags.
        let desktop = use_window();
        let webkit_view: webkit2gtk::WebView = desktop.webview.webview();

        // collect drop paths in shared state between data_received and drop signals
        let pending: Arc<std::sync::Mutex<Option<Vec<PathBuf>>>> =
            Arc::new(std::sync::Mutex::new(None));

        {
            let pending = Arc::clone(&pending);
            webkit_view.connect_drag_data_received(move |_, _, _, _, data: &gtk::SelectionData, info: u32, _| {
                if info == 2 {
                    let paths: Vec<PathBuf> = data.uris()
                        .iter()
                        .map(|u| {
                            let s = u.as_str();
                            let s = s.strip_prefix("file://").unwrap_or(s);
                            let decoded = percent_encoding::percent_decode_str(s)
                                .decode_utf8_lossy()
                                .to_string();
                            PathBuf::from(decoded)
                        })
                        .collect();
                    *pending.lock().unwrap() = Some(paths);
                }
            });
        }

        {
            let pending = Arc::clone(&pending);
            webkit_view.connect_drag_drop(move |_, _, _, _, _| {
                if let Some(paths) = pending.lock().unwrap().take() {
                    for path in paths {
                        let _ = tx.send(path);
                    }
                }
                // Return false so WebKit also processes the drop (avoids visual glitches)
                false
            });
        }
    });

    // ── Global dragover visual indicator via JS ────────────────────────────────
    use_effect(move || {
        document::eval(r#"
            (function() {
                if (window.__bettertan_dragover_registered) return;
                window.__bettertan_dragover_registered = true;
                window.addEventListener('dragover', function(e) {
                    e.preventDefault();
                    e.stopPropagation();
                }, true);
            })();
        "#);
    });

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
        let ocr: Option<String> = ocr_text.read().clone();

        match storage::save_or_update_entry(&path, &model_tags, &custom, ocr.as_deref()) {
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
                            run_inference.send(p);
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

                // Right: tags + OCR + custom input
                div {
                    style: "width:50%; display:flex; flex-direction:column; overflow:hidden;",

                    // Tag results panel (scrollable, takes remaining space)
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

                    // OCR text panel — only shown when text is present
                    if let Some(ref text) = *ocr_text.read() {
                        div {
                            style: "border-top:1px solid #1e1e26; padding:10px 16px; flex-shrink:0; background:#0b0b0e; max-height:130px; display:flex; flex-direction:column;",

                            div {
                                style: "font-size:10px; letter-spacing:0.12em; text-transform:uppercase; color:#3a4a5a; margin-bottom:6px; flex-shrink:0;",
                                "OCR Text"
                            }
                            div {
                                style: "overflow-y:auto; flex:1;",
                                p {
                                    style: "font-size:11px; color:#7a9abb; line-height:1.6; word-break:break-word; white-space:pre-wrap;",
                                    "{text}"
                                }
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
                                            if let Some(path) = image_path.read().clone() {
                                                if let Some(Ok(ref output)) = *raw_output.read() {
                                                    let thresh = *threshold.read();
                                                    let model_tags: Vec<(String, f32)> = output
                                                        .topk.iter()
                                                        .filter(|t| t.score >= thresh)
                                                        .map(|t| (t.tag.clone(), t.score))
                                                        .collect();
                                                    let custom: Vec<String> = custom_tags.read().clone();
                                                    let ocr: Option<String> = ocr_text.read().clone();
                                                    match storage::save_or_update_entry(
                                                        &path,
                                                        &model_tags,
                                                        &custom,
                                                        ocr.as_deref(),
                                                    ) {
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
                                            let ocr: Option<String> = ocr_text.read().clone();
                                            match storage::save_or_update_entry(
                                                &path,
                                                &model_tags,
                                                &custom,
                                                ocr.as_deref(),
                                            ) {
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

                        // Popup overlay for duplicate-name errors
                        if let Some(err_msg) = &*save_status.read() {
                            if let Err(msg) = err_msg {
                                if msg.contains("already exists") {
                                    div {
                                        style: "position:fixed; top:0; left:0; right:0; bottom:0; background:rgba(0,0,0,0.6); display:flex; align-items:center; justify-content:center; z-index:1000;",
                                        onclick: move |_| save_status.set(None),
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
                                                onclick: move |_| save_status.set(None),
                                                "OK"
                                            }
                                        }
                                    }
                                }
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
