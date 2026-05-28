//! Gospel Corpus Sidecar - Standalone corpus operations via stdio JSON-RPC
//!
//! Usage:
//!   gospel-corpus build --dir /path/to/workspace
//!   gospel-corpus summary --dir /path/to/workspace
//!   gospel-corpus query --dir /path/to/workspace --id "symbol_name"
//!
//! JSON-RPC mode (for integration with main app):
//!   Echo JSON-RPC requests on stdin, write responses to stdout

use clap::{Parser, Subcommand};
use gospel_lib::corpus::{extractor::extract_directory, persistence::CorpusPersistence, Corpus};
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gospel-corpus")]
#[command(about = "Gospel Corpus Sidecar - standalone corpus operations")]
struct Cli {
    /// Enable JSON-RPC mode (stdio communication)
    #[arg(long)]
    jsonrpc: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a corpus from a directory
    Build {
        /// Directory to analyze
        #[arg(short, long)]
        dir: PathBuf,

        /// Ignore patterns (comma-separated)
        #[arg(short, long, default_value = "target,node_modules,.git,dist,build")]
        ignore: String,
    },
    /// Get corpus summary
    Summary {
        /// Directory containing corpus
        #[arg(short, long)]
        dir: PathBuf,
    },
    /// Query a specific node
    Query {
        /// Directory containing corpus
        #[arg(short, long)]
        dir: PathBuf,

        /// Node ID or name
        #[arg(short, long)]
        id: String,
    },
    /// Get neighbors of a node
    Neighbors {
        /// Directory containing corpus
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

// JSON-RPC types
#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: Option<u64>,
    method: String,
    params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    id: Option<u64>,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

fn main() {
    let cli = Cli::parse();

    if cli.jsonrpc {
        run_jsonrpc_mode();
    } else {
        run_cli_mode(cli.command);
    }
}

fn run_jsonrpc_mode() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Error reading stdin: {}", e);
                continue;
            }
        };

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let response = JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    id: None,
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32700,
                        message: format!("Parse error: {}", e),
                    }),
                };
                let _ = writeln!(stdout, "{}", serde_json::to_string(&response).unwrap());
                let _ = stdout.flush();
                continue;
            }
        };

        let response = handle_jsonrpc_request(request);

        let _ = writeln!(stdout, "{}", serde_json::to_string(&response).unwrap());
        let _ = stdout.flush();
    }
}

fn handle_jsonrpc_request(request: JsonRpcRequest) -> JsonRpcResponse {
    let result = match request.method.as_str() {
        "corpus.build" => handle_build(request.params),
        "corpus.summary" => handle_summary(request.params),
        "corpus.query" => handle_query(request.params),
        "corpus.neighbors" => handle_neighbors(request.params),
        _ => {
            return JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                }),
            }
        }
    };

    match result {
        Ok(value) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(value),
            error: None,
        },
        Err(e) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message: e,
            }),
        },
    }
}

fn handle_build(params: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let params = params.ok_or("Missing parameters")?;
    let params: BuildParams =
        serde_json::from_value(params).map_err(|e| format!("Invalid parameters: {}", e))?;

    if params.dir.as_os_str().is_empty() {
        return Err("Invalid or missing 'dir' parameter".into());
    }

    let ignore: Vec<&str> = params
        .ignore
        .as_deref()
        .unwrap_or("target,node_modules,.git,dist")
        .split(',')
        .map(|s| s.trim())
        .collect();

    let mut corpus = Corpus::new();
    extract_directory(&mut corpus, &params.dir, &ignore)
        .map_err(|e| format!("Extraction failed: {}", e))?;

    let summary = corpus.summary();

    let persistence = CorpusPersistence::new(&params.dir)
        .map_err(|e| format!("Failed to create persistence: {}", e))?;

    persistence
        .save(&corpus, &params.dir)
        .map_err(|e| format!("Failed to save: {}", e))?;

    Ok(serde_json::json!({
        "success": true,
        "file_count": summary.file_count,
        "symbol_count": summary.symbol_count,
        "relationship_count": summary.relationship_count,
        "message": format!("Built corpus with {} files and {} symbols", summary.file_count, summary.symbol_count)
    }))
}

fn handle_summary(params: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let params = params.ok_or("Missing parameters")?;
    let dir_str = params
        .get("dir")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'dir' parameter")?;

    let dir = PathBuf::from(dir_str);
    let persistence =
        CorpusPersistence::new(&dir).map_err(|e| format!("Failed to access corpus: {}", e))?;

    if !persistence.exists() {
        return Ok(serde_json::json!({
            "exists": false,
            "message": "No corpus exists for this workspace"
        }));
    }

    let corpus = persistence
        .load()
        .map_err(|e| format!("Failed to load corpus: {}", e))?;
    let summary = corpus.summary();

    Ok(serde_json::json!({
        "exists": true,
        "file_count": summary.file_count,
        "symbol_count": summary.symbol_count,
        "concept_count": summary.concept_count,
        "relationship_count": summary.relationship_count,
        "top_symbols": summary.top_symbols,
    }))
}

