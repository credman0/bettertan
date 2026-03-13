use std::collections::HashSet;
use std::path::PathBuf;

use dioxus::prelude::*;

use crate::{
    image_to_data_url, image_to_thumbnail_url,
    meme_storage::{self, MemeTemplate},
    search,
};

// ── Main view ─────────────────────────────────────────────────────────────────

#[allow(non_snake_case)]
pub fn MemeView() -> Element {
    let mut templates: Signal<Vec<MemeTemplate>> = use_signal(meme_storage::load_templates);
    let mut selected:  Signal<Option<String>>     = use_signal(|| None);
    let mut favorites: Signal<HashSet<String>>    = use_signal(meme_storage::load_favorites);
    let mut query:     Signal<String>             = use_signal(String::new);

    let refresh = move |_| {
        templates.set(meme_storage::load_templates());
        selected.set(None);
    };

    // Pre-compute outside rsx! to avoid borrow conflicts.
    let all_templates = templates.read().clone();
    let fav_set       = favorites.read().clone();
    let selected_id   = selected.read().clone();
    let query_str     = query.read().clone();

    let searching = !query_str.trim().is_empty();

    // Search-sorted indices over all templates.
    let sorted_indices = search::search_templates(&all_templates, &query_str);

    // When not searching, keep the existing favorites / rest split.
    let fav_templates: Vec<MemeTemplate> = if !searching {
        sorted_indices.iter()
            .filter(|&&i| fav_set.contains(&all_templates[i].id))
            .map(|&i| all_templates[i].clone())
            .collect()
    } else {
        vec![]
    };
    let rest_templates: Vec<MemeTemplate> = if !searching {
        sorted_indices.iter()
            .filter(|&&i| !fav_set.contains(&all_templates[i].id))
            .map(|&i| all_templates[i].clone())
            .collect()
    } else {
        // When searching show all sorted results in a single flat list.
        sorted_indices.iter()
            .map(|&i| all_templates[i].clone())
            .collect()
    };

    let selected_tmpl: Option<(String, MemeTemplate)> = selected_id.as_ref()
        .and_then(|id| all_templates.iter().find(|t| &t.id == id).map(|t| (t.id.clone(), t.clone())));

    let count       = all_templates.len();
    let count_label = if searching {
        format!("{} / {} template{}", rest_templates.len(), count, if count == 1 { "" } else { "s" })
    } else {
        format!("{} template{}", count, if count == 1 { "" } else { "s" })
    };
    let has_favs    = !fav_templates.is_empty();

    rsx! {
        div {
            style: "display:flex; height:100%; overflow:hidden;",

            div {
                style: "flex:1; display:flex; flex-direction:column; overflow:hidden;",

                div {
                    style: "display:flex; align-items:center; gap:12px; padding:9px 20px; border-bottom:1px solid #1e1e26; background:#13131a; flex-shrink:0;",
                    span {
                        style: "font-size:10px; color:#555; letter-spacing:0.12em; text-transform:uppercase; flex-shrink:0;",
                        "{count_label}"
                    }
                    // Search bar
                    input {
                        r#type: "text",
                        placeholder: "Search name, keywords…",
                        value: "{query_str}",
                        oninput: move |e| {
                            query.set(e.value());
                            selected.set(None);
                        },
                        style: "flex:1; min-width:0; background:#0d0d14; border:1px solid #2a2a38; border-radius:4px; padding:5px 10px; color:#ccc; font-family:inherit; font-size:11px; letter-spacing:0.04em; outline:none;",
                    }
                    span {
                        style: "font-size:10px; color:#2e2e3e; letter-spacing:0.08em; flex-shrink:0;",
                        "~/.image_tagger/templates/"
                    }
                    button {
                        style: "padding:6px 14px; background:#1a1a26; color:#888; border:1px solid #2a2a38; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.08em; cursor:pointer; flex-shrink:0;",
                        onclick: refresh,
                        "↺  Refresh"
                    }
                }

                div {
                    style: "flex:1; overflow-y:auto; padding:20px;",
                    if all_templates.is_empty() {
                        EmptyTemplates {}
                    } else if searching {
                        // Flat relevance-sorted results when a query is active.
                        if rest_templates.is_empty() {
                            div {
                                style: "font-size:11px; color:#2e2e3a; text-align:center; padding-top:40px; letter-spacing:0.08em;",
                                "No matching templates"
                            }
                        } else {
                            div {
                                style: "display:grid; grid-template-columns:repeat(auto-fill,minmax(170px,1fr)); gap:14px;",
                                for tmpl in rest_templates.iter() {
                                    TemplateCard {
                                        key: "{tmpl.id}",
                                        template: tmpl.clone(),
                                        selected: selected_id.as_deref() == Some(tmpl.id.as_str()),
                                        favorited: fav_set.contains(&tmpl.id),
                                        onclick: {
                                            let id = tmpl.id.clone();
                                            move |_| {
                                                let new = if selected.read().as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
                                                selected.set(new);
                                            }
                                        },
                                        on_toggle_favorite: {
                                            let id = tmpl.id.clone();
                                            move |_| { favorites.set(meme_storage::toggle_favorite(&id)); }
                                        },
                                    }
                                }
                            }
                        }
                    } else {
                        // Normal (non-search) layout: favorites section + rest.
                        // Favorites section
                        if has_favs {
                            div {
                                style: "font-size:10px; color:#7a6a30; letter-spacing:0.12em; text-transform:uppercase; margin-bottom:10px;",
                                "★  Favorites"
                            }
                            div {
                                style: "display:grid; grid-template-columns:repeat(auto-fill,minmax(170px,1fr)); gap:14px; margin-bottom:24px;",
                                for tmpl in fav_templates.iter() {
                                    TemplateCard {
                                        key: "fav-{tmpl.id}",
                                        template: tmpl.clone(),
                                        selected: selected_id.as_deref() == Some(tmpl.id.as_str()),
                                        favorited: true,
                                        onclick: {
                                            let id = tmpl.id.clone();
                                            move |_| {
                                                let new = if selected.read().as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
                                                selected.set(new);
                                            }
                                        },
                                        on_toggle_favorite: {
                                            let id = tmpl.id.clone();
                                            move |_| { favorites.set(meme_storage::toggle_favorite(&id)); }
                                        },
                                    }
                                }
                            }
                            div {
                                style: "font-size:10px; color:#3a3a50; letter-spacing:0.12em; text-transform:uppercase; margin-bottom:10px;",
                                "All Templates"
                            }
                        }
                        // Non-favorite templates
                        div {
                            style: "display:grid; grid-template-columns:repeat(auto-fill,minmax(170px,1fr)); gap:14px;",
                            for tmpl in rest_templates.iter() {
                                TemplateCard {
                                    key: "{tmpl.id}",
                                    template: tmpl.clone(),
                                    selected: selected_id.as_deref() == Some(tmpl.id.as_str()),
                                    favorited: false,
                                    onclick: {
                                        let id = tmpl.id.clone();
                                        move |_| {
                                            let new = if selected.read().as_deref() == Some(id.as_str()) { None } else { Some(id.clone()) };
                                            selected.set(new);
                                        }
                                    },
                                    on_toggle_favorite: {
                                        let id = tmpl.id.clone();
                                        move |_| { favorites.set(meme_storage::toggle_favorite(&id)); }
                                    },
                                }
                            }
                        }
                    }
                }
            }

            if let Some((tmpl_id, tmpl)) = selected_tmpl {
                MemeEditor {
                    key: "{tmpl_id}",
                    template: tmpl,
                    favorites,
                }
            }
        }
    }
}

