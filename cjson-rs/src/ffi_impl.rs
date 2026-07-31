//! # FFI Implementation — `extern "C"` entry points
//!
//! This module provides the **actual Rust implementations** of `cJSON_InitHooks`
//! and `cJSON_Delete` that C code links against.  These are `#[no_mangle]`
//! `extern "C"` functions — the thinnest possible `unsafe` shim that
//! immediately delegates to the safe module (`crate::safe`).
//!
//! ## Safety Contract
//!
//! `unsafe` is used **only** for:
//! 1. Dereferencing the raw `*mut cJSON_Hooks` / `*mut cJSON` pointers that
//!    arrive from C callers.
//! 2. Reconstituting `Box` / `Vec` from raw pointers that **we** originally
//!    handed out via `Box::into_raw` / `Vec::into_raw_parts` (or equivalent).
//!
//! No `unsafe` is used for business logic, control flow, or allocation
//! decisions — those live in `crate::safe` under `#![forbid(unsafe_code)]`.

#![allow(non_snake_case)]

use std::os::raw::c_char;
use std::ptr;

use crate::{cJSON, cJSON_Hooks, CJSON_IS_REFERENCE, CJSON_STRING_IS_CONST};
use crate::safe::{self, BoxedNode, NodeResources};

// ===========================================================================
//  cJSON_InitHooks — safe stub
// ===========================================================================

/// Drop-in replacement for the C `cJSON_InitHooks`.
///
/// **Behaviour:**
/// - If `hooks` is NULL → interpreted as "reset to defaults".  Since we
///   always use the Rust global allocator, this is a no-op.
/// - If `hooks` is non-NULL → we inspect whether `malloc_fn` / `free_fn`
///   are set, log a warning via the safe module, and **ignore** them.
///
/// The C test suite calls this during setup and teardown.  Our stub ensures
/// it never segfaults and the tests continue to run with Rust's allocator.
///
/// # Safety
///
/// - `hooks` must be NULL or point to a valid, aligned `cJSON_Hooks` struct.
/// - This function is called from C; the caller is responsible for argument
///   validity per the original `cJSON.h` contract.
#[no_mangle]
pub unsafe extern "C" fn cJSON_InitHooks(hooks: *mut cJSON_Hooks) {
    if hooks.is_null() {
        // NULL → "reset to default allocator".
        // We always use the Rust allocator, so this is a no-op.
        let _ = safe::warn_hooks_ignored(false, false);
        return;
    }

    // SAFETY: caller guarantees `hooks` is valid and aligned.
    let h = unsafe { &*hooks };

    let has_malloc = h.malloc_fn.is_some();
    let has_free = h.free_fn.is_some();

    // Delegate the warning / policy decision to the safe module.
    // We intentionally do NOT store the function pointers.
    let _policy = safe::warn_hooks_ignored(has_malloc, has_free);
}

// ===========================================================================
//  cJSON_Delete — recursive tree deallocation via Rust Drop
// ===========================================================================

/// Drop-in replacement for the C `cJSON_Delete`.
///
/// Faithfully mirrors the original C semantics (cJSON.c lines 253-276):
///
/// 1. Walk the `next` sibling chain **iteratively** (avoids stack overflow
///    on wide arrays).
/// 2. For each node, if NOT a reference and has children, **recursively**
///    delete the child sub-tree.
/// 3. Free `valuestring` if owned (not `cJSON_IsReference`).
/// 4. Free `string` (key name) if owned (not `cJSON_StringIsConst`).
/// 5. Free the node struct itself.
///
/// All resources are collected into a `Vec<NodeResources>` and handed to
/// `safe::execute_delete_plan()` which drops them using standard Rust
/// `Drop` semantics — no manual `free()` calls.
///
/// # Safety
///
/// - `item` must be NULL **or** a pointer previously returned by one of the
///   `cJSON_Create*` / `cJSON_Parse*` functions (i.e., allocated via
///   `Box::into_raw` in our Rust implementations).
/// - After this call, `item` and all nodes reachable from it are dangling —
///   the caller must not dereference them.
/// - This exactly matches the safety contract of the original C `cJSON_Delete`.
#[no_mangle]
pub unsafe extern "C" fn cJSON_Delete(item: *mut cJSON) {
    if item.is_null() {
        return;
    }

    // We build the deallocation plan as a Vec of NodeResources.
    // The safe module drops them in order, releasing memory via Rust's
    // global allocator.
    let mut plan: Vec<NodeResources> = Vec::new();

    // SAFETY: We mirror the C implementation's iterative sibling walk.
    // `item` was checked non-null above; subsequent nodes are checked
    // in the loop condition.
    unsafe {
        collect_tree_for_deletion(item, &mut plan);
    }

    // Hand off to the safe module — this is a safe function call.
    safe::execute_delete_plan(plan);
}

