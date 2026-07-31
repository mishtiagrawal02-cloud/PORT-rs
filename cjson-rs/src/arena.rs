//! # Arena-backed JSON AST — Zero Unsafe Code
//!
//! This module provides a **fully safe** JSON Abstract Syntax Tree backed by an
//! [`Arena`] allocator. Instead of `Box<T>`, `Rc<T>`, or raw pointers, every
//! node-to-node relationship (parent, child, sibling) is expressed through
//! **[`NodeId`]** — a thin `u32` index into the arena's internal `Vec<JsonNode>`.
//!
//! ## Why an Arena?
//!
//! | Approach            | Borrow-checker friction | Cycle safety | Cache locality |
//! |---------------------|------------------------|--------------|----------------|
//! | `Box<T>` tree       | High                   | Leak risk    | Poor           |
//! | `Rc<RefCell<T>>`    | Medium                 | Leak risk    | Poor           |
//! | Raw pointers        | None (bypasses)        | Unsafe       | Varies         |
//! | **Arena + indices** | **None**               | **Safe**     | **Excellent**  |
//!
//! ## Design Invariants
//!
//! 1. `#![forbid(unsafe_code)]` — compiler-enforced, zero escape hatches.
//! 2. All structural links (`next`, `prev`, `child`, `parent`) are `Option<NodeId>`.
//! 3. Nodes are **never** deallocated individually; the entire arena drops at once.
//! 4. Every public method on [`Arena`] validates indices before access.

#![forbid(unsafe_code)]

use std::fmt;

// ===========================================================================
//  NodeId — typed index handle
// ===========================================================================

/// An opaque, type-safe handle to a node inside an [`Arena`].
///
/// Internally it is a `u32`, giving us capacity for ~4 billion nodes while
/// keeping structural links at 4 bytes each (half the size of a 64-bit pointer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// Returns the raw index. Useful for serialization or debugging.
    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// Reconstruct a `NodeId` from a raw `u32` index.
    ///
    /// Available in tests and within the crate for converting the `u32`
    /// returned by [`crate::parser::parse_json`] back into a typed handle.
    #[cfg(any(test, doc))]
    #[inline]
    pub fn from_test(raw: u32) -> Self {
        NodeId(raw)
    }

    /// Crate-internal constructor from a raw `u32`.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        NodeId(raw)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({})", self.0)
    }
}

// ===========================================================================
//  JsonValue — the actual payload
// ===========================================================================

/// The value payload of a JSON node.
///
/// Each variant corresponds to one of the six standard JSON types defined in
/// [RFC 8259](https://datatracker.ietf.org/doc/html/rfc8259).
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    /// JSON `null`.
    Null,

    /// JSON boolean (`true` / `false`).
    Bool(bool),

    /// JSON number, stored as `f64` (same precision as JavaScript / cJSON).
    Number(f64),

    /// JSON string, stored as an owned Rust `String` (valid UTF-8).
    String(String),

    /// JSON array — children are linked via the node's `first_child` / sibling chain.
    /// The `usize` caches the logical element count for O(1) `len()`.
    Array { len: usize },

    /// JSON object — children are linked via the node's `first_child` / sibling chain.
    /// Each child node carries a `key`. The `usize` caches the member count.
    Object { len: usize },
}

// ===========================================================================
//  JsonNode — the arena element
// ===========================================================================

/// A single node in the arena-backed JSON tree.
///
/// Structural links are `Option<NodeId>` indices (not pointers):
///
/// ```text
///         parent
///           │
///     ┌─────┴─────┐
///     │  JsonNode  │
///     └─────┬─────┘
///           │ first_child
///     ┌─────┴─────┐   next   ┌───────────┐   next   ┌───────────┐
///     │  child #0  │────────▶│  child #1  │────────▶│  child #2  │
///     └───────────┘◀────────└───────────┘◀────────└───────────┘
///                     prev                   prev
/// ```
#[derive(Debug, Clone)]
pub struct JsonNode {
    /// The JSON value stored at this node.
    pub value: JsonValue,

    /// Optional key name when this node is a member of an object.
    pub key: Option<String>,

