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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_pascal_to_kebab_case() {
        assert_eq!(pascal_to_kebab_case("MyApp"), "my-app");
        assert_eq!(pascal_to_kebab_case("HTMLParser"), "h-t-m-l-parser");
        assert_eq!(pascal_to_kebab_case("already-kebab"), "already-kebab");
        assert_eq!(pascal_to_kebab_case("Simple"), "simple");
        assert_eq!(pascal_to_kebab_case(""), "");
    }

    #[test]
    fn test_format_mb() {
        assert_eq!(format_mb(0), "0.00 MB");
        assert_eq!(format_mb(1024 * 1024), "1.00 MB");
        assert_eq!(format_mb(1024 * 1024 * 5 + 512 * 1024), "5.50 MB");
    }

    #[test]
    fn test_dir_size_bytes_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.txt");
        fs::write(&file, "hello world").unwrap();
        assert_eq!(dir_size_bytes(&file), 11);
    }

    #[test]
    fn test_dir_size_bytes_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(dir_size_bytes(dir.path()), 0);
    }

    #[test]
    fn test_dir_size_bytes_nested() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("a.txt"), "hello").unwrap();
        fs::write(sub.join("b.txt"), "world!").unwrap();
        assert_eq!(dir_size_bytes(dir.path()), 11);
    }
}
