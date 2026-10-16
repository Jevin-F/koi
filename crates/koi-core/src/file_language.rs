//! Maps file names to the grammar names used by the editor.

/// A tree-sitter language name, or `text` for unknown files.
pub fn language_for(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension.to_ascii_lowercase().as_str() {
        "rs" => "rust",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "py" | "pyw" => "python",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cc" | "cpp" | "cxx" | "hpp" | "hxx" => "cpp",
        "swift" => "swift",
        "rb" => "ruby",
        "php" => "php",
        "sh" | "bash" | "zsh" => "bash",
        "html" | "htm" => "html",
        "css" | "scss" => "css",
        "json" | "jsonc" => "json",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "md" | "mdx" => "markdown",
        "sql" => "sql",
        "vue" => "html",
        _ => "text",
    }
}

#[cfg(test)]
mod tests {
    use super::language_for;

    #[test]
    fn common_extensions_use_grammar_names() {
        for (file, expected) in [
            ("src/main.rs", "rust"),
            ("App.TSX", "tsx"),
            ("main.jsx", "javascript"),
            ("run.py", "python"),
            ("config.yml", "yaml"),
            ("Makefile", "text"),
            ("notes.unknown", "text"),
        ] {
            assert_eq!(language_for(file), expected);
        }
    }
}