    // ── Structural links (all arena indices) ────────────────────────────
    /// Parent node.
    pub parent: Option<NodeId>,
    /// First child (head of the child linked-list for Arrays / Objects).
    pub first_child: Option<NodeId>,
    /// Last child (tail — allows O(1) append).
    pub last_child: Option<NodeId>,
    /// Next sibling in the parent's child list.
    pub next: Option<NodeId>,
    /// Previous sibling in the parent's child list.
    pub prev: Option<NodeId>,
}

impl JsonNode {
    /// Create a new detached node with the given value and no links.
    fn new(value: JsonValue) -> Self {
        JsonNode {
            value,
            key: None,
            parent: None,
            first_child: None,
            last_child: None,
            next: None,
            prev: None,
        }
    }
}

// ===========================================================================
//  Arena — the central allocator + tree operations
// ===========================================================================

/// A contiguous arena that owns all [`JsonNode`]s.
///
/// Nodes are allocated with [`Arena::alloc`] and accessed with [`Arena::get`] /
/// [`Arena::get_mut`]. Parent-child relationships are established with
/// [`Arena::append_child`].
///
/// # Example
///
/// ```rust
/// use cjson_rs::arena::{Arena, JsonValue};
///
/// let mut arena = Arena::new();
///
/// // Build: { "name": "cJSON", "version": 1.7 }
/// let root = arena.alloc_object();
///
/// let name_val = arena.alloc(JsonValue::String("cJSON".into()));
/// arena.append_child_with_key(root, name_val, "name".into());
///
/// let ver_val = arena.alloc(JsonValue::Number(1.7));
/// arena.append_child_with_key(root, ver_val, "version".into());
///
/// assert_eq!(arena.child_count(root), 2);
/// ```
pub struct Arena {
    nodes: Vec<JsonNode>,
}

impl Arena {
    // ── Construction ─────────────────────────────────────────────────────

    /// Create an empty arena.
    pub fn new() -> Self {
        Arena { nodes: Vec::new() }
    }

    /// Create an arena pre-sized for `capacity` nodes to avoid reallocations.
    pub fn with_capacity(capacity: usize) -> Self {
        Arena {
            nodes: Vec::with_capacity(capacity),
        }
    }

    // ── Allocation ───────────────────────────────────────────────────────

    /// Allocate a new node with the given value and return its [`NodeId`].
    ///
    /// The node is initially *detached* — it has no parent, children, or siblings.
    ///
    /// # Panics
    ///
    /// Panics if the arena contains `u32::MAX` nodes (extremely unlikely at ~4 billion).
    pub fn alloc(&mut self, value: JsonValue) -> NodeId {
        let index = self.nodes.len();
        assert!(
            index <= u32::MAX as usize,
            "Arena capacity exceeded: cannot allocate more than {} nodes",
            u32::MAX
        );
        self.nodes.push(JsonNode::new(value));
        NodeId(index as u32)
    }

    /// Convenience: allocate a `Null` node.
    #[inline]
    pub fn alloc_null(&mut self) -> NodeId {
        self.alloc(JsonValue::Null)
    }

    /// Convenience: allocate a `Bool` node.
    #[inline]
    pub fn alloc_bool(&mut self, v: bool) -> NodeId {
        self.alloc(JsonValue::Bool(v))
    }

    /// Convenience: allocate a `Number` node.
    #[inline]
    pub fn alloc_number(&mut self, v: f64) -> NodeId {
        self.alloc(JsonValue::Number(v))
    }

    /// Convenience: allocate a `String` node.
    #[inline]
    pub fn alloc_string(&mut self, v: impl Into<String>) -> NodeId {
        self.alloc(JsonValue::String(v.into()))
    }

    /// Convenience: allocate an empty `Array` node.
    #[inline]
    pub fn alloc_array(&mut self) -> NodeId {
        self.alloc(JsonValue::Array { len: 0 })
    }

    /// Convenience: allocate an empty `Object` node.
    #[inline]
    pub fn alloc_object(&mut self) -> NodeId {
        self.alloc(JsonValue::Object { len: 0 })
    }

    // ── Accessors ────────────────────────────────────────────────────────

    /// Return the total number of nodes currently in the arena.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Return `true` if the arena contains no nodes.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Immutable access to the node at `id`.
    ///
    /// Returns `None` if `id` is out of bounds (e.g., from a different arena).
    #[inline]
    pub fn get(&self, id: NodeId) -> Option<&JsonNode> {
        self.nodes.get(id.index())
    }

