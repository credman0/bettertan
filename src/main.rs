#![allow(non_snake_case)]

mod tagger;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use dioxus::prelude::*;
use tagger::{TagOptions, TagOutput, Tagger};

// ── App entry-point ───────────────────────────────────────────────────────────

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            dioxus::desktop::Config::new()
                .with_window(
                    dioxus::desktop::WindowBuilder::new()
                        .with_title("Image Tagger")
                        .with_inner_size(dioxus::desktop::LogicalSize::new(960.0, 640.0))
                        .with_resizable(true),
                )
                .with_custom_head(r#"<style>
                    * { box-sizing: border-box; margin: 0; padding: 0; }
                    body { background: #0f0f11; color: #e8e6e3; font-family: 'SF Mono', 'Fira Code', monospace; }
                    ::-webkit-scrollbar { width: 6px; }
                    ::-webkit-scrollbar-track { background: transparent; }
                    ::-webkit-scrollbar-thumb { background: #333; border-radius: 3px; }
                </style>"#.into()),
        )
        .launch(App);
}

// ── Shared tagger state ───────────────────────────────────────────────────────

type SharedTagger = Arc<Mutex<Option<Tagger>>>;

fn init_tagger() -> SharedTagger {
    let shared: SharedTagger = Arc::new(Mutex::new(None));
    let clone = Arc::clone(&shared);
    std::thread::spawn(move || match Tagger::new() {
        Ok(t) => *clone.lock().unwrap() = Some(t),
        Err(e) => eprintln!("Failed to initialise tagger: {e}"),
    });
    shared
}

// ── Root component ────────────────────────────────────────────────────────────

fn App() -> Element {
    let tagger = use_context_provider(init_tagger);

    // UI state
    let mut image_src: Signal<Option<String>> = use_signal(|| None);
    let mut output: Signal<Option<Result<TagOutput, String>>> = use_signal(|| None);
    let mut is_loading = use_signal(|| false);
    let mut threshold = use_signal(|| 0.68_f32);
    let topk = use_signal(|| 30_usize);

    // Run inference whenever image_path changes
    let mut run_inference = move |path: PathBuf| {
        is_loading.set(true);
        output.set(None);
        let tagger_arc = Arc::clone(&tagger);
        let opts = TagOptions {
            threshold: *threshold.read(),
            topk: *topk.read(),
        };
        let path_str = path.to_string_lossy().to_string();
        // spawn_blocking runs the inference on a threadpool thread;
        // the surrounding `spawn` is a Dioxus async task that CAN access signals.
        spawn(async move {
            let result = tokio::task::spawn_blocking(move || {
                let mut guard = tagger_arc.lock().unwrap();
                match guard.as_mut() {
                    Some(t) => t.tag_image(&path_str, opts).map_err(|e| e.to_string()),
                    None => Err("Tagger still initialising — please try again shortly.".into()),
                }
            })
            .await
            .unwrap_or_else(|e| Err(e.to_string()));

            output.set(Some(result));
            is_loading.set(false);
        });
    };

    // File picker handler
    let open_file = move |_| {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["jpg", "jpeg", "png", "webp", "bmp", "gif", "tiff"])
            .pick_file()
        {
            if let Some(data_url) = image_to_data_url(&path) {
                image_src.set(Some(data_url));
            }
            run_inference(path);
        }
    };

    rsx! {
        div {
            style: "display:flex; flex-direction:column; height:100vh; background:#0f0f11;",

            // ── Top bar ───────────────────────────────────────────────────
            div {
                style: "display:flex; align-items:center; gap:16px; padding:14px 20px;
                        border-bottom:1px solid #222; background:#13131a; flex-shrink:0;",

                span {
                    style: "font-size:13px; letter-spacing:0.15em; color:#666; text-transform:uppercase;",
                    "Image Tagger"
                }

                div { style: "flex:1;" }

                // Threshold control
                label {
                    style: "font-size:11px; color:#555; letter-spacing:0.1em; text-transform:uppercase;",
                    "Threshold"
                }
                input {
                    r#type: "range",
                    min: "0.0", max: "1.0", step: "0.01",
                    value: "{threshold}",
                    style: "width:80px; accent-color:#5b8dee;",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<f32>() {
                            threshold.set(v);
                        }
                    }
                }
                span {
                    style: "font-size:11px; color:#888; width:36px; text-align:right;",
                    "{threshold:.2}"
                }

                // Open button
                button {
                    style: "padding:8px 18px; background:#5b8dee; color:#fff; border:none;
                            border-radius:4px; font-family:inherit; font-size:12px;
                            letter-spacing:0.08em; cursor:pointer; transition:background 0.15s;",
                    onclick: open_file,
                    "Open Image"
                }
            }

            // ── Content area ──────────────────────────────────────────────
            div {
                style: "display:flex; flex:1; overflow:hidden;",

                // Left: image preview
                div {
                    style: "width:50%; display:flex; align-items:center; justify-content:center;
                            background:#0c0c0e; border-right:1px solid #1e1e26; overflow:hidden;",

                    if let Some(src) = image_src.read().as_ref() {
                        img {
                            src: "{src}",
                            style: "max-width:100%; max-height:100%; object-fit:contain;
                                    display:block; padding:24px;"
                        }
                    } else {
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
                            span {
                                style: "font-size:12px; letter-spacing:0.12em; text-transform:uppercase;",
                                "No image selected"
                            }
                        }
                    }
                }

                // Right: tag results
                div {
                    style: "width:50%; display:flex; flex-direction:column; overflow:hidden;",

                    if *is_loading.read() {
                        div {
                            style: "flex:1; display:flex; align-items:center; justify-content:center;",
                            div {
                                style: "display:flex; flex-direction:column; align-items:center; gap:16px; color:#444;",
                                div {
                                    style: "width:32px; height:32px; border:2px solid #333;
                                            border-top-color:#5b8dee; border-radius:50%;
                                            animation: spin 0.8s linear infinite;",
                                }
                                span {
                                    style: "font-size:11px; letter-spacing:0.15em; text-transform:uppercase;",
                                    "Running inference…"
                                }
                            }
                        }
                    } else if let Some(result) = output.read().as_ref() {
                        match result {
                            Err(msg) => rsx! {
                                div {
                                    style: "flex:1; display:flex; align-items:center; justify-content:center; padding:32px;",
                                    div {
                                        style: "color:#c0392b; font-size:12px; line-height:1.6; text-align:center;",
                                        "⚠  {msg}"
                                    }
                                }
                            },
                            Ok(out) => rsx! {
                                TagPanel { output: out.clone() }
                            },
                        }
                    } else {
                        div {
                            style: "flex:1; display:flex; align-items:center; justify-content:center; color:#252530;",
                            span {
                                style: "font-size:11px; letter-spacing:0.12em; text-transform:uppercase;",
                                "Tags will appear here"
                            }
                        }
                    }
                }
            }
        }

        // Spinner keyframe (injected once)
        style {
            "@keyframes spin {{ from {{ transform: rotate(0deg); }} to {{ transform: rotate(360deg); }} }}"
        }
    }
}

