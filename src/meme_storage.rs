use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Deserialize;

// ── Directories ───────────────────────────────────────────────────────────────

pub fn templates_dir() -> PathBuf {
    crate::storage::data_dir().join("templates")
}

pub fn memes_dir() -> PathBuf {
    crate::storage::data_dir()
}

// ── Config types — exact memegen schema ──────────────────────────────────────

/// One text region. Field names and defaults match memegen's `Text` dataclass.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TextRegion {
    /// "upper" | "lower" | "default" | "none" | "mock"  (default: "upper")
    #[serde(default = "default_style")]
    pub style: String,
    /// CSS color name, "#RRGGBB", or "#RRGGBBAA"  (default: "white")
    #[serde(default = "default_color")]
    pub color: String,
    /// Font id or alias: "thick", "impact", "comic", …  (default: "thick")
    #[serde(default = "default_font")]
    pub font: String,

    /// Left edge of text box as a fraction of image width  (default: 0.0)
    #[serde(default)]
    pub anchor_x: f32,
    /// Top edge of text box as a fraction of image height  (default: 0.0)
    #[serde(default)]
    pub anchor_y: f32,

    /// Rotation angle in degrees — stored but not applied to static PNG output
    #[serde(default)]
    pub angle: f32,

    /// Box width  as a fraction of image width   (default: 1.0)
    #[serde(default = "one_f")]
    pub scale_x: f32,
    /// Box height as a fraction of image height  (default: 0.2)
    #[serde(default = "point_two_f")]
    pub scale_y: f32,

    /// "center" | "left" | "right"  (default: "center")
    #[serde(default = "default_align")]
    pub align: String,

    /// Animation-only; ignored for static PNG output  (default: 0.0)
    #[serde(default)]
    pub start: f32,
    /// Animation-only; ignored for static PNG output  (default: 1.0)
    #[serde(default = "one_f")]
    pub stop: f32,
}

fn default_style()   -> String { "upper".into()  }
fn default_color()   -> String { "white".into()  }
fn default_font()    -> String { "thick".into()  }
fn default_align()   -> String { "center".into() }
fn one_f()           -> f32    { 1.0             }
fn point_two_f()     -> f32    { 0.2             }

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
pub struct OverlayRegion {
    #[serde(default = "half_f")]
    pub center_x: f32,
    #[serde(default = "half_f")]
    pub center_y: f32,
    #[serde(default)]
    pub angle: f32,
    #[serde(default = "quarter_f")]
    pub scale: f32,
}

fn half_f()    -> f32 { 0.5  }
fn quarter_f() -> f32 { 0.25 }

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct MemeConfig {
    pub name: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub text: Vec<TextRegion>,
    /// One example placeholder string per text region.
    #[serde(default)]
    pub example: Vec<String>,
    #[serde(default)]
    pub overlay: Vec<OverlayRegion>,
}

// ── Public template type ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct MemeTemplate {
    /// Folder name, e.g. "ackbar"
    pub id: String,
    pub config: MemeConfig,
    /// Absolute path to the base image (default.jpg / .png / …)
    pub image_path: PathBuf,
}

impl MemeTemplate {
    pub fn display_name(&self) -> &str {
        &self.config.name
    }

    pub fn text_field_count(&self) -> usize {
        self.config.text.len()
    }

    /// Example placeholder text for text field `i`.
    pub fn example_for(&self, i: usize) -> &str {
        self.config.example.get(i).map(|s| s.as_str()).unwrap_or("")
    }
}

// ── Load all templates ────────────────────────────────────────────────────────

pub fn load_templates() -> Vec<MemeTemplate> {
    let dir = templates_dir();
    if !dir.exists() {
        return vec![];
    }
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return vec![];
    };

    let mut out: Vec<MemeTemplate> = rd
        .flatten()
        .filter_map(|e| {
            let folder = e.path();
            if !folder.is_dir() {
                return None;
            }
            let id = folder.file_name()?.to_string_lossy().to_string();
            if id.starts_with('_') {
                return None;
            }

            let cfg_path = ["config.yml", "template.yml"]
                .iter()
                .map(|n| folder.join(n))
                .find(|p| p.exists())?;

            let text = std::fs::read_to_string(&cfg_path).ok()?;
            let config: MemeConfig = serde_yaml::from_str(&text)
                .map_err(|e| {
                    eprintln!("Failed to parse {}: {e}", cfg_path.display());
                    e
                })
                .ok()?;

            let image_path =
                ["default.jpg", "default.jpeg", "default.png", "default.webp", "default.gif"]
                    .iter()
                    .map(|n| folder.join(n))
                    .find(|p| p.exists())?;

            Some(MemeTemplate { id, config, image_path })
        })
        .collect();

    out.sort_by(|a, b| {
        a.config.name.to_lowercase().cmp(&b.config.name.to_lowercase())
    });
    out
}

// ── Meme generation ───────────────────────────────────────────────────────────

