//! In-memory Tantivy search helpers for the library and meme template views.
//!
//! Each public function builds a fresh RAM-backed index from the supplied slice,
//! executes the query, and returns the original slice indices sorted from most
//! relevant to least relevant.  When the query is empty every index is returned
//! in its original order (no filtering, no re-ordering).

use tantivy::{
    collector::TopDocs,
    directory::RamDirectory,
    query::QueryParser,
    schema::{Schema, Value, STORED, TEXT},
    Index, TantivyDocument,
};

use crate::{meme_storage::MemeTemplate, storage::LibraryEntry};

// ── Library ───────────────────────────────────────────────────────────────────

/// Return the indices of `entries` sorted by relevance for `query`.
/// Returns all indices in original order when `query` is blank.
pub fn search_library(entries: &[LibraryEntry], query: &str) -> Vec<usize> {
    let q = query.trim();
    if q.is_empty() {
        return (0..entries.len()).collect();
    }

    let (index, id_field, text_field) = match build_library_index(entries) {
        Some(x) => x,
        None => return (0..entries.len()).collect(),
    };

    run_query(&index, id_field, text_field, q, entries.len())
        .unwrap_or_else(|| (0..entries.len()).collect())
}

fn build_library_index(
    entries: &[LibraryEntry],
) -> Option<(Index, tantivy::schema::Field, tantivy::schema::Field)> {
    let mut sb = Schema::builder();
    let id_field   = sb.add_u64_field("id",   STORED);
    let text_field = sb.add_text_field("text", TEXT);
    let schema = sb.build();

    let index = Index::open_or_create(RamDirectory::create(), schema).ok()?;
    let mut writer = index.writer(15_000_000).ok()?;

    for (i, entry) in entries.iter().enumerate() {
        let mut doc = TantivyDocument::default();
        doc.add_u64(id_field, i as u64);
        doc.add_text(text_field, entry.image_file_name());
        for tag in entry.all_tag_names() {
            doc.add_text(text_field, &tag);
        }
        if let Some(ref ocr) = entry.ocr_text {
            doc.add_text(text_field, ocr);
        }
        writer.add_document(doc).ok()?;
    }
    writer.commit().ok()?;

    Some((index, id_field, text_field))
}

// ── Meme templates ────────────────────────────────────────────────────────────

/// Return the indices of `templates` sorted by relevance for `query`.
/// Returns all indices in original order when `query` is blank.
pub fn search_templates(templates: &[MemeTemplate], query: &str) -> Vec<usize> {
    let q = query.trim();
    if q.is_empty() {
        return (0..templates.len()).collect();
    }

    let (index, id_field, text_field) = match build_template_index(templates) {
        Some(x) => x,
        None => return (0..templates.len()).collect(),
    };

    run_query(&index, id_field, text_field, q, templates.len())
        .unwrap_or_else(|| (0..templates.len()).collect())
}

fn build_template_index(
    templates: &[MemeTemplate],
) -> Option<(Index, tantivy::schema::Field, tantivy::schema::Field)> {
    let mut sb = Schema::builder();
    let id_field   = sb.add_u64_field("id",   STORED);
    let text_field = sb.add_text_field("text", TEXT);
    let schema = sb.build();

    let index = Index::open_or_create(RamDirectory::create(), schema).ok()?;
    let mut writer = index.writer(15_000_000).ok()?;

    for (i, tmpl) in templates.iter().enumerate() {
        let mut doc = TantivyDocument::default();
        doc.add_u64(id_field, i as u64);
        doc.add_text(text_field, tmpl.display_name());
        doc.add_text(text_field, &tmpl.id);
        for kw in &tmpl.config.keywords {
            doc.add_text(text_field, kw);
        }
        writer.add_document(doc).ok()?;
    }
    writer.commit().ok()?;

    Some((index, id_field, text_field))
}

// ── Shared search execution ───────────────────────────────────────────────────

fn run_query(
    index: &Index,
    id_field: tantivy::schema::Field,
    text_field: tantivy::schema::Field,
    query_str: &str,
    total: usize,
) -> Option<Vec<usize>> {
    let reader = index
        .reader_builder()
        .reload_policy(tantivy::ReloadPolicy::Manual)
        .try_into()
        .ok()?;
    let searcher = reader.searcher();

    let mut parser = QueryParser::for_index(index, vec![text_field]);
    parser.set_field_fuzzy(text_field, true, 1, true);

    let query = parser.parse_query(query_str).ok()?;
    let top_docs = searcher.search(&query, &TopDocs::with_limit(total)).ok()?;

    let results: Vec<usize> = top_docs
        .iter()
        .filter_map(|(_, addr)| {
            let doc: TantivyDocument = searcher.doc(*addr).ok()?;
            let v = doc.get_first(id_field)?;
            v.as_u64().map(|n| n as usize)
        })
        .collect();

    Some(results)
}
