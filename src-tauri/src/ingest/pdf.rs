use crate::error::{AppError, Result};
use std::path::Path;

pub struct PdfText {
    pub full_text: String,
}

pub fn extract(path: &Path) -> Result<PdfText> {
    let bytes = std::fs::read(path)?;
    let text = pdf_extract::extract_text_from_mem(&bytes)
        .map_err(|e| AppError::Internal(format!("pdf extract: {}", e)))?;
    Ok(PdfText { full_text: text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_missing_file_errors() {
        let r = extract(Path::new("definitely-not-here.pdf"));
        assert!(r.is_err());
    }
}
