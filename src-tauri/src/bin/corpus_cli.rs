//! Corpus CLI - standalone tool for testing the corpus library

use clap::{Parser, Subcommand};
use gospel_lib::corpus::{
    extractor::{extract_directory, ExtractorLanguage},
    persistence::CorpusPersistence,
    Corpus,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gospel-corpus")]
#[command(about = "Corpus CLI tool for codebase exploration", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a corpus from a directory
    Build {
        /// Directory to analyze
        #[arg(short, long)]
        dir: PathBuf,

        /// Output directory for corpus (defaults to .gospel/corpus in dir)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Ignore patterns (e.g., "target", "node_modules", "*.test.ts")
        #[arg(short, long, default_value = "target,node_modules,.git,dist")]
        ignore: String,
    },
    /// Query a corpus
    Query {
        /// Directory containing the corpus
        #[arg(short, long)]
        dir: PathBuf,

        /// Query SQL or natural language
        #[arg(short, long)]
        query: String,
    },
    /// Get summary of a corpus
    Summary {
        /// Directory containing the corpus
        #[arg(short, long)]
        dir: PathBuf,
    },
    /// Get details about a specific node
    Node {
        /// Directory containing the corpus
        #[arg(short, long)]
        dir: PathBuf,

        /// Node ID or name
        #[arg(short, long)]
        id: String,
    },
    /// Get neighbors of a node
    Neighbors {
        /// Directory containing the corpus
        #[arg(short, long)]
        dir: PathBuf,

        /// Node ID or name
        #[arg(short, long)]
        id: String,

        /// Minimum confidence (high, medium, low)
        #[arg(short, long, default_value = "low")]
        confidence: String,
    },
    /// List all files in the corpus
    Files {
        /// Directory containing the corpus
        #[arg(short, long)]
        dir: PathBuf,
    },
    /// List all symbols in the corpus
    Symbols {
        /// Directory containing the corpus
        #[arg(short, long)]
        dir: PathBuf,

        /// Filter by symbol name
        #[arg(short, long)]
        filter: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Build {
            dir,
            output,
            ignore,
        } => {
            let ignore_patterns: Vec<&str> = ignore.split(',').map(|s| s.trim()).collect();
            
            println!("Building corpus for {:?}...", dir);
            println!("Ignore patterns: {:?}", ignore_patterns);

            let mut corpus = Corpus::new();
            
            println!("Collecting files...");
            match extract_directory(&mut corpus, &dir, &ignore_patterns) {
                Ok(()) => {
                    let summary = corpus.summary();
                    println!("\nExtraction complete:");
                    println!("  Files: {}", summary.file_count);
                    println!("  Symbols: {}", summary.symbol_count);
                    println!("  Concepts: {}", summary.concept_count);
                    println!("  Relationships: {}", summary.relationship_count);

                    // Save corpus
                    let output_dir = output.unwrap_or_else(|| dir.clone());
                    println!("Saving corpus to {:?}...", output_dir.join(".gospel/corpus"));
                    let persistence = CorpusPersistence::new(&output_dir)
                        .expect("Failed to create persistence manager");
                    
                    match persistence.save(&corpus, &dir) {
                        Ok(()) => {
                            println!("\n✓ Corpus saved to {:?}", output_dir.join(".gospel/corpus"));
                        }
                        Err(e) => {
                            eprintln!("Error saving corpus: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error extracting corpus: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Query { dir, query } => {
            let persistence = CorpusPersistence::new(&dir)
                .expect("Failed to create persistence manager");
            
            if !persistence.exists() {
                eprintln!("No corpus found in {:?}", dir);
                std::process::exit(1);
            }

            // Simple SQL query interface
            match persistence.query(&query, &[]) {
                Ok(results) => {
                    println!("Query results ({} rows):", results.len());
                    for result in results {
                        println!("  [{}] {} ({})", result.id, result.name, result.kind);
                    }
                }
                Err(e) => {
                    eprintln!("Query error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Summary { dir } => {
            let persistence = CorpusPersistence::new(&dir)
                .expect("Failed to create persistence manager");
            
            if !persistence.exists() {
                eprintln!("No corpus found in {:?}", dir);
                std::process::exit(1);
            }

            match persistence.load() {
                Ok(corpus) => {
                    let summary = corpus.summary();
                    
                    println!("Corpus Summary:");
                    println!("  Files: {}", summary.file_count);
                    println!("  Symbols: {}", summary.symbol_count);
                    println!("  Concepts: {}", summary.concept_count);
                    println!("  Relationships: {}", summary.relationship_count);
                    
                    println!("\nRelationship breakdown:");
                    for (rel_type, count) in &summary.relationship_counts {
                        println!("  {}: {}", rel_type, count);
                    }
                    
                    if !summary.top_symbols.is_empty() {
                        println!("\nMost referenced symbols:");
                        for (symbol, count) in &summary.top_symbols {
                            println!("  {} ({} references)", symbol, count);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error loading corpus: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Node { dir, id } => {
            let persistence = CorpusPersistence::new(&dir)
                .expect("Failed to create persistence manager");
            
            if !persistence.exists() {
                eprintln!("No corpus found in {:?}", dir);
                std::process::exit(1);
            }

            match persistence.load() {
                Ok(corpus) => {
                    // Try to find by ID first, then by name
                    let node = corpus.get_node(&id)
                        .or_else(|| {
                            corpus.get_symbols_by_name(&id).first().copied()
                        });
                    
                    match node {
                        Some(node) => {
                            println!("Node: {}", node.id);
                            println!("  Name: {}", node.name());
                            println!("  Kind: {}", node.kind_str());
                            
                            match &node.node_type {
                                gospel_lib::corpus::NodeType::File { path, language, line_count } => {
                                    println!("  Type: File");
                                    println!("  Path: {}", path);
                                    println!("  Language: {}", language);
                                    println!("  Lines: {}", line_count);
                                }
                                gospel_lib::corpus::NodeType::Symbol { name, symbol_kind, file_id, start_line, end_line, documentation } => {
                                    println!("  Type: Symbol");
                                    println!("  Symbol Kind: {:?}", symbol_kind);
                                    println!("  Name: {}", name);
                                    println!("  File: {}", file_id);
                                    println!("  Lines: {}-{}", start_line, end_line);
                                    if let Some(doc) = documentation {
                                        println!("  Documentation: {}", doc);
                                    }
                                }
                                gospel_lib::corpus::NodeType::Concept { name, source, summary, keywords } => {
                                    println!("  Type: Concept");
                                    println!("  Source: {}", source);
                                    println!("  Summary: {}", summary);
                                    println!("  Keywords: {:?}", keywords);
                                }
                            }
                            
                            // Show neighbors
                            let neighbors = corpus.get_neighbors(&node.id);
                            if !neighbors.is_empty() {
                                println!("\n  Connections:");
                                for (rel, neighbor) in neighbors {
                                    println!("    -[{:?}]-> {} ({})", 
                                        rel.relationship_type, 
                                        neighbor.name(), 
                                        rel.confidence);
                                }
                            }
                        }
                        None => {
                            eprintln!("Node not found: {}", id);
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error loading corpus: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Neighbors { dir, id, confidence } => {
            let persistence = CorpusPersistence::new(&dir)
                .expect("Failed to create persistence manager");
            
            if !persistence.exists() {
                eprintln!("No corpus found in {:?}", dir);
                std::process::exit(1);
            }

            match persistence.load() {
                Ok(corpus) => {
                    let node = corpus.get_node(&id)
                        .or_else(|| corpus.get_symbols_by_name(&id).first().copied());
                    
                    match node {
                        Some(node) => {
                            let min_confidence = match confidence.to_lowercase().as_str() {
                                "high" => gospel_lib::corpus::Confidence::High,
                                "medium" => gospel_lib::corpus::Confidence::Medium,
                                _ => gospel_lib::corpus::Confidence::Low,
                            };
                            
                            let neighbors = corpus.get_neighbors(&node.id);
                            println!("Neighbors of {} (min confidence: {:?}):", node.name(), min_confidence);
                            
                            for (rel, neighbor) in neighbors {
                                if rel.confidence >= min_confidence {
                                    let direction = if rel.from_id == node.id { "→" } else { "←" };
                                    println!("  {} {} ({:?}) {}", 
                                        direction,
                                        neighbor.name(),
                                        rel.relationship_type,
                                        rel.confidence);
                                }
                            }
                        }
                        None => {
                            eprintln!("Node not found: {}", id);
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error loading corpus: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Files { dir } => {
            let persistence = CorpusPersistence::new(&dir)
                .expect("Failed to create persistence manager");
            
            if !persistence.exists() {
                eprintln!("No corpus found in {:?}", dir);
                std::process::exit(1);
            }

            match persistence.load() {
                Ok(corpus) => {
                    let files: Vec<_> = corpus.nodes.values()
                        .filter(|n| matches!(n.node_type, gospel_lib::corpus::NodeType::File { .. }))
                        .collect();
                    
                    println!("Files in corpus ({} total):", files.len());
                    for file in files {
                        if let gospel_lib::corpus::NodeType::File { path, language, line_count } = &file.node_type {
                            println!("  {} ({}, {} lines)", path, language, line_count);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error loading corpus: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Symbols { dir, filter } => {
            let persistence = CorpusPersistence::new(&dir)
                .expect("Failed to create persistence manager");
            
            if !persistence.exists() {
                eprintln!("No corpus found in {:?}", dir);
                std::process::exit(1);
            }

            match persistence.load() {
                Ok(corpus) => {
                    let symbols: Vec<_> = corpus.nodes.values()
                        .filter(|n| matches!(n.node_type, gospel_lib::corpus::NodeType::Symbol { .. }))
                        .collect();
                    
                    let filtered: Vec<_> = if let Some(f) = &filter {
                        symbols.iter()
                            .filter(|s| s.name().contains(f))
                            .collect()
                    } else {
                        symbols.iter().collect()
                    };
                    
                    println!("Symbols in corpus ({} total, {} shown):", symbols.len(), filtered.len());
                    for symbol in filtered {
                        if let gospel_lib::corpus::NodeType::Symbol { name, symbol_kind, file_id, .. } = &symbol.node_type {
                            println!("  {} ({}) in {}", name, format!("{:?}", symbol_kind).to_lowercase(), file_id);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Error loading corpus: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}
