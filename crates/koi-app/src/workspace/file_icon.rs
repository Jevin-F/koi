//! Icons for the file tree, chosen by file name and extension.

use gpui_kit::assets::IconName;

/// The icon of a folder, open or closed.
pub(super) fn folder_icon(expanded: bool) -> IconName {
    if expanded {
        IconName::FolderOpen
    } else {
        IconName::Folder
    }
}

/// The icon of a file, from its whole name for well-known files, else from
/// its extension.
pub(super) fn file_icon(name: &str) -> IconName {
    let name = name.to_lowercase();
    match name.as_str() {
        "cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml" | "poetry.lock" => {
            return IconName::FileLock;
        }
        "dockerfile" | "makefile" | "justfile" | ".gitignore" | ".gitattributes"
        | ".dockerignore" | ".editorconfig" | ".env" => return IconName::FileCog,
        _ => {}
    }
    let extension = name.rsplit_once('.').map_or("", |(_, extension)| extension);
    match extension {
        "rs" | "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "py" | "go" | "java" | "kt"
        | "swift" | "c" | "h" | "cc" | "cpp" | "hpp" | "cs" | "rb" | "php" | "scala" | "lua"
        | "zig" | "dart" | "ex" | "exs" | "vue" | "svelte" | "html" | "css" | "scss" | "less"
        | "sql" => IconName::FileCode,
        "json" | "jsonc" | "json5" => IconName::FileBraces,
        "yaml" | "yml" | "toml" | "ini" | "conf" | "cfg" | "properties" | "plist" | "xml" => {
            IconName::FileCog
        }
        "md" | "mdx" | "txt" | "rst" | "adoc" => IconName::FileText,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" | "icns" | "bmp" | "avif" => {
            IconName::FileImage
        }
        "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" => IconName::FileTerminal,
        "lock" => IconName::FileLock,
        "csv" | "tsv" | "xls" | "xlsx" => IconName::FileSpreadsheet,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "jar" => IconName::FileArchive,
        "ttf" | "otf" | "woff" | "woff2" => IconName::FileType,
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => IconName::FileMusic,
        "mp4" | "mov" | "webm" | "mkv" | "avi" => IconName::FileVideoCamera,
        "pem" | "key" | "crt" | "cer" | "p12" => IconName::FileKey,
        "diff" | "patch" => IconName::FileDiff,
        _ => IconName::File,
    }
}

#[cfg(test)]
mod tests {
    use super::{IconName, file_icon, folder_icon};

    #[test]
    fn well_known_names_come_before_extensions() {
        assert_eq!(file_icon("Cargo.lock"), IconName::FileLock);
        assert_eq!(file_icon("package-lock.json"), IconName::FileLock);
        assert_eq!(file_icon("Dockerfile"), IconName::FileCog);
    }

    #[test]
    fn extensions_choose_the_kind() {
        assert_eq!(file_icon("main.rs"), IconName::FileCode);
        assert_eq!(file_icon("App.TSX"), IconName::FileCode);
        assert_eq!(file_icon("tsconfig.json"), IconName::FileBraces);
        assert_eq!(file_icon("README.md"), IconName::FileText);
        assert_eq!(file_icon("logo.png"), IconName::FileImage);
        assert_eq!(file_icon("noext"), IconName::File);
    }

    #[test]
    fn folders_open_and_close() {
        assert_eq!(folder_icon(true), IconName::FolderOpen);
        assert_eq!(folder_icon(false), IconName::Folder);
    }
}
