#[cfg(test)]
mod tests {
    use archdrop::server::handlers::hash_path;
    use std::collections::HashSet;

    #[test]
    fn test_hash_deterministic() {
        let path = "some/file/path.txt";
        let hash1 = hash_path(path);
        let hash2 = hash_path(path);
        assert_eq!(hash1, hash2, "Same path should hash to same value");
    }

    #[test]
    fn test_different_paths_different_hashes() {
        let paths = vec![
            "file1.txt",
            "file2.txt",
            "path/to/file.txt",
            "path/to/file2.txt",
            "similar_file.txt",
            "similar_file.TXT", // Case sensitivity
        ];

        let mut hashes = HashSet::new();
        for path in paths {
            let hash = hash_path(path);
            assert!(
                hashes.insert(hash.clone()),
                "Hash collision detected for path: {}",
                path
            );
        }
    }

    #[test]
    fn test_path_traversal_attempts_hash_differently() {
        // Even malicious paths should hash uniquely
        let h1 = hash_path("../../etc/passwd");
        let h2 = hash_path("etc/passwd");
        assert_ne!(h1, h2);
    }
}
