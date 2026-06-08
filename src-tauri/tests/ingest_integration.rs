use cognitrade_lib::lance;
use cognitrade_lib::ingest::ingest_pdf;
use std::path::Path;

#[tokio::test]
#[ignore] // network-touching: downloads BGE-small on first run; manual smoke test
async fn ingest_pdf_into_lancedb() {
    let tmp = tempfile::tempdir().unwrap();
    let store = lance::open(tmp.path()).await.unwrap();
    let fixture = Path::new("tests/fixtures/sample.pdf");
    let report = ingest_pdf(&store, fixture).await.unwrap();
    assert!(report.chunks > 0);
    let hits = cognitrade_lib::retrieval::search(&store, "hello", 3).await.unwrap();
    assert!(!hits.is_empty());
}
