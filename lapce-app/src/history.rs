use std::path::PathBuf;

use lapce_core::buffer::Buffer;

/// A snapshot of a document at a specific version (e.g. the git "head").
///
/// Used by the editor to diff the working buffer against a reference version
/// and render the gutter change markers (see `Doc::trigger_head_change`).
/// Only the buffer text is needed for the rope diff; styling/layout of the
/// reference version is intentionally not retained.
#[derive(Clone)]
pub struct DocumentHistory {
    pub buffer: Buffer,
}

impl DocumentHistory {
    pub fn new(_path: PathBuf, _version: String, content: &str) -> Self {
        Self {
            buffer: Buffer::new(content),
        }
    }
}