    /// Mutable access to the node at `id`.
    ///
    /// Returns `None` if `id` is out of bounds.
    #[inline]
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut JsonNode> {
        self.nodes.get_mut(id.index())
    }

    /// Immutable access with a panic on invalid `id` (convenience for trusted indices).
    ///
    /// # Panics
    ///
    /// Panics if `id` does not reference a valid node in this arena.
    #[inline]
    pub fn node(&self, id: NodeId) -> &JsonNode {
        self.get(id)
            .unwrap_or_else(|| panic!("invalid {id}: arena has {} nodes", self.nodes.len()))
    }

    /// Mutable access with a panic on invalid `id`.
    ///
    /// # Panics
    ///
    /// Panics if `id` does not reference a valid node in this arena.
    #[inline]
    pub fn node_mut(&mut self, id: NodeId) -> &mut JsonNode {
        let len = self.nodes.len();
        self.get_mut(id)
            .unwrap_or_else(|| panic!("invalid {id}: arena has {len} nodes"))
    }

    // ── Tree manipulation ────────────────────────────────────────────────

    /// Append `child_id` as the last child of `parent_id`.
    ///
    /// This wires up all five links (`parent`, `first_child`, `last_child`,
    /// `next`, `prev`) and increments the parent's cached child count
    /// (for `Array` / `Object` variants).
    ///
    /// # Panics
    ///
    /// Panics if either `parent_id` or `child_id` is invalid, or if
    /// `parent_id == child_id`.
    pub fn append_child(&mut self, parent_id: NodeId, child_id: NodeId) {
        assert_ne!(
            parent_id, child_id,
            "a node cannot be its own child ({parent_id})"
        );

        // Read the current tail of the parent's child list.
        let old_last = self.node(parent_id).last_child;

        // Set the child's parent link.
        {
            let child = self.node_mut(child_id);
            child.parent = Some(parent_id);
            child.prev = old_last;
            child.next = None;
        }

        // If there was a previous tail, link it forward to the new child.
        if let Some(old_last_id) = old_last {
            self.node_mut(old_last_id).next = Some(child_id);
        }

        // Update the parent's child pointers and cached count.
        {
            let parent = self.node_mut(parent_id);
            if parent.first_child.is_none() {
                parent.first_child = Some(child_id);
            }
            parent.last_child = Some(child_id);

            // Maintain the cached count.
            match &mut parent.value {
                JsonValue::Array { len } => *len += 1,
                JsonValue::Object { len } => *len += 1,
                _ => {} // leaf nodes don't track child counts
            }
        }
    }

    /// Append `child_id` as the last child of `parent_id` and set the child's
    /// `key` field (for object members).
    ///
    /// This is the primary way to add key-value pairs to JSON objects.
    pub fn append_child_with_key(
        &mut self,
        parent_id: NodeId,
        child_id: NodeId,
        key: String,
    ) {
        self.node_mut(child_id).key = Some(key);
        self.append_child(parent_id, child_id);
    }

    /// Detach `node_id` from its parent (and siblings), making it a root.
    ///
    /// After this call the node's `parent`, `next`, and `prev` are `None`.
    /// The parent's `first_child` / `last_child` and cached count are updated.
    ///
    /// Returns `true` if the node was attached (and is now detached),
    /// `false` if it was already a root.
    pub fn detach(&mut self, node_id: NodeId) -> bool {
        let node = self.node(node_id);
        let parent_id = match node.parent {
            Some(p) => p,
            None => return false,
        };
        let prev_id = node.prev;
        let next_id = node.next;

        // Patch the previous sibling's `next`.
        if let Some(prev) = prev_id {
            self.node_mut(prev).next = next_id;
        }

        // Patch the next sibling's `prev`.
        if let Some(next) = next_id {
            self.node_mut(next).prev = prev_id;
        }

        // Update the parent's head / tail if we were at an edge.
        {
            let parent = self.node_mut(parent_id);
            if parent.first_child == Some(node_id) {
                parent.first_child = next_id;
            }
            if parent.last_child == Some(node_id) {
                parent.last_child = prev_id;
            }
            match &mut parent.value {
                JsonValue::Array { len } => *len = len.saturating_sub(1),
                JsonValue::Object { len } => *len = len.saturating_sub(1),
                _ => {}
            }
        }

        // Clear the detached node's links.
        let node = self.node_mut(node_id);
        node.parent = None;
        node.prev = None;
        node.next = None;

        true
    }

