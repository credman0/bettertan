use dioxus::prelude::*;

use crate::{image_to_data_url, storage::{self, LibraryEntry}, tagger_view::score_color};

// ── Screen ────────────────────────────────────────────────────────────────────

#[allow(non_snake_case)]
pub fn LibraryView() -> Element {
    // Load entries synchronously on first render; user can refresh manually.
    let mut entries: Signal<Vec<LibraryEntry>> =
        use_signal(|| storage::load_all_entries());

    // Index of the currently selected entry (for the detail panel).
    let mut selected: Signal<Option<usize>> = use_signal(|| None);

    let refresh = move |_| {
        entries.set(storage::load_all_entries());
        selected.set(None);
    };

    rsx! {
        div {
            style: "display:flex; height:100%; overflow:hidden;",

            // ── Left: grid panel ──────────────────────────────────────────────
            div {
                style: "flex:1; display:flex; flex-direction:column; overflow:hidden;",

                // Toolbar
                div {
                    style: "display:flex; align-items:center; gap:12px; padding:9px 20px;
                            border-bottom:1px solid #1e1e26; background:#13131a; flex-shrink:0;",

                    span {
                        style: "font-size:10px; color:#555; letter-spacing:0.12em; text-transform:uppercase;",
                        "{entries.read().len()} images"
                    }
                    div { style: "flex:1;" }

                    span {
                        style: "font-size:10px; color:#3a3a4a; letter-spacing:0.08em;",
                        "~/.image_tagger/"
                    }

                    button {
                        style: "padding:6px 14px; background:#1a1a26; color:#888;
                                border:1px solid #2a2a38; border-radius:4px;
                                font-family:inherit; font-size:11px;
                                letter-spacing:0.08em; cursor:pointer;",
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
                            style: "display:grid;
                                    grid-template-columns:repeat(auto-fill,minmax(170px,1fr));
                                    gap:14px;",
                            for (i, entry) in entries.read().iter().enumerate() {
                                LibraryCard {
                                    key: "{i}-{entry.image_file_name()}",
                                    entry: entry.clone(),
                                    selected: *selected.read() == Some(i),
                                    onclick: move |_| {
                                        selected.set(
                                            if *selected.read() == Some(i) { None }
                                            else { Some(i) }
                                        );
                                    },
                                }
                            }
                        }
                    }
                }
            }

            // ── Right: detail panel (conditionally rendered) ──────────────────
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
    // Load the thumbnail as a data-URL (desktop webview may block file://).
    let data_url = image_to_data_url(&entry.image_path);
    let name = entry.image_file_name();
    let tag_count = entry.tags.len() + entry.custom_tags.len();
    let tag_label = format!("{} tag{}", tag_count, if tag_count == 1 { "" } else { "s" });
    // Pre-compute the style string so {…} interpolation doesn't confuse rsx!.
    let card_style = if selected {
        "background:#13131a; border:1px solid #5b8dee; box-shadow:0 0 0 1px #5b8dee22; border-radius:6px; overflow:hidden; cursor:pointer; transition:border-color 0.15s;".to_string()
    } else {
        "background:#13131a; border:1px solid #1e1e26; border-radius:6px; overflow:hidden; cursor:pointer; transition:border-color 0.15s;".to_string()
    };

    rsx! {
        div {
            style: "{card_style}",
            onclick: move |e| onclick.call(e),

            // Square thumbnail
            div {
                style: "width:100%; aspect-ratio:1/1; overflow:hidden; background:#0c0c0e;",
                if let Some(src) = data_url {
                    img {
                        src: "{src}",
                        style: "width:100%; height:100%; object-fit:cover; display:block;",
                    }
                } else {
                    div {
                        style: "width:100%; height:100%; display:flex; align-items:center;
                                justify-content:center; color:#2e2e3a; font-size:10px;",
                        "no preview"
                    }
                }
            }

            // Name + tag count
            div {
                style: "padding:8px 10px;",
                div {
                    style: "font-size:11px; color:#bbb; white-space:nowrap;
                            overflow:hidden; text-overflow:ellipsis; margin-bottom:3px;",
                    "{name}"
                }
                div {
                    style: "font-size:10px; color:#555; letter-spacing:0.05em;",
                    "{tag_label}"
                }
            }
        }
    }
}

// ── Detail panel ──────────────────────────────────────────────────────────────

#[component]
#[allow(non_snake_case)]
fn DetailPanel(entry: LibraryEntry) -> Element {
    let data_url = image_to_data_url(&entry.image_path);
    let name = entry.image_file_name();
    let idx_name = format!("{}.idx", name);
    let path_str = entry.image_path.to_string_lossy().to_string();

    rsx! {
        div {
            style: "width:300px; border-left:1px solid #1e1e26; display:flex;
                    flex-direction:column; background:#0d0d10; flex-shrink:0;",

            // Image preview (square)
            div {
                style: "width:100%; aspect-ratio:1/1; background:#0c0c0e;
                        flex-shrink:0; overflow:hidden;",
                if let Some(src) = data_url {
                    img {
                        src: "{src}",
                        style: "width:100%; height:100%; object-fit:contain; display:block;",
                    }
                }
            }

            // File paths
            div {
                style: "padding:11px 14px; border-bottom:1px solid #1a1a22; flex-shrink:0;",
                div {
                    style: "font-size:12px; color:#ddd; margin-bottom:4px; word-break:break-all;",
                    "{name}"
                }
                div {
                    style: "font-size:10px; color:#3a3a50; letter-spacing:0.04em; margin-bottom:2px;",
                    "{idx_name}"
                }
                div {
                    style: "font-size:10px; color:#2e2e3e; letter-spacing:0.03em; word-break:break-all;",
                    "{path_str}"
                }
            }

            // Tag lists
            div {
                style: "flex:1; overflow-y:auto;",

                // Model tags
                if !entry.tags.is_empty() {
                    SectionHeader { label: format!("Model Tags ({})", entry.tags.len()) }
                    for (tag, score) in entry.tags.iter() {
                        div {
                            style: "display:flex; align-items:center; gap:10px; padding:4px 14px;",
                            span {
                                style: "font-size:10px; color:{score_color(*score)};
                                        width:40px; text-align:right; flex-shrink:0;
                                        font-variant-numeric:tabular-nums;",
                                "{score:.3}"
                            }
                            div {
                                style: "flex:1; height:2px; background:#1a1a22; border-radius:1px;
                                        overflow:hidden;",
                                div {
                                    style: "height:100%; width:{(score * 100.0) as u32}%;
                                            background:{score_color(*score)}; border-radius:1px;",
                                }
                            }
                            span {
                                style: "font-size:11px; color:#bbb; min-width:100px; letter-spacing:0.02em;",
                                "{tag}"
                            }
                        }
                    }
                }

                // Custom tags
                if !entry.custom_tags.is_empty() {
                    SectionHeader { label: format!("Custom Tags ({})", entry.custom_tags.len()) }
                    for tag in entry.custom_tags.iter() {
                        div {
                            style: "padding:5px 14px;",
                            span {
                                style: "font-size:11px; color:#9b8dd4; letter-spacing:0.04em;",
                                "# {tag}"
                            }
                        }
                    }
                }

                if entry.tags.is_empty() && entry.custom_tags.is_empty() {
                    div {
                        style: "padding:24px 14px; font-size:11px; color:#333;
                                text-align:center; letter-spacing:0.08em;",
                        "no tags recorded"
                    }
                }
            }
        }
    }
}

// ── Section header ────────────────────────────────────────────────────────────

#[component]
#[allow(non_snake_case)]
fn SectionHeader(label: String) -> Element {
    rsx! {
        div {
            style: "padding:10px 14px 4px; font-size:10px; letter-spacing:0.12em;
                    text-transform:uppercase; color:#3a3a50; border-top:1px solid #1a1a22;
                    margin-top:2px;",
            "{label}"
        }
    }
}

// ── Empty state ───────────────────────────────────────────────────────────────

#[allow(non_snake_case)]
fn EmptyLibrary() -> Element {
    rsx! {
        div {
            style: "display:flex; flex-direction:column; align-items:center;
                    justify-content:center; height:100%; gap:14px; color:#2e2e3a;",

            svg {
                width: "56", height: "56", view_box: "0 0 24 24",
                fill: "none", stroke: "currentColor", stroke_width: "1",
                stroke_linecap: "round", stroke_linejoin: "round",
                rect { x: "3", y: "3", width: "18", height: "18", rx: "2", ry: "2" }
                circle { cx: "8.5", cy: "8.5", r: "1.5" }
                polyline { points: "21 15 16 10 5 21" }
            }

            span {
                style: "font-size:12px; letter-spacing:0.12em; text-transform:uppercase;",
                "Library is empty"
            }
            span {
                style: "font-size:11px; color:#222; letter-spacing:0.06em;",
                "Tag and save images from the Tagger tab"
            }
        }
    }
}