pub fn generate_meme(template: &MemeTemplate, texts: &[String]) -> Result<PathBuf> {
    use ab_glyph::PxScale;
    use imageproc::drawing::{draw_text_mut, text_size};

    let img = image::open(&template.image_path)
        .with_context(|| format!("cannot open {}", template.image_path.display()))?;
    let mut canvas = img.to_rgba8();
    let (img_w, img_h) = (canvas.width(), canvas.height());

    for (i, region) in template.config.text.iter().enumerate() {
        let raw = texts.get(i).map(|s| s.as_str()).unwrap_or("").trim();
        if raw.is_empty() {
            continue;
        }

        let display_text = apply_style(raw, &region.style);

        // anchor_x/y = top-left corner (fractions of image size).
        // scale_x/y  = box dimensions  (fractions of image size).
        // start/stop are animation-only; they do not affect static rendering.
        let box_x = (region.anchor_x.clamp(0.0, 1.0) * img_w as f32) as u32;
        let box_y = (region.anchor_y.clamp(0.0, 1.0) * img_h as f32) as u32;
        let box_w = (region.scale_x.clamp(0.0, 1.0) * img_w as f32).max(1.0) as u32;
        let box_h = (region.scale_y.clamp(0.0, 1.0) * img_h as f32).max(1.0) as u32;

        let font = match load_font(&region.font) {
            Some(f) => f,
            None => {
                eprintln!("No font found for '{}'; skipping region {i}", region.font);
                continue;
            }
        };

        let (fg_color, stroke_color, stroke_px) = resolve_colors(&region.color);

        let (font_size, lines) = fit_text(&font, &display_text, box_w, box_h);
        let scale   = PxScale::from(font_size);
        let line_h  = font_size * 1.2;
        let total_h = lines.len() as f32 * line_h;

        let block_top = box_y as f32 + (box_h as f32 - total_h) / 2.0;

        for (li, line) in lines.iter().enumerate() {
            let (lw, _) = text_size(scale, &font, line);
            let draw_x = match region.align.as_str() {
                "center" | "" => box_x as i32 + (box_w as i32 - lw as i32) / 2,
                "right"       => box_x as i32 +  box_w as i32 - lw as i32,
                _             => box_x as i32,
            };
            let draw_y = (block_top + li as f32 * line_h) as i32;

            let sw = stroke_px as i32;
            for ox in -sw..=sw {
                for oy in -sw..=sw {
                    if ox != 0 || oy != 0 {
                        draw_text_mut(
                            &mut canvas, stroke_color,
                            draw_x + ox, draw_y + oy,
                            scale, &font, line,
                        );
                    }
                }
            }
            draw_text_mut(&mut canvas, fg_color, draw_x, draw_y, scale, &font, line);
        }
    }

    let dir = memes_dir();
    std::fs::create_dir_all(&dir).context("cannot create memes dir")?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let out = dir.join(format!("{}_{}.png", template.id, ts));
    canvas.save(&out).context("cannot save meme")?;

    // Write a library .idx sidecar so this meme appears in the image library.
    let idx_path = crate::storage::idx_path_for(&out);
    let _ = std::fs::write(
        &idx_path,
        format!(
            "# image tagger index v1\n[tags]\nmeme=1.0000\n[custom]\n{}\n",
            template.id
        ),
    );

    Ok(out)
}

// ── Text style ────────────────────────────────────────────────────────────────

fn apply_style(text: &str, style: &str) -> String {
    match style {
        "upper" | "" => text.to_uppercase(),
        "lower"      => text.to_lowercase(),
        "none"       => text.to_string(),
        "default"    => stylize_default(text),
        "mock"       => stylize_mock(text),
        _            => text.to_uppercase(),
    }
}

fn stylize_default(text: &str) -> String {
    let s = if text.chars().all(|c| c.is_lowercase() || !c.is_alphabetic()) {
        let mut chars = text.chars();
        match chars.next() {
            None    => String::new(),
            Some(c) => c.to_uppercase().to_string() + chars.as_str(),
        }
    } else {
        text.to_string()
    };
    s.split(' ')
        .map(|w| if w == "i" { "I" } else { w })
        .collect::<Vec<_>>()
        .join(" ")
}

fn stylize_mock(text: &str) -> String {
    let mut toggle = false;
    text.chars()
        .map(|c| {
            if c.is_alphabetic() {
                let out = if toggle {
                    c.to_uppercase().next().unwrap_or(c)
                } else {
                    c.to_lowercase().next().unwrap_or(c)
                };
                toggle = !toggle;
                out
            } else {
                c
            }
        })
        .collect()
}

// ── Text fitting ──────────────────────────────────────────────────────────────

fn fit_text(
    font: &ab_glyph::FontVec,
    text: &str,
    box_w: u32,
    box_h: u32,
) -> (f32, Vec<String>) {
    use ab_glyph::PxScale;
    use imageproc::drawing::text_size;

    let mut size = (box_h as f32 * 0.85).min(200.0).max(10.0);
    loop {
        let scale = PxScale::from(size);
        let lines = word_wrap(font, text, scale, box_w);
        let max_w = lines.iter()
            .map(|l| text_size(scale, font, l).0)
            .max()
            .unwrap_or(0);
        let total_h = lines.len() as f32 * size * 1.2;

        if (max_w <= box_w && total_h <= box_h as f32) || size <= 10.0 {
            return (size, lines);
        }
        size = (size * 0.92).max(10.0);
    }
}

