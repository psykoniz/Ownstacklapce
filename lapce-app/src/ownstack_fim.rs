//! Client-side state and helpers for AI inline autocompletion (Fill-in-the-Middle).
//!
//! The editor sends a [`FimRequest`](lapce_rpc::ownstack::OwnStackRpc::FimRequest)
//! with the code before (prefix) and after (suffix) the cursor; the agent replies
//! asynchronously with a [`FimResponse`]. Because the user may keep typing, every
//! request carries a monotonically increasing id and we only render the response
//! whose id matches the latest in-flight request *and* whose cursor offset has not
//! moved.

use std::path::PathBuf;

use floem::reactive::{RwSignal, Scope, SignalGet, SignalUpdate};

/// Maximum characters of context sent before the cursor.
const MAX_PREFIX_CHARS: usize = 2000;
/// Maximum characters of context sent after the cursor.
const MAX_SUFFIX_CHARS: usize = 1000;

/// An in-flight FIM request awaiting a response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FimPending {
    pub id: u64,
    pub path: PathBuf,
    pub offset: usize,
}

/// Reactive state for the FIM client, stored in `CommonData`.
#[derive(Clone)]
pub struct FimClientData {
    /// Whether AI autocompletion is enabled (user setting).
    pub enabled: RwSignal<bool>,
    /// Id generator for correlating requests with responses.
    pub next_id: RwSignal<u64>,
    /// The latest request still awaiting a reply.
    pub pending: RwSignal<Option<FimPending>>,
    /// Whether a workspace index build is in progress.
    pub indexing: RwSignal<bool>,
    /// Number of chunks in the semantic index (0 = not indexed).
    pub chunk_count: RwSignal<u64>,
}

impl FimClientData {
    pub fn new(cx: Scope) -> Self {
        Self {
            // On by default; cheap to ignore when no Ollama/API is present
            // because the agent simply returns no completion.
            enabled: cx.create_rw_signal(true),
            next_id: cx.create_rw_signal(0),
            pending: cx.create_rw_signal(None),
            indexing: cx.create_rw_signal(false),
            chunk_count: cx.create_rw_signal(0),
        }
    }

    /// Allocate the next request id and record it as pending.
    pub fn begin_request(&self, path: PathBuf, offset: usize) -> u64 {
        let id = self.next_id.get_untracked().wrapping_add(1);
        self.next_id.set(id);
        self.pending.set(Some(FimPending { id, path, offset }));
        id
    }

    /// Clear any pending request (e.g. on cursor move / cancel).
    pub fn clear(&self) {
        if self.pending.get_untracked().is_some() {
            self.pending.set(None);
        }
    }

    /// Returns the pending request iff `id` is still the latest one.
    pub fn take_if_current(&self, id: u64) -> Option<FimPending> {
        let pending = self.pending.get_untracked()?;
        if pending.id == id {
            self.pending.set(None);
            Some(pending)
        } else {
            None
        }
    }
}

/// Extract a bounded prefix/suffix window around a byte offset.
///
/// Boundaries are snapped to valid UTF-8 char boundaries so slicing never
/// panics on multi-byte text. Pure function — unit tested below.
pub fn window_around(
    text: &str,
    offset: usize,
    max_prefix: usize,
    max_suffix: usize,
) -> (String, String) {
    let len = text.len();
    let offset = offset.min(len);

    let prefix_start = snap_boundary(text, offset.saturating_sub(max_prefix));
    let suffix_end = snap_boundary(text, (offset + max_suffix).min(len));
    let offset = snap_boundary(text, offset);

    let prefix = text[prefix_start..offset].to_string();
    let suffix = text[offset..suffix_end].to_string();
    (prefix, suffix)
}

/// Default-window convenience wrapper.
pub fn default_window(text: &str, offset: usize) -> (String, String) {
    window_around(text, offset, MAX_PREFIX_CHARS, MAX_SUFFIX_CHARS)
}

/// Snap a byte index down to the nearest valid char boundary.
fn snap_boundary(text: &str, mut idx: usize) -> usize {
    if idx >= text.len() {
        return text.len();
    }
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Derive a coarse language id from a file path's extension. The FIM model does
/// not strictly require it, but it helps prompt framing for some backends.
pub fn language_of_path(path: &std::path::Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => "rust",
        Some("ts") | Some("tsx") => "typescript",
        Some("js") | Some("jsx") => "javascript",
        Some("py") => "python",
        Some("go") => "go",
        Some("java") => "java",
        Some("c") | Some("h") => "c",
        Some("cpp") | Some("cc") | Some("hpp") => "cpp",
        Some("rb") => "ruby",
        Some(other) => return other.to_string(),
        None => "text",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_splits_at_offset() {
        let (p, s) = window_around("abcdef", 3, 100, 100);
        assert_eq!(p, "abc");
        assert_eq!(s, "def");
    }

    #[test]
    fn window_respects_prefix_limit() {
        let text = "0123456789";
        let (p, s) = window_around(text, 8, 3, 100);
        assert_eq!(p, "567");
        assert_eq!(s, "89");
    }

    #[test]
    fn window_respects_suffix_limit() {
        let text = "0123456789";
        let (p, s) = window_around(text, 2, 100, 3);
        assert_eq!(p, "01");
        assert_eq!(s, "234");
    }

    #[test]
    fn window_offset_past_end_is_clamped() {
        let (p, s) = window_around("abc", 99, 100, 100);
        assert_eq!(p, "abc");
        assert_eq!(s, "");
    }

    #[test]
    fn window_offset_zero() {
        let (p, s) = window_around("abc", 0, 100, 100);
        assert_eq!(p, "");
        assert_eq!(s, "abc");
    }

    #[test]
    fn window_handles_multibyte_without_panicking() {
        // "café" — é is 2 bytes; offset between them must snap safely.
        let text = "café au lait";
        let (p, s) = window_around(text, 4, 100, 100);
        // Snap may land on 3 (before é) — the important part is no panic and
        // that prefix+suffix reconstruct the string.
        assert_eq!(format!("{p}{s}"), text);
    }

    #[test]
    fn language_detection() {
        assert_eq!(language_of_path(std::path::Path::new("a.rs")), "rust");
        assert_eq!(language_of_path(std::path::Path::new("a.tsx")), "typescript");
        assert_eq!(language_of_path(std::path::Path::new("a.unknownext")), "unknownext");
        assert_eq!(language_of_path(std::path::Path::new("Makefile")), "text");
    }

    #[test]
    fn pending_correlation() {
        let cx = Scope::new();
        let data = FimClientData::new(cx);
        let id1 = data.begin_request(PathBuf::from("a.rs"), 10);
        let id2 = data.begin_request(PathBuf::from("a.rs"), 12);
        assert_ne!(id1, id2);
        // Stale id is rejected.
        assert!(data.take_if_current(id1).is_none());
        // Current id is accepted exactly once.
        assert!(data.take_if_current(id2).is_some());
        assert!(data.take_if_current(id2).is_none());
    }
}