fn handle_query(params: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let params = params.ok_or("Missing parameters")?;
    let dir_str = params
        .get("dir")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'dir' parameter")?;
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'id' parameter")?;

    let dir = PathBuf::from(dir_str);
    let persistence =
        CorpusPersistence::new(&dir).map_err(|e| format!("Failed to access corpus: {}", e))?;

    if !persistence.exists() {
        return Ok(serde_json::json!({
            "found": false,
            "message": "No corpus exists"
        }));
    }

    let corpus = persistence
        .load()
        .map_err(|e| format!("Failed to load corpus: {}", e))?;

    let node = corpus
        .get_node(&id.to_string())
        .or_else(|| corpus.get_symbols_by_name(id).first().copied())
        .or_else(|| corpus.get_file_by_path(id));

    match node {
        Some(node) => {
            let neighbors = corpus.get_neighbors(&node.id);
            Ok(serde_json::json!({
                "found": true,
                "node": {
                    "id": node.id,
                    "name": node.name(),
                    "kind": node.kind_str(),
                },
                "neighbor_count": neighbors.len(),
            }))
        }
        None => Ok(serde_json::json!({
            "found": false,
            "message": format!("Node not found: {}", id)
        })),
    }
}

fn handle_neighbors(params: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
    let params = params.ok_or("Missing parameters")?;
    let dir_str = params
        .get("dir")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'dir' parameter")?;
    let id = params
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or("Missing 'id' parameter")?;

    let dir = PathBuf::from(dir_str);
    let persistence =
        CorpusPersistence::new(&dir).map_err(|e| format!("Failed to access corpus: {}", e))?;

    if !persistence.exists() {
        return Ok(serde_json::json!({
            "neighbors": []
        }));
    }

    let corpus = persistence
        .load()
        .map_err(|e| format!("Failed to load corpus: {}", e))?;

    let node = corpus
        .get_node(&id.to_string())
        .or_else(|| corpus.get_symbols_by_name(id).first().copied())
        .or_else(|| corpus.get_file_by_path(id));

    match node {
        Some(node) => {
            let neighbors = corpus.get_neighbors(&node.id);
            let neighbor_list: Vec<_> = neighbors
                .into_iter()
                .map(|(rel, n)| {
                    serde_json::json!({
                        "node_id": n.id,
                        "node_name": n.name(),
                        "node_kind": n.kind_str(),
                        "relationship": format!("{:?}", rel.relationship_type),
                        "confidence": rel.confidence.to_string(),
                        "direction": if rel.from_id == node.id { "outgoing" } else { "incoming" },
                    })
                })
                .collect();

            Ok(serde_json::json!({
                "neighbors": neighbor_list
            }))
        }
        None => Ok(serde_json::json!({
            "neighbors": [],
            "message": format!("Node not found: {}", id)
        })),
    }
}

#[derive(Debug, Deserialize, Default)]
struct BuildParams {
    dir: PathBuf,
    ignore: Option<String>,
}

