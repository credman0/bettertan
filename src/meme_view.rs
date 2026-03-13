use std::path::PathBuf;

use dioxus::prelude::*;

use crate::{
    image_to_data_url,
    meme_storage::{self, MemeTemplate},
};

// ── Main view ─────────────────────────────────────────────────────────────────

#[allow(non_snake_case)]
pub fn MemeView() -> Element {
    let mut templates: Signal<Vec<MemeTemplate>> = use_signal(meme_storage::load_templates);
    let mut selected: Signal<Option<usize>> = use_signal(|| None);

    let refresh = move |_| {
        templates.set(meme_storage::load_templates());
        selected.set(None);
    };

    let count = templates.read().len();
    let count_label = format!("{} template{}", count, if count == 1 { "" } else { "s" });

    rsx! {
        div {
            style: "display:flex; height:100%; overflow:hidden;",

            div {
                style: "flex:1; display:flex; flex-direction:column; overflow:hidden;",

                div {
                    style: "display:flex; align-items:center; gap:12px; padding:9px 20px; border-bottom:1px solid #1e1e26; background:#13131a; flex-shrink:0;",
                    span {
                        style: "font-size:10px; color:#555; letter-spacing:0.12em; text-transform:uppercase;",
                        "{count_label}"
                    }
                    div { style: "flex:1;" }
                    span {
                        style: "font-size:10px; color:#2e2e3e; letter-spacing:0.08em;",
                        "~/.image_tagger/templates/"
                    }
                    button {
                        style: "padding:6px 14px; background:#1a1a26; color:#888; border:1px solid #2a2a38; border-radius:4px; font-family:inherit; font-size:11px; letter-spacing:0.08em; cursor:pointer;",
                        onclick: refresh,
                        "↺  Refresh"
                    }
                }

                div {
                    style: "flex:1; overflow-y:auto; padding:20px;",
                    if templates.read().is_empty() {
                        EmptyTemplates {}
                    } else {
                        div {
                            style: "display:grid; grid-template-columns:repeat(auto-fill,minmax(170px,1fr)); gap:14px;",
                            for (i, tmpl) in templates.read().iter().enumerate() {
                                TemplateCard {
                                    key: "{i}",
                                    template: tmpl.clone(),
                                    selected: *selected.read() == Some(i),
                                    onclick: move |_| {
                                        let new = if *selected.read() == Some(i) { None } else { Some(i) };
                                        selected.set(new);
                                    },
                                }
                            }
                        }
                    }
                }
            }

            if let Some(idx) = *selected.read() {
                if let Some(tmpl) = templates.read().get(idx).cloned() {
                    MemeEditor { key: "{tmpl.id}", template: tmpl }
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
    onclick: EventHandler<MouseEvent>,
) -> Element {
    let data_url   = image_to_data_url(&template.image_path);
    let name       = template.display_name().to_string();
    let n_fields   = template.text_field_count();
    let field_label = format!("{} text field{}", n_fields, if n_fields == 1 { "" } else { "s" });
    let border = if selected { "#5b8dee" } else { "#1e1e26" };

    rsx! {
        div {
            style: "background:#13131a; border:1px solid {border}; border-radius:6px; overflow:hidden; cursor:pointer; transition:border-color 0.15s;",
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
fn MemeEditor(template: MemeTemplate) -> Element {
    let n = template.text_field_count();

    let mut texts:      Signal<Vec<String>>                    = use_signal(|| vec![String::new(); n]);
    let mut generating: Signal<bool>                           = use_signal(|| false);
    let mut result:     Signal<Option<Result<PathBuf, String>>> = use_signal(|| None);

    let template_data_url = image_to_data_url(&template.image_path);
    let name              = template.display_name().to_string();
    let id                = template.id.clone();
    let memes_dir         = meme_storage::memes_dir();

    // Pre-compute what the preview image source should be:
    // after a successful generate, show the output; otherwise the template.
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

    // Pre-compute the status line so no match lives inside rsx!
    let status_ok:  Option<String> = result.read().as_ref().and_then(|r| {
        r.as_ref().ok().and_then(|p| {
            p.file_name().map(|n| n.to_string_lossy().to_string())
        })
    });
    let status_err: Option<String> = result.read().as_ref().and_then(|r| {
        r.as_ref().err().map(|e| e.clone())
    });

    let btn_disabled = *generating.read() || n == 0;
    let btn_label    = if *generating.read() { "Generating…" } else { "Generate Meme" };
    let btn_opacity  = if btn_disabled { "0.5" } else { "1" };

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
                style: "padding:11px 14px; border-bottom:1px solid #1a1a22; flex-shrink:0;",
                div {
                    style: "font-size:12px; color:#ddd; margin-bottom:2px; word-break:break-all;",
                    "{name}"
                }
                div {
                    style: "font-size:10px; color:#3a3a50; letter-spacing:0.04em;",
                    "{id}"
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
                        let _ = open_in_file_manager(&memes_dir);
                    },
                    "Open Memes Folder"
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
