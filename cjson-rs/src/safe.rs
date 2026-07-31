//! # Safe Memory Management — `#![forbid(unsafe_code)]`
//!
//! This module contains **zero** `unsafe` blocks.  All memory management uses
//! standard Rust ownership (`Box`, `String`, `Vec`) and the `Drop` trait.
//!
//! The C library's `cJSON_InitHooks` mechanism (custom allocator injection via
//! raw C function pointers) is intentionally rejected here: we rely on Rust's
//! global allocator exclusively.
//!
//! ## Architecture
//!
//! ```text
//!  C caller
//!    │
//!    ▼
//!  ffi_impl.rs  (thin unsafe boundary — pointer↔reference conversion only)
//!    │
//!    ▼
//!  safe.rs      (this file — #![forbid(unsafe_code)])
//!    │
//!    ▼
//!  Rust global allocator (jemalloc / system / whatever is linked)
//! ```

#![forbid(unsafe_code)]

use std::fmt;

// ---------------------------------------------------------------------------
// Allocator hook policy
// ---------------------------------------------------------------------------

/// Records whether a caller attempted to install custom C allocator hooks.
/// We never actually use them — this is purely for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPolicy {
    /// No hooks were ever requested (or hooks were reset to NULL).
    RustDefault,
    /// A caller tried to install custom hooks; we logged a warning and
    /// continued using the Rust allocator.
    IgnoredCustomHooks,
}

impl fmt::Display for HookPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HookPolicy::RustDefault => write!(f, "RustDefault (standard Rust allocator)"),
            HookPolicy::IgnoredCustomHooks => {
                write!(f, "IgnoredCustomHooks (C hooks were silently rejected)")
            }
        }
    }
}

/// Log a warning to stderr when C code tries to install custom allocators.
///
/// This is a **safe** function — no raw pointers, no FFI, no `unsafe`.
/// The `ffi_impl` layer calls this after extracting the relevant information
/// from the raw `cJSON_Hooks*` pointer.
pub fn warn_hooks_ignored(has_malloc: bool, has_free: bool) -> HookPolicy {
    if !has_malloc && !has_free {
        // Caller passed NULL hooks or both fields are NULL → this is a reset.
        // Nothing to warn about.
        return HookPolicy::RustDefault;
    }

    let which = match (has_malloc, has_free) {
        (true, true) => "malloc_fn and free_fn",
        (true, false) => "malloc_fn",
        (false, true) => "free_fn",
        (false, false) => unreachable!(), // handled above
    };

    eprintln!(
        "[cjson-rs] WARNING: cJSON_InitHooks() called with custom {which}. \
         The Rust implementation does NOT support custom C allocators — \
         memory is managed exclusively by Rust's global allocator. \
         The custom hooks have been safely ignored."
    );

    HookPolicy::IgnoredCustomHooks
}

// ---------------------------------------------------------------------------
// Safe node deallocation descriptor
//
// The FFI layer (`ffi_impl.rs`) walks the raw `*mut cJSON` linked list and
// builds a `DeallocPlan` describing what needs to be freed.  This module
// then executes that plan using safe Rust `Drop` semantics — no raw
// pointer manipulation here.
// ---------------------------------------------------------------------------

/// Describes a single cJSON node's owned resources that need deallocation.
///
/// Built by `ffi_impl.rs` from raw pointer inspection, consumed by safe code.
/// All `Vec<u8>` fields were constructed via `Vec::from_raw_parts` in the
/// unsafe boundary — by the time they arrive here they are ordinary owned
/// Rust `Vec`s and dropping them is fully safe.
pub struct NodeResources {
    /// The heap allocation for the `cJSON` struct itself.
    /// Wrapped in Option so we can `.take()` it for explicit drop ordering.
    pub node_box: Option<BoxedNode>,

    /// Owned `valuestring` bytes (including NUL terminator).
    /// `None` if the node is a reference (`cJSON_IsReference`) or the field was NULL.
    pub owned_valuestring: Option<Vec<u8>>,

    /// Owned `string` (key-name) bytes (including NUL terminator).
    /// `None` if `cJSON_StringIsConst` was set or the field was NULL.
    pub owned_keystring: Option<Vec<u8>>,
}

/// Opaque wrapper around a `Box<[u8]>` representing the raw `cJSON` struct
/// allocation.  When this is dropped, the memory is freed.
///
/// The FFI boundary creates this via `Box::from_raw`; once it lands here it's
/// a safe, owned Rust value.
pub struct BoxedNode {
    /// The actual heap allocation.  Size == `mem::size_of::<cJSON>()`.
    pub(crate) _storage: Box<[u8]>,
}

impl Drop for NodeResources {
    fn drop(&mut self) {
        // Drop order: strings first, then the node struct.
        // This is safe and deterministic — just standard Rust Drop.
        drop(self.owned_valuestring.take());
        drop(self.owned_keystring.take());
        drop(self.node_box.take());
    }
}

/// Execute a full tree-deletion plan.
///
/// Each element in `nodes` is an independently owned set of resources.
/// Dropping the `Vec` frees everything in order — no `unsafe` required.
///
/// The `ffi_impl` layer is responsible for walking the C linked-list and
/// building this vec in the correct order (children before parents, siblings
/// left-to-right, matching the original C `cJSON_Delete` semantics).
pub fn execute_delete_plan(nodes: Vec<NodeResources>) {
    // Simply dropping the vec deallocates every node's resources in order.
    // Each NodeResources::drop() frees strings, then the struct itself.
    drop(nodes);
}

// ---------------------------------------------------------------------------
// Unit tests — safe module
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warn_hooks_ignored_returns_default_when_no_hooks() {
        assert_eq!(warn_hooks_ignored(false, false), HookPolicy::RustDefault);
    }

    #[test]
    fn warn_hooks_ignored_returns_ignored_when_malloc_set() {
        assert_eq!(
            warn_hooks_ignored(true, false),
            HookPolicy::IgnoredCustomHooks
        );
    }

    #[test]
    fn warn_hooks_ignored_returns_ignored_when_both_set() {
        assert_eq!(
            warn_hooks_ignored(true, true),
            HookPolicy::IgnoredCustomHooks
        );
    }

    #[test]
    fn node_resources_drop_is_safe() {
        // Create resources with no owned strings and a dummy node box
        let storage = vec![0u8; 64].into_boxed_slice();
        let resources = NodeResources {
            node_box: Some(BoxedNode { _storage: storage }),
            owned_valuestring: Some(b"hello\0".to_vec()),
            owned_keystring: Some(b"key\0".to_vec()),
        };
        // Dropping should not panic or leak
        drop(resources);
    }

    #[test]
    fn execute_delete_plan_handles_empty() {
        execute_delete_plan(Vec::new()); // no-op, must not panic
    }

    #[test]
    fn hook_policy_display() {
        assert!(format!("{}", HookPolicy::RustDefault).contains("Rust"));
        assert!(format!("{}", HookPolicy::IgnoredCustomHooks).contains("rejected"));
    }
}
