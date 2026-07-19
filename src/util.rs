use std::path::Path;

pub fn pascal_to_kebab_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('-');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

pub fn format_mb(bytes: u64) -> String {
    let mb = (bytes as f64) / 1024.0 / 1024.0;
    format!("{:.2} MB", mb)
}

pub fn dir_size_bytes(path: &Path) -> u64 {
    let mut total: u64 = 0;
    if path.is_file() {
        if let Ok(meta) = std::fs::metadata(path) {
            return meta.len();
        }
        return 0;
    }

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                if let Ok(meta) = std::fs::metadata(&p) {
                    total += meta.len();
                }
            } else if p.is_dir() {
                total += dir_size_bytes(&p);
            }
        }
    }
    total
}
