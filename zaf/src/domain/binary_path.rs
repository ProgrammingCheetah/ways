//! The path of a running binary, as the system reports it.

use std::path::PathBuf;

/// `path` without the ` (deleted)` the system adds to the path of a running image whose file was
/// replaced or removed.
pub fn without_deleted(path: PathBuf) -> PathBuf {
    match path
        .to_str()
        .and_then(|text| text.strip_suffix(" (deleted)"))
    {
        Some(path) => PathBuf::from(path),
        None => path,
    }
}