fn run_cli_mode(command: Option<Commands>) {
    match command {
        Some(Commands::Build { dir, ignore }) => {
            println!("Building corpus for {:?}...", dir);

            let ignore: Vec<&str> = ignore.split(',').map(|s| s.trim()).collect();
            let mut corpus = Corpus::new();

            match extract_directory(&mut corpus, &dir, &ignore) {
                Ok(()) => {
                    let summary = corpus.summary();
                    println!("\n✓ Extraction complete:");
                    println!("  Files: {}", summary.file_count);
                    println!("  Symbols: {}", summary.symbol_count);
                    println!("  Relationships: {}", summary.relationship_count);

                    let persistence =
                        CorpusPersistence::new(&dir).expect("Failed to create persistence");

                    match persistence.save(&corpus, &dir) {
                        Ok(()) => {
                            println!("\n✓ Corpus saved to {:?}", dir.join(".gospel/corpus"));
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
        Some(Commands::Summary { dir }) => {
            let persistence = CorpusPersistence::new(&dir).expect("Failed to create persistence");

            if !persistence.exists() {
                println!("No corpus found in {:?}", dir);
                std::process::exit(1);
            }

            let corpus = persistence.load().expect("Failed to load corpus");
            let summary = corpus.summary();

            println!("Corpus Summary:");
            println!("  Files: {}", summary.file_count);
            println!("  Symbols: {}", summary.symbol_count);
            println!("  Concepts: {}", summary.concept_count);
            println!("  Relationships: {}", summary.relationship_count);

            if !summary.top_symbols.is_empty() {
                println!("\nTop symbols:");
                for (name, count) in summary.top_symbols.iter().take(5) {
                    println!("  {} ({} references)", name, count);
                }
            }
        }
        Some(Commands::Query { dir, id }) => {
            let persistence = CorpusPersistence::new(&dir).expect("Failed to create persistence");

            if !persistence.exists() {
                println!("No corpus found in {:?}", dir);
                std::process::exit(1);
            }

            let corpus = persistence.load().expect("Failed to load corpus");

            let node = corpus
                .get_node(&id)
                .or_else(|| corpus.get_symbols_by_name(&id).first().copied())
                .or_else(|| corpus.get_file_by_path(&id));

            match node {
                Some(node) => {
                    println!("Node: {}", node.id);
                    println!("  Name: {}", node.name());
                    println!("  Kind: {}", node.kind_str());

                    let neighbors = corpus.get_neighbors(&node.id);
                    if !neighbors.is_empty() {
                        println!("\n  Connections ({}):", neighbors.len());
                        for (rel, neighbor) in neighbors.iter().take(10) {
                            println!(
                                "    {} {} ({:?})",
                                if rel.from_id == node.id { "→" } else { "←" },
                                neighbor.name(),
                                rel.relationship_type
                            );
                        }
                    }
                }
                None => {
                    println!("Node not found: {}", id);
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Neighbors {
            dir,
            id,
            confidence,
        }) => {
            let persistence = CorpusPersistence::new(&dir).expect("Failed to create persistence");

            if !persistence.exists() {
                println!("No corpus found in {:?}", dir);
                std::process::exit(1);
            }

            let corpus = persistence.load().expect("Failed to load corpus");

            let min_conf = match confidence.trim().to_lowercase().as_str() {
                "high" => gospel_lib::corpus::Confidence::High,
                "medium" => gospel_lib::corpus::Confidence::Medium,
                _ => gospel_lib::corpus::Confidence::Low,
            };

            let node = corpus
                .get_node(&id)
                .or_else(|| corpus.get_symbols_by_name(&id).first().copied())
                .or_else(|| corpus.get_file_by_path(&id));

            match node {
                Some(node) => {
                    let neighbors = corpus.get_neighbors(&node.id);
                    println!(
                        "Neighbors of {} (min confidence: {:?}):",
                        node.name(),
                        min_conf
                    );

                    for (rel, neighbor) in neighbors {
                        if rel.confidence >= min_conf {
                            let direction = if rel.from_id == node.id { "→" } else { "←" };
                            println!(
                                "  {} {} ({:?}, {})",
                                direction,
                                neighbor.name(),
                                rel.relationship_type,
                                rel.confidence
                            );
                        }
                    }
                }
                None => {
                    println!("Node not found: {}", id);
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Files { dir }) => {
            let persistence = CorpusPersistence::new(&dir).expect("Failed to create persistence");

            if !persistence.exists() {
                println!("No corpus found in {:?}", dir);
                std::process::exit(1);
            }

            let corpus = persistence.load().expect("Failed to load corpus");
            let files: Vec<_> = corpus
                .nodes
                .values()
                .filter(|n| matches!(n.node_type, gospel_lib::corpus::NodeType::File { .. }))
                .collect();

            println!("Files in corpus ({} total):", files.len());
            for file in files {
                if let gospel_lib::corpus::NodeType::File {
                    path,
                    language,
                    line_count,
                } = &file.node_type
                {
                    println!("  {} ({}, {} lines)", path, language, line_count);
                }
            }
        }
        Some(Commands::Symbols { dir, filter }) => {
            let persistence = CorpusPersistence::new(&dir).expect("Failed to create persistence");

            if !persistence.exists() {
                println!("No corpus found in {:?}", dir);
                std::process::exit(1);
            }

            let corpus = persistence.load().expect("Failed to load corpus");
            let symbols: Vec<_> = corpus
                .nodes
                .values()
                .filter(|n| matches!(n.node_type, gospel_lib::corpus::NodeType::Symbol { .. }))
                .collect();

            let filtered: Vec<_> = if let Some(f) = &filter {
                symbols
                    .iter()
                    .filter(|s| s.name().to_lowercase().contains(&f.to_lowercase()))
                    .collect()
            } else {
                symbols.iter().collect()
            };

            println!(
                "Symbols in corpus ({} total, {} shown):",
                symbols.len(),
                filtered.len()
            );
            for symbol in filtered {
                if let gospel_lib::corpus::NodeType::Symbol {
                    name,
                    symbol_kind,
                    file_id,
                    ..
                } = &symbol.node_type
                {
                    println!(
                        "  {} ({}) in {}",
                        name,
                        format!("{:?}", symbol_kind).to_lowercase(),
                        file_id
                    );
                }
            }
        }
        None => {
            println!("Use --help for usage information");
            println!("Or use --jsonrpc for JSON-RPC mode");
        }
    }
}
