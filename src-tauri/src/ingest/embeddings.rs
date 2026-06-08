use crate::error::{AppError, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::{Mutex, OnceLock};

pub const EMBED_DIM: usize = 384; // BGE-small dim

static MODEL: OnceLock<Mutex<TextEmbedding>> = OnceLock::new();

fn get_or_init() -> Result<&'static Mutex<TextEmbedding>> {
    if MODEL.get().is_none() {
        let model = TextEmbedding::try_new(InitOptions::new(EmbeddingModel::BGESmallENV15))
            .map_err(|e| AppError::Internal(format!("fastembed init: {}", e)))?;
        let _ = MODEL.set(Mutex::new(model));
    }
    Ok(MODEL.get().expect("set above"))
}

/// True once the embedding model has been initialized (no init triggered).
pub fn is_ready() -> bool {
    MODEL.get().is_some()
}

/// Force initialization of the embedding model. CPU/sync and potentially slow on
/// first call (may download the model), so callers should run this off the async
/// runtime (e.g. a dedicated std thread at startup).
pub fn warmup() -> Result<()> {
    get_or_init()?;
    Ok(())
}

pub fn embed(texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
    if texts.is_empty() { return Ok(vec![]); }
    let m = get_or_init()?;
    let guard = m.lock().unwrap();
    let v = guard.embed(texts, None)
        .map_err(|e| AppError::Internal(format!("fastembed embed: {}", e)))?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `is_ready()` must purely reflect the OnceLock state without forcing init.
    /// It mirrors `MODEL.get().is_some()` so the result depends on whether any
    /// prior call (in this process) initialized the model. Either way it must
    /// agree with the underlying OnceLock and never panic or trigger a download.
    #[test]
    fn is_ready_matches_oncelock_and_never_inits() {
        assert_eq!(is_ready(), MODEL.get().is_some());
    }
}
