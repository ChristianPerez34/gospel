use gospel_lib::corpus::{extractor::{Extractor, ExtractorLanguage}, Corpus};
use std::path::Path;

fn main() {
    println!("Testing corpus extraction...");
    
    let mut corpus = Corpus::new();
    let test_file = Path::new("/tmp/test_corpus/test.rs");
    
    println!("Creating extractor...");
    let mut extractor = Extractor::new(ExtractorLanguage::Rust).expect("Failed to create extractor");
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