// ── Template card ─────────────────────────────────────────────────────────────

#[component]
#[allow(non_snake_case)]
fn TemplateCard(
    template: MemeTemplate,
    selected: bool,
    favorited: bool,
    onclick: EventHandler<MouseEvent>,
    on_toggle_favorite: EventHandler<MouseEvent>,
) -> Element {
    // Load thumbnail asynchronously so it never blocks the render thread.
    let thumb_path = template.image_path.clone();
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
    let name        = template.display_name().to_string();
    let n_fields    = template.text_field_count();
    let field_label = format!("{} text field{}", n_fields, if n_fields == 1 { "" } else { "s" });
    let border      = if selected { "#5b8dee" } else { "#1e1e26" };
    let star        = if favorited { "★" } else { "☆" };
    let star_color  = if favorited { "#f0c040" } else { "#3a3a50" };

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
                    style: "font-size:11px; color:#bbb; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; margin-bottom:3px;",
                    "{name}"
                }
                div {
                    style: "font-size:10px; color:#555; letter-spacing:0.05em;",
                    "{field_label}"
                }
            }
        }
    }
}

// ── Meme editor ───────────────────────────────────────────────────────────────

#[component]
#[allow(non_snake_case)]
fn MemeEditor(
    template: MemeTemplate,
    favorites: Signal<HashSet<String>>,
) -> Element {
    let n = template.text_field_count();

    let mut texts:      Signal<Vec<String>>                    = use_signal(|| vec![String::new(); n]);
    let mut generating: Signal<bool>                           = use_signal(|| false);
    let mut result:     Signal<Option<Result<PathBuf, String>>> = use_signal(|| None);

    let name        = template.display_name().to_string();
    let id          = template.id.clone();
    let library_dir = meme_storage::memes_dir();

    // Read favorites reactively so this component re-renders on changes.
    let favorited   = favorites.read().contains(&id);

    // Load the template base image asynchronously.
    let tmpl_img_path = template.image_path.clone();
    let tmpl_url_res = use_resource(move || {
        let p = tmpl_img_path.clone();
        async move {
            tokio::task::spawn_blocking(move || image_to_data_url(&p))
                .await
                .ok()
                .flatten()
        }
    });
    let template_data_url: Option<String> = tmpl_url_res.read().as_ref().and_then(|v| v.clone());

    // After a successful generate, switch to showing the output image.
    let preview_src: Option<String> = {
        let r = result.read();
        if let Some(res) = r.as_ref() {
            if let Ok(path) = res {
                image_to_data_url(path)
            } else {
                template_data_url.clone()
            }
        } else {
            template_data_url.clone()
        }
    };

    let status_ok:  Option<String> = result.read().as_ref().and_then(|r| {
        r.as_ref().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
    });
    let status_err: Option<String> = result.read().as_ref().and_then(|r| {
        r.as_ref().err().map(|e| e.clone())
    });

    let btn_disabled = *generating.read() || n == 0;
    let btn_label    = if *generating.read() { "Generating…" } else { "Generate Meme" };
    let btn_opacity  = if btn_disabled { "0.5" } else { "1" };

    let fav_label  = if favorited { "★" } else { "☆" };
    let fav_color  = if favorited { "#f0c040" } else { "#555" };
    let fav_border = if favorited { "#8a7030" } else { "#2a2a38" };

    rsx! {
        div {
            style: "width:320px; border-left:1px solid #1e1e26; display:flex; flex-direction:column; background:#0d0d10; flex-shrink:0; overflow:hidden;",

            div {
                style: "width:100%; aspect-ratio:4/3; background:#0c0c0e; flex-shrink:0; overflow:hidden;",
                if let Some(src) = preview_src {
                    img { src: "{src}", style: "width:100%; height:100%; object-fit:contain; display:block;" }
                }
            }

            div {
                style: "padding:11px 14px; border-bottom:1px solid #1a1a22; flex-shrink:0; display:flex; align-items:center; gap:8px;",
                div {
                    style: "flex:1; min-width:0;",
                    div {
                        style: "font-size:12px; color:#ddd; margin-bottom:2px; word-break:break-all;",
                        "{name}"
                    }
                    div {
                        style: "font-size:10px; color:#3a3a50; letter-spacing:0.04em;",
                        "{id}"
                    }
                }
                button {
                    style: "flex-shrink:0; padding:3px 8px; background:transparent; border:1px solid {fav_border}; border-radius:4px; color:{fav_color}; font-size:15px; cursor:pointer; line-height:1;",
                    onclick: {
                        let id = id.clone();
                        move |_| { favorites.set(meme_storage::toggle_favorite(&id)); }
                    },
                    "{fav_label}"
                }
            }

            div {
                style: "flex:1; overflow-y:auto; padding:12px 14px 6px;",

                if n == 0 {
                    div {
                        style: "font-size:11px; color:#3a3a50; letter-spacing:0.06em; padding-top:6px; line-height:1.6;",
                        "This template has no text fields defined in its config.yml."
                    }
                } else {
                    for i in 0..n {
                        TextInput {
                            key: "{i}",
                            label: format!("Line {}", i + 1),
                            placeholder: template.example_for(i).to_string(),
                            value: texts.read()[i].clone(),
                            on_change: move |val: String| {
                                texts.write()[i] = val;
                            },
                        }
                    }
                }
            }

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
                        let tmpl = template.clone();
                        move |_| {
                            let tmpl = tmpl.clone();
                            let txts = texts.read().clone();
                            generating.set(true);
                            result.set(None);
                            spawn(async move {
                                let r = tokio::task::spawn_blocking(move || {
                                    meme_storage::generate_meme(&tmpl, &txts)
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
                    style: "width:100%; padding:6px 0; background:transparent; color:#444; border:1px solid #1e1e28; border-radius:4px; font-family:inherit; font-size:10px; letter-spacing:0.1em; cursor:pointer;",
                    onclick: move |_| {
                        let _ = open_in_file_manager(&library_dir);
                    },
                    "Open Library Folder"
                }
            }
        }
    }
}

// ── Reusable labelled textarea ────────────────────────────────────────────────

#[component]
#[allow(non_snake_case)]
fn TextInput(
    label: String,
    placeholder: String,
    value: String,
    on_change: EventHandler<String>,
) -> Element {
    rsx! {
        div {
            style: "margin-bottom:14px;",
            div {
                style: "font-size:10px; letter-spacing:0.12em; text-transform:uppercase; color:#3a3a50; margin-bottom:5px;",
                "{label}"
            }
            textarea {
                style: "width:100%; background:#0c0c14; border:1px solid #1e1e2e; border-radius:4px; padding:7px 9px; color:#ccc; font-family:inherit; font-size:11px; resize:vertical; min-height:38px; letter-spacing:0.03em; line-height:1.5;",
                placeholder: "{placeholder}",
                value: "{value}",
                oninput: move |e| on_change.call(e.value()),
            }
        }
    }
}

// ── Empty state ───────────────────────────────────────────────────────────────

#[allow(non_snake_case)]
fn EmptyTemplates() -> Element {
    let dir = meme_storage::templates_dir().to_string_lossy().to_string();

    rsx! {
        div {
            style: "display:flex; flex-direction:column; align-items:center; justify-content:center; height:100%; gap:14px; color:#2e2e3a;",

            svg {
                width: "56", height: "56", view_box: "0 0 24 24",
                fill: "none", stroke: "currentColor", stroke_width: "1",
                stroke_linecap: "round", stroke_linejoin: "round",
                path { d: "M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" }
                line { x1: "12", y1: "11", x2: "12", y2: "17" }
                line { x1: "9",  y1: "14", x2: "15", y2: "14" }
            }

            span {
                style: "font-size:12px; letter-spacing:0.12em; text-transform:uppercase;",
                "No templates found"
            }
            div {
                style: "display:flex; flex-direction:column; align-items:center; gap:4px; max-width:280px; text-align:center;",
                span {
                    style: "font-size:11px; color:#2a2a40; letter-spacing:0.06em; line-height:1.7;",
                    "Add memegen-compatible template folders to:"
                }
                span {
                    style: "font-size:10px; color:#3a3a58; letter-spacing:0.04em; word-break:break-all; line-height:1.6;",
                    "{dir}"
                }
                span {
                    style: "font-size:10px; color:#252535; letter-spacing:0.04em; line-height:1.7; margin-top:4px;",
                    "Each folder needs a config.yml and a default.jpg"
                }
            }
        }
    }
}

// ── OS file-manager helper ────────────────────────────────────────────────────

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
