pub mod chunker;
pub mod pdf;
pub mod image;
pub mod embeddings;
pub mod watcher;

use crate::error::{AppError, Result};
use crate::lance::{DocRow, LanceStore};
use sha2::{Digest, Sha256};
use std::path::Path;
use uuid::Uuid;

pub fn file_hash(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let mut h = Sha256::new();
    h.update(&bytes);
    Ok(hex::encode(h.finalize()))
}

#[derive(Debug)]
pub struct IngestReport {
    pub doc_path: String,
    pub chunks: usize,
}

pub async fn ingest_pdf(store: &LanceStore, path: &Path) -> Result<IngestReport> {
    let extracted = pdf::extract(path)?;
    let chunks = chunker::chunk_text(&extracted.full_text, 500, 50);
    if chunks.is_empty() {
        return Err(AppError::Invalid(format!("no text in {}", path.display())));
    }
    let embs = embeddings::embed(chunks.clone())?;
    let path_str = path.to_string_lossy().to_string();
    let rows: Vec<DocRow> = chunks.iter().enumerate().zip(embs).map(|((i, t), e)| DocRow {
        id: Uuid::new_v4().to_string(),
        doc_path: path_str.clone(),
        doc_type: "pdf".into(),
        chunk_index: i as i32,
        text: t.clone(),
        page: None,
        embedding: e,
    }).collect();
    store.delete_by_doc_path(&path_str).await?;
    let n = rows.len();
    store.insert(rows).await?;
    Ok(IngestReport { doc_path: path_str, chunks: n })
}

pub async fn ingest_image(
    store: &LanceStore,
    api_key: &str,
    base_url: &str,
    model: &str,
    path: &Path,
) -> Result<IngestReport> {
    let desc = image::describe_image(api_key, base_url, model, path).await?;
    let chunks = chunker::chunk_text(&desc, 500, 50);
    let embs = embeddings::embed(chunks.clone())?;
    let path_str = path.to_string_lossy().to_string();
    let rows: Vec<DocRow> = chunks.iter().enumerate().zip(embs).map(|((i, t), e)| DocRow {
        id: Uuid::new_v4().to_string(),
        doc_path: path_str.clone(),
        doc_type: "image".into(),
        chunk_index: i as i32,
        text: t.clone(),
        page: None,
        embedding: e,
    }).collect();
    store.delete_by_doc_path(&path_str).await?;
    let n = rows.len();
    store.insert(rows).await?;
    Ok(IngestReport { doc_path: path_str, chunks: n })
}
