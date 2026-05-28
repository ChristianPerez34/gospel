use gospel_lib::corpus::{
    extractor::{Extractor, ExtractorLanguage},
    Corpus,
};
use std::io::Write;
use tempfile::NamedTempFile;

fn main() {
    println!("Testing corpus extraction...");

    let mut corpus = Corpus::new();

    let mut temp_file = NamedTempFile::with_suffix(".rs").expect("Failed to create temp file");
    temp_file
        .write_all(b"fn example() { let x = 42; }")
        .expect("Failed to write to temp file");
    let test_file = temp_file.path();

    println!("Creating extractor...");
    let mut extractor =
        Extractor::new(ExtractorLanguage::Rust).expect("Failed to create extractor");
    println!("Extractor created!");

    println!("Extracting file: {:?}", test_file);
    match extractor.extract_file(&mut corpus, test_file) {
        Ok(()) => {
            println!("✓ Extraction successful!");
            println!("  Nodes: {}", corpus.nodes.len());
            println!("  Relationships: {}", corpus.relationships.len());

            let summary = corpus.summary();
            println!("  Files: {}", summary.file_count);
            println!("  Symbols: {}", summary.symbol_count);
        }
        Err(e) => {
            eprintln!("✗ Extraction failed: {}", e);
        }
    }
}
