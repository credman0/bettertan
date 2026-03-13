use std::path::PathBuf;

use dioxus::prelude::*;

use crate::{image_to_data_url, storage::{self, LibraryEntry, update_ui_state}, Tab};

// ── Screen ────────────────────────────────────────────────────────────────────

#[allow(non_snake_case)]
pub fn LibraryView() -> Element {
    let restore_path = storage::load_ui_state().library_selected;
    let initial_entries = storage::load_all_entries();
    let restore_idx = restore_path
        .as_ref()
        .and_then(|p| initial_entries.iter().position(|e| &e.image_path == p));

    let mut entries: Signal<Vec<LibraryEntry>> = use_signal(|| initial_entries);
    let mut selected: Signal<Option<usize>>    = use_signal(|| restore_idx);

    let refresh = move |_| {
        entries.set(storage::load_all_entries());
        selected.set(None);
        let _ = update_ui_state(|s| s.library_selected = None);
    };

    rsx! {
        div {
            style: "display:flex; height:100%; overflow:hidden;",

            // ── Left: thumbnail grid ──────────────────────────────────────────
            div {
                style: "flex:1; display:flex; flex-direction:column; overflow:hidden;",

                // Toolbar
                div {
                    style: "display:flex; align-items:center; gap:12px; padding:9px 20px; border-bottom:1px solid #1e1e26; background:#13131a; flex-shrink:0;",
                    span {
                        style: "font-size:10px; color:#555; letter-spacing:0.12em; text-transform:uppercase;",
                        "{entries.read().len()} images"
                    }
                    div { style: "flex:1;" }
                    span { style: "font-size:10px; color:#2e2e3e; letter-spacing:0.08em;", "~/.image_tagger/" }
                    button {
                        style: "padding:6px 14px; background:#1a1a26; color:#888; border:1px solid #2a2a38; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.08em; cursor:pointer;",
                        onclick: refresh,
                        "↺  Refresh"
                    }
                }

                // Grid
                div {
                    style: "flex:1; overflow-y:auto; padding:20px;",
                    if entries.read().is_empty() {
                        EmptyLibrary {}
                    } else {
                        div {
                            style: "display:grid; grid-template-columns:repeat(auto-fill,minmax(170px,1fr)); gap:14px;",
                            for (i, entry) in entries.read().iter().enumerate() {
                                LibraryCard {
                                    key: "{i}",
                                    entry: entry.clone(),
                                    selected: *selected.read() == Some(i),
                                    onclick: move |_| {
                                        let new_sel = if *selected.read() == Some(i) { None } else { Some(i) };
                                        selected.set(new_sel);
                                        let path = new_sel.and_then(|i| entries.read().get(i).map(|e| e.image_path.clone()));
                                        let _ = update_ui_state(|s| s.library_selected = path);
                                    },
                                }
                            }
                        }
                    }
                }
            }

            // ── Right: detail panel ───────────────────────────────────────────
            if let Some(idx) = *selected.read() {
                if let Some(entry) = entries.read().get(idx).cloned() {
                    DetailPanel { entry }
                }
            }
        }
    }
}

// ── Library card ──────────────────────────────────────────────────────────────

#[component]
#[allow(non_snake_case)]
fn LibraryCard(
    entry: LibraryEntry,
    selected: bool,
    onclick: EventHandler<MouseEvent>,
) -> Element {
    // Load thumbnail asynchronously so the library render never blocks.
    let thumb_path = entry.image_path.clone();
    let thumb_res = use_resource(move || {
        let p = thumb_path.clone();
        async move {
            tokio::task::spawn_blocking(move || crate::image_to_thumbnail_url(&p, 200))
                .await
                .ok()
                .flatten()
        }
    });
    let data_url: Option<String> = thumb_res.read().as_ref().and_then(|v| v.clone());
    let name     = entry.image_file_name();
    let tag_count = entry.tags.len() + entry.custom_tags.len();
    let tag_label = format!("{} tag{}", tag_count, if tag_count == 1 { "" } else { "s" });

    let card_style = if selected {
        "background:#13131a; border:1px solid #5b8dee; border-radius:6px; overflow:hidden; cursor:pointer; transition:border-color 0.15s;"
    } else {
        "background:#13131a; border:1px solid #1e1e26; border-radius:6px; overflow:hidden; cursor:pointer; transition:border-color 0.15s;"
    };

    rsx! {
        div {
            style: "{card_style}",
            onclick: move |e| onclick.call(e),

            div {
                style: "width:100%; aspect-ratio:1/1; overflow:hidden; background:#0c0c0e;",
                if let Some(src) = data_url {
                    img { src: "{src}", style: "width:100%; height:100%; object-fit:cover; display:block;" }
                } else {
                    div {
                        style: "width:100%; height:100%; display:flex; align-items:center; justify-content:center; color:#2e2e3a; font-size:10px;",
                        "no preview"
                    }
                }
            }

            div {
                style: "padding:8px 10px;",
                div { style: "font-size:11px; color:#bbb; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; margin-bottom:3px;", "{name}" }
                div { style: "font-size:10px; color:#555; letter-spacing:0.05em;", "{tag_label}" }
            }
        }
    }
}