fn word_wrap(
    font: &ab_glyph::FontVec,
    text: &str,
    scale: ab_glyph::PxScale,
    max_w: u32,
) -> Vec<String> {
    use imageproc::drawing::text_size;

    let mut out = Vec::new();
    for para in text.split("~n") {
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

// ── Color + stroke resolution ─────────────────────────────────────────────────

fn resolve_colors(color: &str) -> (image::Rgba<u8>, image::Rgba<u8>, u32) {
    let fg = parse_color(color);

    if color.to_lowercase() == "black" {
        return (fg, image::Rgba([255, 255, 255, 128]), 1);
    }

    let stroke_alpha = if color.starts_with('#') && color.len() == 9 {
        u8::from_str_radix(&color[7..9], 16).unwrap_or(255)
    } else {
        255
    };
    (fg, image::Rgba([0, 0, 0, stroke_alpha]), 2)
}

fn parse_color(s: &str) -> image::Rgba<u8> {
    match s.to_lowercase().as_str() {
        "white"  => return image::Rgba([255, 255, 255, 255]),
        "black"  => return image::Rgba([0,   0,   0,   255]),
        "red"    => return image::Rgba([255, 0,   0,   255]),
        "yellow" => return image::Rgba([255, 255, 0,   255]),
        "green"  => return image::Rgba([0,   200, 0,   255]),
        "blue"   => return image::Rgba([0,   0,   255, 255]),
        "orange" => return image::Rgba([255, 165, 0,   255]),
        _        => {}
    }
    let hex = s.trim_start_matches('#');
    match hex.len() {
        6 => image::Rgba([
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(255),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(255),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(255),
            255,
        ]),
        8 => image::Rgba([
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(255),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(255),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(255),
            u8::from_str_radix(&hex[6..8], 16).unwrap_or(255),
        ]),
        _ => image::Rgba([255, 255, 255, 255]),
    }
}

// ── Font loading ──────────────────────────────────────────────────────────────

/// Returns an [`ab_glyph::FontVec`] for the given memegen font id or alias.
///
/// All fonts are fetched via the `google-fonts` crate (variable feature),
/// which downloads each font once and caches it locally. Substitutes are
/// chosen to be the closest available variable font for each alias:
///
/// | alias            | original          | variable substitute          |
/// |------------------|-------------------|------------------------------|
/// | thick/titillium  | Titillium Black   | League Spartan (heavy cond.) |
/// | impact           | Impact            | League Gothic (condensed)    |
/// | comic/kalam      | Kalam             | Caveat (handwriting)         |
/// | notosans         | Noto Sans         | Noto Sans                    |
/// | thin             | Titillium SemiBold| Josefin Sans (thin)          |
/// | tiny/segoe       | Segoe UI          | Arimo (metric-compat.)       |
/// | jp/hgminchob     | HG Mincho         | Noto Sans JP                 |
/// | he/notosanshebrew| Noto Sans Hebrew  | Noto Sans Hebrew             |
fn load_font(id_or_alias: &str) -> Option<ab_glyph::FontVec> {
    let bytes: Vec<u8> = match id_or_alias.to_lowercase().as_str() {
        "thick" | "titilliumweb"     => google_fonts::league_spartan_variable(),
        "impact"                      => google_fonts::league_gothic_regular_variable(),
        "comic" | "kalam"             => google_fonts::caveat_variable(),
        "notosans"                    => google_fonts::noto_sans_variable(),
        "thin" | "titilliumweb-thin" => google_fonts::josefin_sans_variable(),
        "tiny" | "segoe"              => google_fonts::arimo_variable(),
        "jp" | "hgminchob"           => google_fonts::noto_sans_jp_variable(),
        "he" | "notosanshebrew"      => google_fonts::noto_sans_hebrew_variable(),
        _                             => google_fonts::league_gothic_regular_variable(),
    }.ok()?;
    ab_glyph::FontVec::try_from_vec(bytes).ok()
}

// ── Favorites ─────────────────────────────────────────────────────────────────

fn favorites_path() -> PathBuf {
    crate::storage::data_dir().join("meme_favorites")
}

pub fn load_favorites() -> std::collections::HashSet<String> {
    let Ok(text) = std::fs::read_to_string(favorites_path()) else {
        return std::collections::HashSet::new();
    };
    text.lines()
        .map(|l| l.trim().to_owned())
        .filter(|l| !l.is_empty())
        .collect()
}

fn save_favorites(favorites: &std::collections::HashSet<String>) {
    let mut ids: Vec<&str> = favorites.iter().map(|s| s.as_str()).collect();
    ids.sort();
    let _ = std::fs::write(favorites_path(), ids.join("\n"));
}

pub fn toggle_favorite(id: &str) -> std::collections::HashSet<String> {
    let mut favs = load_favorites();
    if !favs.remove(id) {
        favs.insert(id.to_owned());
    }
    save_favorites(&favs);
    favs
}