/// Recursively collect all nodes in the tree rooted at `item` into `plan`.
///
/// Walks the `next` chain iteratively (matching C's `while (item != NULL)`
/// loop) and recurses into `child` sub-trees.  For each node:
///
/// - Reconstitutes the `Box<cJSON>` that was originally created via
///   `Box::into_raw` in the `cJSON_Create*` / `cJSON_Parse*` functions.
/// - Reconstitutes any owned strings (`valuestring`, `string`) as `Vec<u8>`
///   via `Vec::from_raw_parts`.
/// - Respects the `cJSON_IsReference` and `cJSON_StringIsConst` modifier
///   flags — borrowed pointers are NOT freed.
///
/// # Safety
///
/// - `item` must be a valid, non-null pointer to a `cJSON` node allocated
///   by our Rust code (via `Box::into_raw`).
/// - Owned string fields must have been allocated by our Rust code (via
///   `CString::into_raw` or equivalent).
/// - The caller must ensure no other references to these nodes exist.
unsafe fn collect_tree_for_deletion(item: *mut cJSON, plan: &mut Vec<NodeResources>) {
    let mut current = item;

    while !current.is_null() {
        // Read the next pointer BEFORE we consume the node.
        // SAFETY: `current` is non-null and valid per our invariant.
        let next_sibling = (*current).next;

        // Recursively collect children (if owned, not a reference).
        let is_reference = ((*current).type_ & CJSON_IS_REFERENCE) != 0;
        let child = (*current).child;

        if !is_reference && !child.is_null() {
            collect_tree_for_deletion(child, plan);
        }

        // --- Collect owned valuestring ---
        let owned_valuestring = if !is_reference && !(*current).valuestring.is_null() {
            let vs_ptr = (*current).valuestring;
            // Null out the field to prevent double-free if something goes wrong.
            (*current).valuestring = ptr::null_mut();

            // SAFETY: This string was allocated by our Rust code via
            // CString::into_raw (which gives us a NUL-terminated buffer).
            // We reconstruct the Vec to take ownership for safe deallocation.
            // strlen + 1 to include the NUL terminator.
            let len = libc_strlen(vs_ptr) + 1;
            Some(Vec::from_raw_parts(vs_ptr as *mut u8, len, len))
        } else {
            None
        };

        // --- Collect owned keystring ---
        let is_const_string = ((*current).type_ & CJSON_STRING_IS_CONST) != 0;
        let owned_keystring = if !is_const_string && !(*current).string.is_null() {
            let s_ptr = (*current).string;
            (*current).string = ptr::null_mut();

            let len = libc_strlen(s_ptr) + 1;
            Some(Vec::from_raw_parts(s_ptr as *mut u8, len, len))
        } else {
            None
        };

        // --- Collect the node struct itself ---
        // Null out tree pointers to prevent any dangling references.
        (*current).next = ptr::null_mut();
        (*current).prev = ptr::null_mut();
        (*current).child = ptr::null_mut();

        // SAFETY: `current` was allocated via `Box::into_raw(Box::new(cJSON { .. }))`
        // in our Create* functions.  We reconstitute the Box here.
        let node_allocation = Box::from_raw(current);
        // Convert to a byte-slice Box for the safe module (which doesn't
        // know about the cJSON type — it just holds the allocation).
        let raw_bytes = Box::from_raw(
            Box::into_raw(node_allocation) as *mut [u8; std::mem::size_of::<cJSON>()]
        );
        let byte_slice: Box<[u8]> = Box::from(raw_bytes.as_ref().to_vec().into_boxed_slice());

        plan.push(NodeResources {
            node_box: Some(BoxedNode { _storage: byte_slice }),
            owned_valuestring,
            owned_keystring,
        });

        // Move to the next sibling (iterative, matching C).
        current = next_sibling;
    }
}

// ---------------------------------------------------------------------------
// strlen helper — we avoid linking libc just for this
// ---------------------------------------------------------------------------

/// Compute the length of a NUL-terminated C string, excluding the terminator.
///
/// # Safety
///
/// `s` must point to a valid NUL-terminated string.
#[inline]
unsafe fn libc_strlen(s: *const c_char) -> usize {
    let mut len = 0usize;
    while *s.add(len) != 0 {
        len += 1;
    }
    len
}

