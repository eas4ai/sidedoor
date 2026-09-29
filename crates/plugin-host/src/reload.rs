use std::{fs, path::Path};
/// A value that changes whenever a file in the plugin changes, so the host
/// can reload it on save. Skips `node_modules` and hidden files.
pub fn fingerprint(dir: &Path) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};
    fn visit(dir: &Path, depth: usize, hasher: &mut DefaultHasher) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                if depth > 0 {
                    visit(&entry.path(), depth - 1, hasher);
                }
            } else {
                name.hash(hasher);
                metadata.len().hash(hasher);
                metadata.modified().ok().hash(hasher);
            }
        }
    }
    let mut hasher = DefaultHasher::new();
    visit(dir, 4, &mut hasher);
    hasher.finish()
}
