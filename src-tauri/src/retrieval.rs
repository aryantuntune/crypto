use crate::error::Result;
use crate::ingest::embeddings;
use crate::lance::{DocRow, LanceStore};

pub async fn search(store: &LanceStore, query: &str, k: usize) -> Result<Vec<DocRow>> {
    let emb = embeddings::embed(vec![query.to_string()])?;
    let q = emb.into_iter().next().unwrap_or_default();
    if q.is_empty() { return Ok(vec![]); }
    store.search(q, k).await
}