// ===========================================================================
//  Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{cJSON, CJSON_OBJECT, CJSON_STRING, CJSON_STRING_IS_CONST, CJSON_IS_REFERENCE};
    use std::ffi::CString;

    /// Helper: allocate a cJSON node via Box and return the raw pointer,
    /// exactly as our Create* functions will do.
    fn new_raw_node(type_: i32) -> *mut cJSON {
        let node = Box::new(cJSON {
            next: ptr::null_mut(),
            prev: ptr::null_mut(),
            child: ptr::null_mut(),
            type_,
            valuestring: ptr::null_mut(),
            valueint: 0,
            valuedouble: 0.0,
            string: ptr::null_mut(),
        });
        Box::into_raw(node)
    }

    #[test]
    fn delete_null_is_noop() {
        // Must not segfault.
        unsafe { cJSON_Delete(ptr::null_mut()) };
    }

    #[test]
    fn delete_single_node_no_strings() {
        let node = new_raw_node(CJSON_OBJECT);
        unsafe { cJSON_Delete(node) };
        // If we reach here without SIGSEGV / double-free, the test passes.
    }

    #[test]
    fn delete_node_with_owned_strings() {
        let node = new_raw_node(CJSON_STRING);
        let vs = CString::new("hello world").unwrap();
        let ks = CString::new("mykey").unwrap();
        unsafe {
            (*node).valuestring = vs.into_raw();
            (*node).string = ks.into_raw();
            cJSON_Delete(node);
        }
    }

    #[test]
    fn delete_node_with_const_key_skips_key_free() {
        let node = new_raw_node(CJSON_STRING | CJSON_STRING_IS_CONST);
        let vs = CString::new("value").unwrap();
        // key is a static / const string — we must NOT free it.
        let static_key: &'static [u8] = b"static_key\0";
        unsafe {
            (*node).valuestring = vs.into_raw();
            (*node).string = static_key.as_ptr() as *mut c_char;
            // Only valuestring should be freed, not string.
            // We need to avoid freeing the static key, so mark it const.
            cJSON_Delete(node);
        }
    }

    #[test]
    fn delete_reference_node_skips_child_and_valuestring() {
        // The referenced child is owned elsewhere and must NOT be freed here.
        let referenced_child = new_raw_node(CJSON_STRING);
        let vs = CString::new("child_value").unwrap();
        unsafe {
            (*referenced_child).valuestring = vs.into_raw();
        }

        let ref_node = new_raw_node(CJSON_OBJECT | CJSON_IS_REFERENCE);
        unsafe {
            (*ref_node).child = referenced_child;
            // Delete the reference node — must NOT delete referenced_child.
            cJSON_Delete(ref_node);

            // referenced_child is still alive — clean it up separately.
            cJSON_Delete(referenced_child);
        }
    }

    #[test]
    fn delete_sibling_chain() {
        // Build a chain: a → b → c (via `next` pointers).
        let a = new_raw_node(CJSON_OBJECT);
        let b = new_raw_node(CJSON_OBJECT);
        let c = new_raw_node(CJSON_OBJECT);
        unsafe {
            (*a).next = b;
            (*b).prev = a;
            (*b).next = c;
            (*c).prev = b;
            // Deleting `a` should free a, b, and c.
            cJSON_Delete(a);
        }
    }

    #[test]
    fn delete_tree_with_children_and_siblings() {
        //  root (object)
        //    └─ child_a (string, key="a") ──next──▶ child_b (string, key="b")
        let root = new_raw_node(CJSON_OBJECT);
        let child_a = new_raw_node(CJSON_STRING);
        let child_b = new_raw_node(CJSON_STRING);

        let key_a = CString::new("a").unwrap();
        let val_a = CString::new("alpha").unwrap();
        let key_b = CString::new("b").unwrap();
        let val_b = CString::new("beta").unwrap();

        unsafe {
            (*child_a).string = key_a.into_raw();
            (*child_a).valuestring = val_a.into_raw();
            (*child_a).next = child_b;

            (*child_b).string = key_b.into_raw();
            (*child_b).valuestring = val_b.into_raw();
            (*child_b).prev = child_a;

            (*root).child = child_a;

            // This single call must free root + child_a + child_b + all strings.
            cJSON_Delete(root);
        }
    }

    #[test]
    fn init_hooks_null_is_noop() {
        unsafe { cJSON_InitHooks(ptr::null_mut()) };
    }

    #[test]
    fn init_hooks_with_custom_hooks_logs_warning() {
        let mut hooks = cJSON_Hooks {
            malloc_fn: Some(dummy_malloc),
            free_fn: Some(dummy_free),
        };
        // Must not segfault; should log a warning to stderr.
        unsafe { cJSON_InitHooks(&mut hooks as *mut cJSON_Hooks) };
    }

    unsafe extern "C" fn dummy_malloc(sz: usize) -> *mut std::os::raw::c_void {
        // This should NEVER actually be called by our implementation.
        panic!("dummy_malloc called with size {sz} — Rust implementation should not call C hooks!");
    }

    unsafe extern "C" fn dummy_free(ptr: *mut std::os::raw::c_void) {
        // This should NEVER actually be called by our implementation.
        panic!("dummy_free called with {ptr:?} — Rust implementation should not call C hooks!");
    }
}