    // ── Traversal helpers ────────────────────────────────────────────────

    /// Return the number of direct children of `parent_id`.
    ///
    /// For `Array` and `Object` nodes this is O(1) (cached).
    /// For other node types it is always 0.
    pub fn child_count(&self, parent_id: NodeId) -> usize {
        match &self.node(parent_id).value {
            JsonValue::Array { len } => *len,
            JsonValue::Object { len } => *len,
            _ => 0,
        }
    }

    /// Iterate over the direct children of `parent_id`.
    ///
    /// Yields `NodeId` values by walking the `first_child` → `next` chain.
    pub fn children(&self, parent_id: NodeId) -> ChildIter<'_> {
        let first = self.node(parent_id).first_child;
        ChildIter {
            arena: self,
            current: first,
        }
    }

    /// Look up a direct child of an object node by key (case-sensitive).
    ///
    /// Returns `None` if `parent_id` is not an `Object`, or if no child has
    /// the matching key.
    pub fn get_object_member(&self, parent_id: NodeId, key: &str) -> Option<NodeId> {
        if !matches!(self.node(parent_id).value, JsonValue::Object { .. }) {
            return None;
        }
        self.children(parent_id).find(|&child_id| {
            self.node(child_id)
                .key
                .as_deref()
                .map_or(false, |k| k == key)
        })
    }

    /// Get the Nth child (0-indexed) of an array or object node.
    ///
    /// O(n) — walks the sibling chain. Returns `None` if out of bounds.
    pub fn get_child_at(&self, parent_id: NodeId, index: usize) -> Option<NodeId> {
        self.children(parent_id).nth(index)
    }

    // ── Pretty-printing ──────────────────────────────────────────────────

    /// Render the subtree rooted at `root_id` as a formatted JSON string.
    ///
    /// This is a recursive, purely safe implementation.
    pub fn to_json_string(&self, root_id: NodeId) -> String {
        let mut buf = String::new();
        self.write_json(root_id, &mut buf, 0);
        buf
    }

    fn write_json(&self, id: NodeId, buf: &mut String, indent: usize) {
        let node = self.node(id);
        match &node.value {
            JsonValue::Null => buf.push_str("null"),
            JsonValue::Bool(true) => buf.push_str("true"),
            JsonValue::Bool(false) => buf.push_str("false"),
            JsonValue::Number(n) => {
                // Match JSON number formatting: no trailing ".0" for integers.
                if n.fract() == 0.0 && n.is_finite() {
                    buf.push_str(&format!("{}", *n as i64));
                } else {
                    buf.push_str(&format!("{n}"));
                }
            }
            JsonValue::String(s) => {
                buf.push('"');
                // Minimal JSON string escaping.
                for ch in s.chars() {
                    match ch {
                        '"' => buf.push_str("\\\""),
                        '\\' => buf.push_str("\\\\"),
                        '\n' => buf.push_str("\\n"),
                        '\r' => buf.push_str("\\r"),
                        '\t' => buf.push_str("\\t"),
                        c if c.is_control() => {
                            buf.push_str(&format!("\\u{:04x}", c as u32));
                        }
                        c => buf.push(c),
                    }
                }
                buf.push('"');
            }
            JsonValue::Array { len } => {
                if *len == 0 {
                    buf.push_str("[]");
                    return;
                }
                buf.push_str("[\n");
                let child_indent = indent + 2;
                let mut first = true;
                for child_id in self.children(id) {
                    if !first {
                        buf.push_str(",\n");
                    }
                    first = false;
                    buf.push_str(&" ".repeat(child_indent));
                    self.write_json(child_id, buf, child_indent);
                }
                buf.push('\n');
                buf.push_str(&" ".repeat(indent));
                buf.push(']');
            }
            JsonValue::Object { len } => {
                if *len == 0 {
                    buf.push_str("{}");
                    return;
                }
                buf.push_str("{\n");
                let child_indent = indent + 2;
                let mut first = true;
                for child_id in self.children(id) {
                    if !first {
                        buf.push_str(",\n");
                    }
                    first = false;
                    buf.push_str(&" ".repeat(child_indent));
                    // Write the key.
                    if let Some(ref key) = self.node(child_id).key {
                        buf.push('"');
                        buf.push_str(key);
                        buf.push_str("\": ");
                    }
                    self.write_json(child_id, buf, child_indent);
                }
                buf.push('\n');
                buf.push_str(&" ".repeat(indent));
                buf.push('}');
            }
        }
    }
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Arena {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Arena")
            .field("node_count", &self.nodes.len())
            .finish()
    }
}

