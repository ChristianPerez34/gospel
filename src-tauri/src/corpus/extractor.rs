//! Tree-sitter based structural extraction for Rust and TypeScript

use crate::corpus::{
    Confidence, Corpus, Node, NodeType, Relationship, RelationshipType, SymbolKind,
};
use std::path::Path;
use tree_sitter::{Parser, Tree};

/// Extract symbols and relationships from source files
pub struct Extractor {
    parser: Parser,
    language: ExtractorLanguage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtractorLanguage {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Jsx,
}

impl ExtractorLanguage {
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(ExtractorLanguage::Rust),
            "ts" => Some(ExtractorLanguage::TypeScript),
            "tsx" => Some(ExtractorLanguage::Tsx),
            "js" => Some(ExtractorLanguage::JavaScript),
            "jsx" => Some(ExtractorLanguage::Jsx),
            _ => None,
        }
    }

    pub fn language_name(&self) -> &'static str {
        match self {
            ExtractorLanguage::Rust => "rust",
            ExtractorLanguage::TypeScript => "typescript",
            ExtractorLanguage::Tsx => "tsx",
            ExtractorLanguage::JavaScript => "javascript",
            ExtractorLanguage::Jsx => "jsx",
        }
    }
}

impl Extractor {
    pub fn new(language: ExtractorLanguage) -> Result<Self, ExtractionError> {
        let mut parser = Parser::new();
        
        match language {
            ExtractorLanguage::Rust => {
                parser.set_language(&tree_sitter_rust::LANGUAGE.into())?;
            }
            ExtractorLanguage::TypeScript => {
                parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?;
            }
            ExtractorLanguage::Tsx => {
                parser.set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())?;
            }
            ExtractorLanguage::JavaScript => {
                // Use typescript language for JavaScript files too
                parser.set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())?;
            }
            ExtractorLanguage::Jsx => {
                parser.set_language(&tree_sitter_typescript::LANGUAGE_TSX.into())?;
            }
        }

        Ok(Self { parser, language })
    }

    /// Extract symbols from a file and add them to the corpus
    pub fn extract_file(&mut self, corpus: &mut Corpus, file_path: &Path) -> Result<(), ExtractionError> {
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| ExtractionError::IoError(e.to_string()))?;
        
        let line_count = content.lines().count();
        
        // Create file node
        let file_node = Node::new(NodeType::File {
            path: file_path.to_string_lossy().to_string(),
            language: self.language.language_name().to_string(),
            line_count,
        });
        let file_id = corpus.add_node(file_node);

        tracing::debug!("Parsing file with tree-sitter...");
        let tree = self.parser.parse(&content, None)
            .ok_or_else(|| ExtractionError::ParseError("Failed to parse file".to_string()))?;

        tracing::debug!("Parse successful, extracting symbols...");
        self.extract_symbols(corpus, &file_id, &tree, &content);
        self.extract_imports(corpus, &file_id, &tree, file_path, &content);

        tracing::debug!("Extraction complete for {:?}", file_path);
        Ok(())
    }

    fn extract_symbols(&self, corpus: &mut Corpus, file_id: &str, tree: &Tree, content: &str) {
        let root = tree.root_node();
        self.walk_node_recursive(corpus, file_id, root, content);
    }

    fn walk_node_recursive(
        &self,
        corpus: &mut Corpus,
        file_id: &str,
        node: tree_sitter::Node,
        content: &str,
    ) {
        let kind = node.kind();
        tracing::debug!(kind = kind, is_named = node.is_named(), "walk_node");

        if let Some(symbol_kind) = SymbolKind::from_ts_symbol(kind) {
            tracing::debug!(?symbol_kind, "symbol kind matched");
            if let Some(symbol_name) = self.get_symbol_name(node, content) {
                tracing::debug!(symbol_name = %symbol_name, "symbol name found");
                let documentation = self.get_documentation(node, content);
                let (start_line, end_line) = (node.start_position().row, node.end_position().row);
                
                let symbol_node = Node::new(NodeType::Symbol {
                    name: symbol_name.clone(),
                    symbol_kind,
                    file_id: file_id.to_string(),
                    start_line,
                    end_line,
                    documentation,
                });
                let symbol_id = corpus.add_node(symbol_node);
                
                // Add contains relationship
                corpus.add_relationship(Relationship::new(
                    file_id.to_string(),
                    symbol_id.clone(),
                    RelationshipType::Contains,
                    Confidence::High,
                ));

                // Extract references within this symbol
                self.extract_references(corpus, &symbol_id, node, content);
            }
        }
        
        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_node_recursive(corpus, file_id, child, content);
        }
    }

    fn get_symbol_name(&self, node: tree_sitter::Node, content: &str) -> Option<String> {
        // Different languages have different patterns for names
        match self.language {
            ExtractorLanguage::Rust => {
                // For Rust, look for identifier child
                let mut cursor = node.walk();
                if cursor.goto_first_child() {
                    loop {
                        let child = cursor.node();
                        if child.kind() == "identifier" {
                            return Some(child.utf8_text(content.as_bytes()).ok()?.to_string());
                        }
                        if !cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
                None
            }
            ExtractorLanguage::TypeScript | ExtractorLanguage::Tsx | ExtractorLanguage::JavaScript | ExtractorLanguage::Jsx => {
                // For TS/TSX/JS/JSX, look for name or identifier
                let mut cursor = node.walk();
                if cursor.goto_first_child() {
                    loop {
                        let child = cursor.node();
                        let kind = child.kind();
                        if kind == "identifier" || kind == "name" || kind == "type_identifier" || kind == "property_identifier" {
                            return Some(child.utf8_text(content.as_bytes()).ok()?.to_string());
                        }
                        if !cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
                None
            }
        }
    }

    fn get_documentation(&self, node: tree_sitter::Node, content: &str) -> Option<String> {
        // Look for documentation comments before the node
        let start_byte = node.start_byte();
        
        // Simple heuristic: look for /// or /** comments in Rust, or /** */ in TS
        let prefix = &content[..start_byte.min(content.len())];
        let lines: Vec<&str> = prefix.lines().rev().collect();
        
        let mut doc_lines = Vec::new();
        for line in lines {
            let trimmed = line.trim();
            if self.language == ExtractorLanguage::Rust && trimmed.starts_with("///") {
                doc_lines.push(trimmed[3..].trim());
            } else if trimmed.starts_with("*") && trimmed.ends_with("*/") {
                // End of block comment
                break;
            } else if self.language == ExtractorLanguage::TypeScript 
                || self.language == ExtractorLanguage::Tsx
            {
                if trimmed.starts_with("/**") {
                    break;
                }
                if trimmed.starts_with("*") {
                    doc_lines.push(trimmed[1..].trim());
                }
            } else {
                if !trimmed.is_empty() && !trimmed.starts_with("//") {
                    break;
                }
            }
        }
        
        if doc_lines.is_empty() {
            None
        } else {
            doc_lines.reverse();
            Some(doc_lines.join("\n"))
        }
    }

    fn extract_references(
        &self,
        corpus: &mut Corpus,
        symbol_id: &str,
        node: tree_sitter::Node,
        content: &str,
    ) {
        let mut references_to_add: Vec<(String, RelationshipType, Confidence)> = Vec::new();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            self.collect_references_from_node(child, content, &mut references_to_add);
        }

        for (name, rel_type, confidence) in references_to_add {
            if let Some(target_symbols) = corpus.symbol_index.get(&name) {
                let target_ids: Vec<_> = target_symbols.clone();
                for target_id in target_ids {
                    corpus.add_relationship(Relationship::new(
                        symbol_id.to_string(),
                        target_id,
                        rel_type.clone(),
                        confidence,
                    ));
                }
            }
        }
    }

    fn collect_references_from_node(
        &self,
        node: tree_sitter::Node,
        content: &str,
        refs: &mut Vec<(String, RelationshipType, Confidence)>,
    ) {
        let kind = node.kind();

        if kind == "call_expression" || kind == "function_call" {
            if let Some(called_name) = self.get_called_function_name(node, content) {
                refs.push((called_name, RelationshipType::Uses, Confidence::High));
            }
        }

        if kind == "type_identifier" || kind == "generic_type" {
            if let Ok(type_name) = node.utf8_text(content.as_bytes()) {
                refs.push((type_name.to_string(), RelationshipType::References, Confidence::Medium));
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_references_from_node(child, content, refs);
        }
    }

    fn get_called_function_name(&self, node: tree_sitter::Node, content: &str) -> Option<String> {
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            let child = cursor.node();
            if child.kind() == "identifier" || child.kind() == "property_identifier" {
                return Some(child.utf8_text(content.as_bytes()).ok()?.to_string());
            }
        }
        None
    }

    fn extract_imports(
        &self,
        corpus: &mut Corpus,
        file_id: &str,
        tree: &Tree,
        file_path: &Path,
        content: &str,
    ) {
        let mut import_targets: Vec<String> = Vec::new();
        let root = tree.root_node();
        let mut cursor = root.walk();

        for child in root.children(&mut cursor) {
            self.collect_imports_from_node(child, content, file_path, &mut import_targets);
        }

        let file_id_owned = file_id.to_string();
        for target_file in import_targets {
            if let Some(target_id) = corpus.file_index.get(&target_file) {
                let target_id = target_id.clone();
                corpus.add_relationship(Relationship::new(
                    file_id_owned.clone(),
                    target_id,
                    RelationshipType::Imports,
                    Confidence::High,
                ));
            }
        }
    }

    fn collect_imports_from_node(
        &self,
        node: tree_sitter::Node,
        content: &str,
        file_path: &Path,
        targets: &mut Vec<String>,
    ) {
        let kind = node.kind();

        if self.language == ExtractorLanguage::Rust && kind == "use_declaration" {
            if let Some(imported_path) = self.get_rust_use_path(node, content) {
                if let Some(target_file) = self.resolve_import(file_path, &imported_path) {
                    targets.push(target_file);
                }
            }
        }

        if (self.language == ExtractorLanguage::TypeScript
            || self.language == ExtractorLanguage::Tsx
            || self.language == ExtractorLanguage::JavaScript)
            && (kind == "import_statement" || kind == "import")
        {
            if let Some(import_path) = self.get_ts_import_path(node, content) {
                if let Some(target_file) = self.resolve_import(file_path, &import_path) {
                    targets.push(target_file);
                }
            }
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_imports_from_node(child, content, file_path, targets);
        }
    }

    fn get_rust_use_path(&self, node: tree_sitter::Node, content: &str) -> Option<String> {
        let content_bytes = content.as_bytes();
        let mut path_parts = Vec::new();
        
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.kind() == "identifier" || child.kind() == "scoped_identifier" {
                    if let Ok(text) = child.utf8_text(content_bytes) {
                        path_parts.push(text.to_string());
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        
        if path_parts.is_empty() {
            None
        } else {
            Some(path_parts.join("::"))
        }
    }

    fn get_ts_import_path(&self, node: tree_sitter::Node, content: &str) -> Option<String> {
        let content_bytes = content.as_bytes();
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.kind() == "string" {
                    if let Ok(text) = child.utf8_text(content_bytes) {
                        let path = text.trim_matches(|c| c == '"' || c == '\'');
                        return Some(path.to_string());
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        None
    }

    fn resolve_import(&self, from_file: &Path, import_path: &str) -> Option<String> {
        // Simple import resolution - in production this would be more sophisticated
        let from_dir = from_file.parent()?;
        
        // Handle relative imports
        let target_path = if import_path.starts_with("./") || import_path.starts_with("../") {
            from_dir.join(import_path)
        } else {
            // For absolute imports, would need to know the project root
            // For now, just try common patterns
            from_dir.join("src").join(import_path)
        };
        
        // Try different extensions
        for ext in &["rs", "ts", "tsx", "js", "jsx"] {
            let mut path_with_ext = target_path.clone();
            if let Some(parent) = path_with_ext.parent() {
                let file_name = path_with_ext.file_name()?.to_string_lossy().to_string();
                path_with_ext = parent.join(format!("{}.{}", file_name, ext));
            }
            
            if path_with_ext.exists() {
                return Some(path_with_ext.to_string_lossy().to_string());
            }
        }
        
        // Try with index files
        for index_name in &["index.rs", "index.ts", "index.tsx", "index.js", "index.jsx"] {
            let index_path = target_path.join(index_name);
            if index_path.exists() {
                return Some(index_path.to_string_lossy().to_string());
            }
        }
        
        None
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExtractionError {
    #[error("IO error: {0}")]
    IoError(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Tree-sitter error: {0}")]
    TreeSitter(#[from] tree_sitter::LanguageError),
}

/// Extract corpus from a directory
pub fn extract_directory(
    corpus: &mut Corpus,
    root_path: &Path,
    ignore_patterns: &[&str],
) -> Result<(), ExtractionError> {
    let mut files = Vec::new();
    collect_files(root_path, root_path, &mut files, ignore_patterns)?;
    
    for file_path in files {
        if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
            if let Some(language) = ExtractorLanguage::from_extension(ext) {
                if let Ok(mut extractor) = Extractor::new(language) {
                    let _ = extractor.extract_file(corpus, &file_path);
                }
            }
        }
    }
    
    Ok(())
}

fn collect_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<std::path::PathBuf>,
    ignore_patterns: &[&str],
) -> Result<(), ExtractionError> {
    if current.is_file() {
        files.push(current.to_path_buf());
        return Ok(());
    }
    
    if !current.is_dir() {
        return Ok(());
    }
    
    // Check ignore patterns
    if let Some(name) = current.file_name().and_then(|n| n.to_str()) {
        for pattern in ignore_patterns {
            if glob_match(pattern, name) {
                return Ok(());
            }
        }
    }
    
    let Ok(entries) = std::fs::read_dir(current) else {
        return Ok(());
    };
    
    for entry in entries.flatten() {
        let path = entry.path();
        // Skip hidden files and directories
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }
        // Recurse
        let _ = collect_files(root, &path, files, ignore_patterns);
    }
    
    Ok(())
}

fn glob_match(pattern: &str, text: &str) -> bool {
    // Simple glob matching for common patterns
    if pattern == "*" {
        return true;
    }
    
    if pattern.ends_with("*") {
        let prefix = &pattern[..pattern.len() - 1];
        return text.starts_with(prefix);
    }
    
    if pattern.starts_with("*") {
        let suffix = &pattern[1..];
        return text.ends_with(suffix);
    }
    
    pattern == text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_temp_rust_project() -> (TempDir, std::path::PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();
        
        // Create main.rs
        let main_rs = r#"
/// Main entry point
fn main() {
    println!("Hello!");
    helper();
}

/// Helper function
fn helper() {
    println!("Helper");
}

struct Data {
    value: i32,
}
"#;
        fs::write(src_dir.join("main.rs"), main_rs).unwrap();
        
        // Create lib.rs
        let lib_rs = r#"
/// Library module
pub mod utils;

pub fn library_func() {
    utils::helper();
}
"#;
        fs::write(src_dir.join("lib.rs"), lib_rs).unwrap();
        
        (temp_dir, src_dir)
    }

    #[test]
    fn test_extract_rust_file() {
        let mut corpus = Corpus::new();
        let mut extractor = Extractor::new(ExtractorLanguage::Rust).unwrap();
        
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, r#"
/// Test function
fn test() {
    println!("test");
}

struct TestStruct {
    field: i32,
}
"#).unwrap();
        
        let result = extractor.extract_file(&mut corpus, &test_file);
        assert!(result.is_ok());
        
        // Should have file node and symbol nodes
        assert!(corpus.nodes.len() >= 1);
        
        // Check for symbols
        let symbols: Vec<_> = corpus.nodes.values()
            .filter(|n| matches!(n.node_type, NodeType::Symbol { .. }))
            .collect();
        assert!(!symbols.is_empty());
    }

    #[test]
    fn test_extractor_language_from_extension() {
        assert_eq!(ExtractorLanguage::from_extension("rs"), Some(ExtractorLanguage::Rust));
        assert_eq!(ExtractorLanguage::from_extension("ts"), Some(ExtractorLanguage::TypeScript));
        assert_eq!(ExtractorLanguage::from_extension("tsx"), Some(ExtractorLanguage::Tsx));
        assert_eq!(ExtractorLanguage::from_extension("js"), Some(ExtractorLanguage::JavaScript));
        assert_eq!(ExtractorLanguage::from_extension("jsx"), Some(ExtractorLanguage::Jsx));
        assert_eq!(ExtractorLanguage::from_extension("py"), None);
    }
}