// ── Detail panel ──────────────────────────────────────────────────────────────

#[component]
#[allow(non_snake_case)]
fn DetailPanel(entry: LibraryEntry) -> Element {
    let mut pending_image = use_context::<Signal<Option<PathBuf>>>();
    let mut active_tab    = use_context::<Signal<Tab>>();

    let data_url  = image_to_data_url(&entry.image_path);
    let name      = entry.image_file_name();
    let idx_name  = format!("{}.idx", name);
    let path_str  = entry.image_path.to_string_lossy().to_string();

    let custom_csv: String = entry.custom_tags.join(", ");
    let model_csv: String  = entry.tags.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(", ");

    let open_path = entry.image_path.clone();

    rsx! {
        div {
            style: "width:300px; border-left:1px solid #1e1e26; display:flex; flex-direction:column; background:#0d0d10; flex-shrink:0;",

            // Thumbnail
            div {
                style: "width:100%; aspect-ratio:1/1; background:#0c0c0e; flex-shrink:0; overflow:hidden;",
                if let Some(src) = data_url {
                    img { src: "{src}", style: "width:100%; height:100%; object-fit:contain; display:block;" }
                }
            }

            // File info + Open in Tagger button
            div {
                style: "padding:11px 14px; border-bottom:1px solid #1a1a22; flex-shrink:0;",
                div { style: "font-size:12px; color:#ddd; margin-bottom:3px; word-break:break-all;", "{name}" }
                div { style: "font-size:10px; color:#3a3a50; letter-spacing:0.04em; margin-bottom:2px;", "{idx_name}" }
                div { style: "font-size:10px; color:#2a2a3a; letter-spacing:0.03em; word-break:break-all; margin-bottom:10px;", "{path_str}" }

                button {
                    style: "width:100%; padding:7px 0; background:#1e1e30; color:#9b8dd4; border:1px solid #2e2e46; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.08em; cursor:pointer;",
                    onclick: move |_| {
                        pending_image.set(Some(open_path.clone()));
                        *active_tab.write() = Tab::Tagger;
                    },
                    "Open in Tagger"
                }
            }

            // Tag + OCR display — scrollable
            div {
                style: "flex:1; overflow-y:auto; padding:12px 14px;",

                // Custom tags — shown first
                if !entry.custom_tags.is_empty() {
                    div {
                        style: "font-size:10px; letter-spacing:0.12em; text-transform:uppercase; color:#3a3a50; margin-bottom:6px;",
                        "Custom Tags"
                    }
                    div {
                        style: "font-size:12px; color:#9b8dd4; line-height:1.7; word-break:break-word; margin-bottom:14px;",
                        "{custom_csv}"
                    }
                }

                // Model tags
                if !entry.tags.is_empty() {
                    div {
                        style: "font-size:10px; letter-spacing:0.12em; text-transform:uppercase; color:#3a3a50; margin-bottom:6px;",
                        "Model Tags"
                    }
                    div {
                        style: "font-size:12px; color:#777; line-height:1.7; word-break:break-word; margin-bottom:14px;",
                        "{model_csv}"
                    }
                }

                // OCR text — shown when present
                if let Some(ref text) = entry.ocr_text {
                    div {
                        style: "font-size:10px; letter-spacing:0.12em; text-transform:uppercase; color:#3a4a5a; margin-bottom:6px;",
                        "OCR Text"
                    }
                    div {
                        style: "font-size:11px; color:#7a9abb; line-height:1.7; word-break:break-word; white-space:pre-wrap;",
                        "{text}"
                    }
                }

                if entry.tags.is_empty() && entry.custom_tags.is_empty() && entry.ocr_text.is_none() {
                    div {
                        style: "font-size:11px; color:#2e2e3a; text-align:center; padding-top:20px; letter-spacing:0.08em;",
                        "no tags recorded"
                    }
                }
            }
        }
    }
}

// ── Empty library state ───────────────────────────────────────────────────────

#[allow(non_snake_case)]
fn EmptyLibrary() -> Element {
    rsx! {
        div {
            style: "display:flex; flex-direction:column; align-items:center; justify-content:center; height:100%; gap:14px; color:#2e2e3a;",
            svg {
                width: "56", height: "56", view_box: "0 0 24 24",
                fill: "none", stroke: "currentColor", stroke_width: "1",
                stroke_linecap: "round", stroke_linejoin: "round",
                rect { x: "3", y: "3", width: "18", height: "18", rx: "2", ry: "2" }
                circle { cx: "8.5", cy: "8.5", r: "1.5" }
                polyline { points: "21 15 16 10 5 21" }
            }
            span { style: "font-size:12px; letter-spacing:0.12em; text-transform:uppercase;", "Library is empty" }
            span { style: "font-size:11px; color:#222; letter-spacing:0.06em;", "Tag and save images from the Tagger tab" }
        }
    }
}
