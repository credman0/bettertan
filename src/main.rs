#![allow(non_snake_case)]

mod library_view;
mod meme_storage;
mod meme_view;
mod ocr;
mod storage;
mod tagger;
mod tagger_view;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use dioxus::prelude::*;
use tagger::Tagger;
use ocr::OcrEngine;

// ── Shared tagger handle ──────────────────────────────────────────────────────

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

// ── Shared OCR handle ─────────────────────────────────────────────────────────

pub type SharedOcr = Arc<Mutex<Option<OcrEngine>>>;

fn init_ocr() -> SharedOcr {
    let shared: SharedOcr = Arc::new(Mutex::new(None));
    let clone = Arc::clone(&shared);
    std::thread::spawn(move || match OcrEngine::new() {
        Ok(engine) => *clone.lock().unwrap() = Some(engine),
        Err(e) => eprintln!("OCR not available: {e}"),
    });
    shared
}

// ── Navigation ────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub enum Tab {
    Tagger,
    Library,
    Meme,
}

// ── Root component ────────────────────────────────────────────────────────────

fn App() -> Element {
    let _tagger = use_context_provider(init_tagger);
    let _ocr    = use_context_provider(init_ocr);

    let mut active_tab: Signal<Tab> = use_context_provider(|| {
        let tab = storage::load_ui_state().active_tab;
        Signal::new(match tab.as_str() {
            "library" => Tab::Library,
            "meme"    => Tab::Meme,
            _         => Tab::Tagger,
        })
    });

    let _pending: Signal<Option<PathBuf>> =
        use_context_provider(|| Signal::new(storage::load_ui_state().tagger_image));

    rsx! {
        div {
            style: "display:flex; flex-direction:column; height:100vh; background:#0f0f11;",

            // ── Nav bar ───────────────────────────────────────────────────────
            div {
                style: "display:flex; align-items:stretch; padding:0 20px; \
                        border-bottom:1px solid #1e1e26; background:#13131a; \
                        flex-shrink:0; height:44px;",

                div {
                    style: "display:flex; align-items:center; margin-right:28px;",
                    span {
                        style: "font-size:11px; letter-spacing:0.18em; color:#383848; \
                                text-transform:uppercase; user-select:none;",
                        "Image Tagger"
                    }
                }

                NavTab {
                    label: "Tagger",
                    active: *active_tab.read() == Tab::Tagger,
                    onclick: move |_| {
                        *active_tab.write() = Tab::Tagger;
                        let _ = storage::update_ui_state(|s| s.active_tab = "tagger".into());
                    },
                }
                NavTab {
                    label: "Library",
                    active: *active_tab.read() == Tab::Library,
                    onclick: move |_| {
                        *active_tab.write() = Tab::Library;
                        let _ = storage::update_ui_state(|s| s.active_tab = "library".into());
                    },
                }
                NavTab {
                    label: "Memes",
                    active: *active_tab.read() == Tab::Meme,
                    onclick: move |_| {
                        *active_tab.write() = Tab::Meme;
                        let _ = storage::update_ui_state(|s| s.active_tab = "meme".into());
                    },
                }
            }

            // ── Content ───────────────────────────────────────────────────────
            div {
                style: "flex:1; overflow:hidden;",
                { match *active_tab.read() {
                    Tab::Tagger  => rsx! { tagger_view::TaggerView {} },
                    Tab::Library => rsx! { library_view::LibraryView {} },
                    Tab::Meme    => rsx! { meme_view::MemeView {} },
                } }
            }
        }

        style {
            "@keyframes spin {{ from {{ transform: rotate(0deg); }} to {{ transform: rotate(360deg); }} }}"
        }
    }
}

// ── Nav tab ───────────────────────────────────────────────────────────────────

#[component]
fn NavTab(label: String, active: bool, onclick: EventHandler<MouseEvent>) -> Element {
    let s = if active {
        "padding:0 18px; background:none; border:none; border-bottom:2px solid #5b8dee; \
         color:#e8e6e3; font-family:inherit; font-size:11px; letter-spacing:0.12em; \
         text-transform:uppercase; cursor:pointer; height:100%;"
    } else {
        "padding:0 18px; background:none; border:none; border-bottom:2px solid transparent; \
         color:#555; font-family:inherit; font-size:11px; letter-spacing:0.12em; \
         text-transform:uppercase; cursor:pointer; height:100%;"
    };
    rsx! {
        button { style: "{s}", onclick: move |e| onclick.call(e), "{label}" }
    }
}

// ── Shared utility ────────────────────────────────────────────────────────────

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

/// Like `image_to_data_url` but resizes to a small thumbnail first.
/// Use this for grid cards to avoid loading full-res images into memory.
pub fn image_to_thumbnail_url(path: &PathBuf, max_px: u32) -> Option<String> {
    let img = image::open(path).ok()?;
    let thumb = img.thumbnail(max_px, max_px);
    let mut buf = Vec::new();
    thumb
        .write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Jpeg,
        )
        .ok()?;
    Some(format!("data:image/jpeg;base64,{}", BASE64.encode(&buf)))
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    // Disable WebKit GPU compositing to avoid Wayland protocol errors.
    unsafe { std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1"); }
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
                        body { background:#0f0f11; color:#e8e6e3; font-family:'SF Mono','Fira Code','Cascadia Code',monospace; }
                        ::-webkit-scrollbar { width:6px; }
                        ::-webkit-scrollbar-track { background:transparent; }
                        ::-webkit-scrollbar-thumb { background:#2a2a36; border-radius:3px; }
                        input:focus, textarea:focus { border-color:#5b8dee !important; outline:none; }
                        button:active { opacity:0.8; }
                    </style>"#
                    .into(),
                ),
        )
        .launch(App);
}