// ===========================================================================
//  ChildIter — iterate over a node's direct children
// ===========================================================================

/// Iterator over the direct children of a node, yielding [`NodeId`] values.
pub struct ChildIter<'a> {
    arena: &'a Arena,
    current: Option<NodeId>,
}

impl<'a> Iterator for ChildIter<'a> {
    type Item = NodeId;

    #[inline]
    fn next(&mut self) -> Option<NodeId> {
        let id = self.current?;
        self.current = self.arena.node(id).next;
        Some(id)
    }
}

// ===========================================================================
//  Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── Allocation ───────────────────────────────────────────────────────

    #[test]
    fn alloc_produces_sequential_ids() {
        let mut arena = Arena::new();
        let a = arena.alloc_null();
        let b = arena.alloc_bool(true);
        let c = arena.alloc_number(3.14);
        assert_eq!(a.index(), 0);
        assert_eq!(b.index(), 1);
        assert_eq!(c.index(), 2);
        assert_eq!(arena.len(), 3);
    }

    #[test]
    fn get_returns_none_for_invalid_id() {
        let arena = Arena::new();
        let bogus = NodeId(42);
        assert!(arena.get(bogus).is_none());
    }

    // ── Tree structure ───────────────────────────────────────────────────

    #[test]
    fn append_child_links_correctly() {
        let mut arena = Arena::new();
        let arr = arena.alloc_array();
        let a = arena.alloc_number(1.0);
        let b = arena.alloc_number(2.0);
        let c = arena.alloc_number(3.0);

        arena.append_child(arr, a);
        arena.append_child(arr, b);
        arena.append_child(arr, c);

        assert_eq!(arena.child_count(arr), 3);

        // Forward traversal: a → b → c.
        assert_eq!(arena.node(arr).first_child, Some(a));
        assert_eq!(arena.node(a).next, Some(b));
        assert_eq!(arena.node(b).next, Some(c));
        assert_eq!(arena.node(c).next, None);

        // Backward traversal: c → b → a.
        assert_eq!(arena.node(arr).last_child, Some(c));
        assert_eq!(arena.node(c).prev, Some(b));
        assert_eq!(arena.node(b).prev, Some(a));
        assert_eq!(arena.node(a).prev, None);

        // Parent links.
        assert_eq!(arena.node(a).parent, Some(arr));
        assert_eq!(arena.node(b).parent, Some(arr));
        assert_eq!(arena.node(c).parent, Some(arr));
    }

    #[test]
    fn children_iter_yields_all_children() {
        let mut arena = Arena::new();
        let arr = arena.alloc_array();
        let items: Vec<NodeId> = (0..5)
            .map(|i| {
                let id = arena.alloc_number(i as f64);
                arena.append_child(arr, id);
                id
            })
            .collect();

        let collected: Vec<NodeId> = arena.children(arr).collect();
        assert_eq!(collected, items);
    }

    // ── Object key lookup ────────────────────────────────────────────────

    #[test]
    fn object_member_lookup() {
        let mut arena = Arena::new();
        let obj = arena.alloc_object();

        let name = arena.alloc_string("Rust");
        arena.append_child_with_key(obj, name, "language".into());

        let year = arena.alloc_number(2015.0);
        arena.append_child_with_key(obj, year, "since".into());

        assert_eq!(arena.get_object_member(obj, "language"), Some(name));
        assert_eq!(arena.get_object_member(obj, "since"), Some(year));
        assert_eq!(arena.get_object_member(obj, "nope"), None);
    }

    // ── Detach ───────────────────────────────────────────────────────────

    #[test]
    fn detach_middle_child() {
        let mut arena = Arena::new();
        let arr = arena.alloc_array();
        let a = arena.alloc_number(1.0);
        let b = arena.alloc_number(2.0);
        let c = arena.alloc_number(3.0);
        arena.append_child(arr, a);
        arena.append_child(arr, b);
        arena.append_child(arr, c);

        assert!(arena.detach(b));
        assert_eq!(arena.child_count(arr), 2);
        assert_eq!(arena.node(a).next, Some(c));
        assert_eq!(arena.node(c).prev, Some(a));
        assert_eq!(arena.node(b).parent, None);
    }

    #[test]
    fn detach_first_child() {
        let mut arena = Arena::new();
        let arr = arena.alloc_array();
        let a = arena.alloc_number(1.0);
        let b = arena.alloc_number(2.0);
        arena.append_child(arr, a);
        arena.append_child(arr, b);

        assert!(arena.detach(a));
        assert_eq!(arena.node(arr).first_child, Some(b));
        assert_eq!(arena.node(b).prev, None);
        assert_eq!(arena.child_count(arr), 1);
    }

    #[test]
    fn detach_last_child() {
        let mut arena = Arena::new();
        let arr = arena.alloc_array();
        let a = arena.alloc_number(1.0);
        let b = arena.alloc_number(2.0);
        arena.append_child(arr, a);
        arena.append_child(arr, b);

        assert!(arena.detach(b));
        assert_eq!(arena.node(arr).last_child, Some(a));
        assert_eq!(arena.node(a).next, None);
        assert_eq!(arena.child_count(arr), 1);
    }

    #[test]
    fn detach_root_returns_false() {
        let mut arena = Arena::new();
        let root = arena.alloc_null();
        assert!(!arena.detach(root));
    }

    // ── Nested tree (JSON document) ──────────────────────────────────────

    #[test]
    fn nested_json_tree() {
        // Build: { "name": "cJSON", "stars": 11000, "tags": ["c", "parser"] }
        let mut arena = Arena::with_capacity(8);

        let root = arena.alloc_object();

        let name = arena.alloc_string("cJSON");
        arena.append_child_with_key(root, name, "name".into());

        let stars = arena.alloc_number(11000.0);
        arena.append_child_with_key(root, stars, "stars".into());

        let tags = arena.alloc_array();
        arena.append_child_with_key(root, tags, "tags".into());

        let tag_c = arena.alloc_string("c");
        arena.append_child(tags, tag_c);

        let tag_parser = arena.alloc_string("parser");
        arena.append_child(tags, tag_parser);

        // Verify structure.
        assert_eq!(arena.child_count(root), 3);
        assert_eq!(arena.child_count(tags), 2);
        assert_eq!(arena.len(), 6); // root + name + stars + tags + "c" + "parser"

        // Verify traversal.
        let tags_id = arena.get_object_member(root, "tags").unwrap();
        assert_eq!(tags_id, tags);
        let first_tag = arena.get_child_at(tags_id, 0).unwrap();
        assert_eq!(arena.node(first_tag).value, JsonValue::String("c".into()));
    }

    // ── Pretty-print ─────────────────────────────────────────────────────

    #[test]
    fn json_to_string_round_trip() {
        let mut arena = Arena::new();
        let root = arena.alloc_object();

        let v = arena.alloc_bool(true);
        arena.append_child_with_key(root, v, "active".into());

        let v = arena.alloc_null();
        arena.append_child_with_key(root, v, "deleted".into());

        let output = arena.to_json_string(root);
        assert!(output.contains("\"active\": true"));
        assert!(output.contains("\"deleted\": null"));
    }

    // ── NodeId display ───────────────────────────────────────────────────

    #[test]
    fn node_id_display() {
        let id = NodeId(7);
        assert_eq!(format!("{id}"), "NodeId(7)");
    }

    // ── Empty containers ─────────────────────────────────────────────────

    #[test]
    fn empty_containers_print_compactly() {
        let mut arena = Arena::new();
        let arr = arena.alloc_array();
        let obj = arena.alloc_object();
        assert_eq!(arena.to_json_string(arr), "[]");
        assert_eq!(arena.to_json_string(obj), "{}");
    }
}
