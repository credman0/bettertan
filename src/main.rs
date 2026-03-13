#![allow(non_snake_case)]

mod library_view;
mod storage;
mod tagger;
mod tagger_view;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use dioxus::prelude::*;
use tagger::Tagger;

// ── Shared tagger handle ──────────────────────────────────────────────────────

/// The ONNX tagger wrapped in `Arc<Mutex<Option<…>>>` so it can be shared
/// across async tasks and the main thread.
pub type SharedTagger = Arc<Mutex<Option<Tagger>>>;

fn init_tagger() -> SharedTagger {
    let shared: SharedTagger = Arc::new(Mutex::new(None));
    let clone = Arc::clone(&shared);
    std::thread::spawn(move || match Tagger::new() {
        Ok(t) => *clone.lock().unwrap() = Some(t),
        Err(e) => eprintln!("Failed to initialise tagger: {e}"),
    });
    shared
}

// ── Navigation ────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum Tab {
    Tagger,
    Library,
}

// ── Root component ────────────────────────────────────────────────────────────

fn App() -> Element {
    // Tagger is initialised once and provided via context so child screens can
    // access it without prop-drilling.
    let _tagger = use_context_provider(init_tagger);

    let mut active_tab = use_signal(|| Tab::Tagger);

    rsx! {
        div {
            style: "display:flex; flex-direction:column; height:100vh; background:#0f0f11;",

            // ── Navigation bar ────────────────────────────────────────────────
            div {
                style: "display:flex; align-items:stretch; padding:0 20px;
                        border-bottom:1px solid #1e1e26; background:#13131a;
                        flex-shrink:0; height:44px;",

                // App wordmark
                div {
                    style: "display:flex; align-items:center; margin-right:28px;",
                    span {
                        style: "font-size:11px; letter-spacing:0.18em; color:#383848;
                                text-transform:uppercase; user-select:none;",
                        "Image Tagger"
                    }
                }

                // Tab buttons (full-height, active tab has bottom border)
                NavTab {
                    label: "Tagger",
                    active: *active_tab.read() == Tab::Tagger,
                    onclick: move |_| active_tab.set(Tab::Tagger),
                }
                NavTab {
                    label: "Library",
                    active: *active_tab.read() == Tab::Library,
                    onclick: move |_| active_tab.set(Tab::Library),
                }
            }

            // ── Screen content ────────────────────────────────────────────────
            div {
                style: "flex:1; overflow:hidden;",
                { match *active_tab.read() {
                    Tab::Tagger  => rsx! { tagger_view::TaggerView {} },
                    Tab::Library => rsx! { library_view::LibraryView {} },
                } }
            }
        }

        // Global keyframe animation (spinner used in TaggerView)
        // Note: {{ and }} are escaped braces inside rsx! string literals.
        style {
            "@keyframes spin {{ from {{ transform: rotate(0deg); }} to {{ transform: rotate(360deg); }} }}"
        }
    }
}

#[component]
fn NavTab(label: String, active: bool, onclick: EventHandler<MouseEvent>) -> Element {
    let border = if active {
        "border-bottom:2px solid #5b8dee;"
    } else {
        "border-bottom:2px solid transparent;"
    };
    let color = if active { "color:#e8e6e3;" } else { "color:#555;" };

    rsx! {
        button {
            style: "padding:0 18px; background:none; border:none; {border} {color}
                    font-family:inherit; font-size:11px; letter-spacing:0.12em;
                    text-transform:uppercase; cursor:pointer; transition:color 0.15s;
                    height:100%;",
            onclick: move |e| onclick.call(e),
            "{label}"
        }
    }
}

// ── Shared utility ─────────────────────────────────────────────────────────────

/// Reads `path` and returns a `data:` URL so the webview can display the image
/// without requiring `file://` access.
pub fn image_to_data_url(path: &PathBuf) -> Option<String> {
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
    Some(format!("data:{mime};base64,{}", BASE64.encode(&bytes)))
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            dioxus::desktop::Config::new()
                .with_window(
                    dioxus::desktop::WindowBuilder::new()
                        .with_title("Image Tagger")
                        .with_inner_size(dioxus::desktop::LogicalSize::new(1100.0, 720.0))
                        .with_resizable(true),
                )
                .with_custom_head(
                    r#"<style>
                        * { box-sizing: border-box; margin: 0; padding: 0; }
                        body {
                            background: #0f0f11;
                            color: #e8e6e3;
                            font-family: 'SF Mono', 'Fira Code', 'Cascadia Code', monospace;
                        }
                        ::-webkit-scrollbar { width: 6px; }
                        ::-webkit-scrollbar-track { background: transparent; }
                        ::-webkit-scrollbar-thumb { background: #2a2a36; border-radius: 3px; }
                        textarea:focus { border-color: #5b8dee !important; }
                        button:active { opacity: 0.8; }
                    </style>"#
                    .into(),
                ),
        )
        .launch(App);
}
