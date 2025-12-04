use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Component, Path};

//===============
// Path Handling
//===============
#[derive(Debug)]
pub enum PathValidationError {
    ContainsParentDir,
    AbsolutePath,
    InvalidComponent,
    NullByte,
    Empty,
}

impl fmt::Display for PathValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathValidationError::ContainsParentDir => {
                write!(f, "Path contains parent directory (..)")
            }
            PathValidationError::AbsolutePath => write!(f, "Path is absolute"),
            PathValidationError::InvalidComponent => write!(f, "Path contains invalid component"),
            PathValidationError::NullByte => write!(f, "Path contains null byte"),
            PathValidationError::Empty => write!(f, "Path is empty"),
        }
    }
}

impl std::error::Error for PathValidationError {}

// hash path for safe directory name
pub fn hash_path(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());

    // using first 16 chars (64 bits) for shorter directory names
    // with 16 still HIGHLY unlikely to collide
    format!("{:x}", hasher.finalize())[..16].to_string()
}

// Validate paths are safe to use
// Used for receiving.
// Since receive is writing entire path should be checked
// no: parent dir travel, abosolute paths, null bytes
pub fn validate_path(path: &str) -> Result<(), PathValidationError> {
    if path.is_empty() {
        return Err(PathValidationError::Empty);
    }

    // null bytes
    // rust uses C-style APIs so \0 can end str early
    if path.contains('\0') {
        return Err(PathValidationError::NullByte);
    }

    let path = Path::new(path);

    // Keep path in specified dir
    if path.is_absolute() {
        return Err(PathValidationError::AbsolutePath);
    }

    for component in path.components() {
        match component {
            Component::Normal(_) => continue,
            Component::ParentDir => return Err(PathValidationError::ContainsParentDir),
            Component::RootDir => return Err(PathValidationError::AbsolutePath),
            Component::CurDir => continue, // "./" is okay, just redundant
            Component::Prefix(_) => return Err(PathValidationError::InvalidComponent), // Windows
        }
    }

    Ok(())
}

// Used for send
// Only sending files so just the name should be valid
pub fn validate_filename(filename: &str) -> Result<(), PathValidationError> {
    if filename.is_empty() {
        return Err(PathValidationError::Empty);
    }
    // Check for null byte
    if filename.contains('\0') {
        return Err(PathValidationError::NullByte);
    }

    // Check for path traversal components (.., /, etc.)
    for component in Path::new(filename).components() {
        match component {
            Component::Normal(_) => continue,
            Component::ParentDir => return Err(PathValidationError::ContainsParentDir),
            Component::RootDir => return Err(PathValidationError::AbsolutePath),
            Component::CurDir => continue,
            Component::Prefix(_) => return Err(PathValidationError::InvalidComponent),
        }
    }

    Ok(())
}
