use crate::error::{AppError, Result};
use crate::ingest::embeddings::EMBED_DIM;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct DocRow {
    pub id: String,
    pub doc_path: String,
    pub doc_type: String,
    pub chunk_index: i32,
    pub text: String,
    pub page: Option<i32>,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocInfo {
    pub doc_path: String,
    pub doc_type: String,
    pub chunks: i64,
}

pub struct LanceStore {
    inner: Arc<Mutex<Connection>>,
}

impl Clone for LanceStore {
    fn clone(&self) -> Self {
        Self { inner: self.inner.clone() }
    }
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS doc_chunks (
  id TEXT PRIMARY KEY,
  doc_path TEXT NOT NULL,
  doc_type TEXT NOT NULL,
  chunk_index INTEGER NOT NULL,
  text TEXT NOT NULL,
  page INTEGER,
  embedding BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_doc_chunks_path ON doc_chunks(doc_path);
"#;

pub async fn open(path: &Path) -> Result<LanceStore> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    conn.execute_batch(SCHEMA)?;
    Ok(LanceStore { inner: Arc::new(Mutex::new(conn)) })
}

fn embedding_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn bytes_to_embedding(b: &[u8]) -> Vec<f32> {
    if b.len() % 4 != 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(b.len() / 4);
    for chunk in b.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    out
}

/// Return a unit-length copy of `v` (L2 norm == 1). If `v` is all-zeros (or
/// empty), returns a zero vector of the same length so it scores 0 and is
/// effectively skipped during ranking.
fn normalize(v: &[f32]) -> Vec<f32> {
    let mut norm = 0.0f32;
    for &x in v {
        norm += x * x;
    }
    let norm = norm.sqrt();
    if norm == 0.0 {
        return vec![0.0; v.len()];
    }
    v.iter().map(|&x| x / norm).collect()
}

/// Dot product over two equal-length vectors. When both inputs are unit
/// vectors this equals cosine similarity. If lengths differ or either side is
/// empty/all-zeros (norm encoded as zeros), the result is 0.
fn dot(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut acc = 0.0f32;
    for i in 0..a.len() {
        acc += a[i] * b[i];
    }
    acc
}

impl LanceStore {
    pub async fn ensure_table(&self) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute_batch(SCHEMA)?;
        Ok(())
    }

    pub async fn insert(&self, rows: Vec<DocRow>) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut conn = self.inner.lock().unwrap();
        let tx = conn.transaction()?;
        for r in &rows {
            if r.embedding.len() != EMBED_DIM {
                return Err(AppError::Invalid(format!(
                    "embedding dim mismatch: got {}, expected {}",
                    r.embedding.len(),
                    EMBED_DIM
                )));
            }
            // Store unit-normalized embeddings so search can score via a plain
            // dot product (== cosine for unit vectors) without recomputing the
            // stored vector's norm on every query.
            let unit = normalize(&r.embedding);
            tx.execute(
                "INSERT OR REPLACE INTO doc_chunks(id, doc_path, doc_type, chunk_index, text, page, embedding) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                rusqlite::params![
                    r.id,
                    r.doc_path,
                    r.doc_type,
                    r.chunk_index,
                    r.text,
                    r.page,
                    embedding_to_bytes(&unit),
                ],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub async fn search(&self, query_emb: Vec<f32>, k: usize) -> Result<Vec<DocRow>> {
        if query_emb.len() != EMBED_DIM {
            return Ok(Vec::new());
        }
        // Normalize the query once; stored vectors are already unit-length, so
        // a dot product reproduces cosine ranking exactly.
        let query_unit = normalize(&query_emb);
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, doc_path, doc_type, chunk_index, text, page, embedding FROM doc_chunks",
        )?;
        let rows_iter = stmt.query_map([], |r| {
            let blob: Vec<u8> = r.get(6)?;
            Ok(DocRow {
                id: r.get(0)?,
                doc_path: r.get(1)?,
                doc_type: r.get(2)?,
                chunk_index: r.get(3)?,
                text: r.get(4)?,
                page: r.get(5)?,
                embedding: bytes_to_embedding(&blob),
            })
        })?;
        let mut scored: Vec<(f32, DocRow)> = Vec::new();
        for row in rows_iter {
            let row = row?;
            let s = dot(&query_unit, &row.embedding);
            scored.push((s, row));
        }
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(k);
        Ok(scored.into_iter().map(|(_, r)| r).collect())
    }

    pub async fn delete_by_doc_path(&self, doc_path: &str) -> Result<()> {
        let conn = self.inner.lock().unwrap();
        conn.execute("DELETE FROM doc_chunks WHERE doc_path = ?1", [doc_path])?;
        Ok(())
    }

    pub async fn list_docs(&self) -> Result<Vec<DocInfo>> {
        let conn = self.inner.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT doc_path, doc_type, COUNT(*) AS chunks FROM doc_chunks GROUP BY doc_path, doc_type ORDER BY doc_path",
        )?;
        let rows_iter = stmt.query_map([], |r| {
            Ok(DocInfo {
                doc_path: r.get(0)?,
                doc_type: r.get(1)?,
                chunks: r.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows_iter {
            out.push(row?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> LanceStore {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        LanceStore { inner: Arc::new(Mutex::new(conn)) }
    }

    fn row(id: &str, doc_path: &str, doc_type: &str, idx: i32, emb: Vec<f32>) -> DocRow {
        DocRow {
            id: id.to_string(),
            doc_path: doc_path.to_string(),
            doc_type: doc_type.to_string(),
            chunk_index: idx,
            text: format!("text-{id}"),
            page: None,
            embedding: emb,
        }
    }

    /// Build a unit-length EMBED_DIM vector pointing mostly along axis `axis`,
    /// with a small offset so different rows are distinguishable.
    fn vec_along(axis: usize, offset: f32) -> Vec<f32> {
        let mut v = vec![offset; EMBED_DIM];
        v[axis % EMBED_DIM] += 1.0;
        v
    }

    /// Old reference implementation: full cosine over raw stored vectors.
    fn cosine_ref(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let mut d = 0.0f32;
        let mut na = 0.0f32;
        let mut nb = 0.0f32;
        for i in 0..a.len() {
            d += a[i] * b[i];
            na += a[i] * a[i];
            nb += b[i] * b[i];
        }
        let denom = na.sqrt() * nb.sqrt();
        if denom == 0.0 {
            0.0
        } else {
            d / denom
        }
    }

    #[tokio::test]
    async fn list_docs_groups_and_counts() {
        let s = store();
        s.insert(vec![
            row("a1", "/docs/a.pdf", "pdf", 0, vec_along(0, 0.0)),
            row("a2", "/docs/a.pdf", "pdf", 1, vec_along(1, 0.0)),
            row("a3", "/docs/a.pdf", "pdf", 2, vec_along(2, 0.0)),
            row("b1", "/docs/b.txt", "text", 0, vec_along(3, 0.0)),
            row("b2", "/docs/b.txt", "text", 1, vec_along(4, 0.0)),
        ])
        .await
        .unwrap();

        let docs = s.list_docs().await.unwrap();
        assert_eq!(docs.len(), 2);
        // ORDER BY doc_path => a.pdf before b.txt
        assert_eq!(docs[0].doc_path, "/docs/a.pdf");
        assert_eq!(docs[0].doc_type, "pdf");
        assert_eq!(docs[0].chunks, 3);
        assert_eq!(docs[1].doc_path, "/docs/b.txt");
        assert_eq!(docs[1].doc_type, "text");
        assert_eq!(docs[1].chunks, 2);
    }

    #[tokio::test]
    async fn normalization_preserves_topk_ranking() {
        // Raw (non-unit) vectors with distinct directions and magnitudes.
        let raws: Vec<Vec<f32>> = (0..6)
            .map(|i| {
                let mut v = vec_along(i, 0.1 * i as f32);
                // scale to vary magnitude; cosine must be magnitude-invariant.
                for x in v.iter_mut() {
                    *x *= (i as f32 + 1.0) * 2.0;
                }
                v
            })
            .collect();

        let s = store();
        let rows: Vec<DocRow> = raws
            .iter()
            .enumerate()
            .map(|(i, e)| row(&format!("r{i}"), "/d.pdf", "pdf", i as i32, e.clone()))
            .collect();
        s.insert(rows).await.unwrap();

        let query = vec_along(2, 0.05);

        // Reference ranking using full cosine over the ORIGINAL raw vectors.
        let mut ref_scored: Vec<(usize, f32)> = raws
            .iter()
            .enumerate()
            .map(|(i, e)| (i, cosine_ref(&query, e)))
            .collect();
        ref_scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let ref_order: Vec<String> =
            ref_scored.iter().map(|(i, _)| format!("r{i}")).collect();

        // New ranking via normalized-store + dot-product search.
        let got = s.search(query, raws.len()).await.unwrap();
        let got_order: Vec<String> = got.iter().map(|r| r.id.clone()).collect();

        assert_eq!(got_order, ref_order, "dot-product ranking must match cosine");
    }

    #[tokio::test]
    async fn search_returns_nearest_first() {
        let s = store();
        s.insert(vec![
            row("near", "/d.pdf", "pdf", 0, vec_along(0, 0.0)),
            row("mid", "/d.pdf", "pdf", 1, vec_along(1, 0.0)),
            row("far", "/d.pdf", "pdf", 2, vec_along(2, 0.0)),
        ])
        .await
        .unwrap();

        // Query aligned with the "near" row's axis.
        let query = vec_along(0, 0.0);
        let got = s.search(query, 3).await.unwrap();
        assert_eq!(got.len(), 3);
        assert_eq!(got[0].id, "near");
    }

    #[tokio::test]
    async fn all_zero_vector_scores_zero_and_does_not_crash() {
        let s = store();
        s.insert(vec![
            row("zero", "/d.pdf", "pdf", 0, vec![0.0; EMBED_DIM]),
            row("real", "/d.pdf", "pdf", 1, vec_along(0, 0.0)),
        ])
        .await
        .unwrap();

        let query = vec_along(0, 0.0);
        let got = s.search(query, 2).await.unwrap();
        // The real row must outrank the all-zero row.
        assert_eq!(got[0].id, "real");
    }
}