// ── Tag panel ─────────────────────────────────────────────────────────────────

#[component]
fn TagPanel(output: TagOutput) -> Element {
    let mut show_topk = use_signal(|| false);

    let display_tags = if *show_topk.read() {
        &output.topk
    } else {
        &output.above_threshold
    };

    let max_score = display_tags
        .iter()
        .map(|t| t.score)
        .fold(0.0_f32, f32::max)
        .max(0.001);

    rsx! {
        // Tab bar
        div {
            style: "display:flex; border-bottom:1px solid #1e1e26; flex-shrink:0; padding:0 16px; gap:4px; background:#0f0f11;",

            TabButton {
                label: format!("Above threshold ({})", output.above_threshold.len()),
                active: !*show_topk.read(),
                onclick: move |_| show_topk.set(false),
            }
            TabButton {
                label: format!("Top {} by score", output.topk.len()),
                active: *show_topk.read(),
                onclick: move |_| show_topk.set(true),
            }
        }

        // Tag list
        div {
            style: "flex:1; overflow-y:auto; padding:8px 0;",

            if display_tags.is_empty() {
                div {
                    style: "padding:40px; text-align:center; color:#333; font-size:12px; letter-spacing:0.1em;",
                    "No tags above threshold"
                }
            } else {
                for tag in display_tags.iter() {
                    TagRow {
                        key: "{tag.tag}",
                        tag: tag.tag.clone(),
                        score: tag.score,
                        max_score,
                    }
                }
            }
        }
    }
}

// ── Tab button ────────────────────────────────────────────────────────────────

#[component]
fn TabButton(label: String, active: bool, onclick: EventHandler<MouseEvent>) -> Element {
    let border = if active { "border-bottom: 2px solid #5b8dee;" } else { "border-bottom: 2px solid transparent;" };
    let color = if active { "color:#e8e6e3;" } else { "color:#555;" };

    rsx! {
        button {
            style: "padding:10px 14px; background:none; border:none; {border} {color}
                    font-family:inherit; font-size:11px; letter-spacing:0.1em;
                    text-transform:uppercase; cursor:pointer; transition:color 0.15s;",
            onclick: move |e| onclick.call(e),
            "{label}"
        }
    }
}

// ── Individual tag row ────────────────────────────────────────────────────────

#[component]
fn TagRow(tag: String, score: f32, max_score: f32) -> Element {
    let bar_pct = (score / max_score * 100.0) as u32;
    let color = score_color(score);

    rsx! {
        div {
            style: "display:flex; align-items:center; gap:12px; padding:6px 20px;
                    transition:background 0.1s; cursor:default;",
            onmouseenter: |e| { e.stop_propagation(); },

            // Score badge
            span {
                style: "font-size:11px; color:{color}; width:44px; text-align:right; flex-shrink:0; letter-spacing:0.03em;",
                "{score:.3}"
            }

            // Bar
            div {
                style: "flex:1; height:3px; background:#1e1e26; border-radius:2px; overflow:hidden;",
                div {
                    style: "height:100%; width:{bar_pct}%; background:{color}; border-radius:2px; transition:width 0.3s ease;",
                }
            }

            // Tag name
            span {
                style: "font-size:12px; color:#ccc; min-width:140px; letter-spacing:0.02em;",
                "{tag}"
            }
        }
    }
}

fn score_color(score: f32) -> &'static str {
    if score >= 0.85 { "#5b8dee" }
    else if score >= 0.70 { "#7ecba1" }
    else if score >= 0.50 { "#d4a853" }
    else { "#8a6a6a" }
}

// ── Image helpers ─────────────────────────────────────────────────────────────

fn image_to_data_url(path: &PathBuf) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mime = match path.extension()?.to_str()?.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png"          => "image/png",
        "webp"         => "image/webp",
        "gif"          => "image/gif",
        "bmp"          => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        _              => "image/jpeg",
    };
    let encoded = BASE64.encode(&bytes);
    Some(format!("data:{mime};base64,{encoded}"))
}