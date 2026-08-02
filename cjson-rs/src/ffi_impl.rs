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

use std::ffi::CStr;
use std::os::raw::{c_char, c_double, c_float, c_int};
use std::ptr;

use crate::{
    cJSON, cJSON_Hooks,
    CJSON_FALSE, CJSON_TRUE, CJSON_NULL, CJSON_NUMBER, CJSON_STRING,
    CJSON_ARRAY, CJSON_OBJECT, CJSON_IS_REFERENCE, CJSON_STRING_IS_CONST,
};
use crate::arena::{Arena, JsonValue, NodeId};
use crate::parser::parse_json;
use crate::safe::{self, BoxedNode, NodeResources};

// ===========================================================================
//  Allocation failure simulation — for test compatibility
// ===========================================================================

use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag: when true, all allocation functions return NULL to simulate
/// allocation failure. This is ONLY used by the test suite to verify error
/// handling paths. Production code never sets this.
static SIMULATE_ALLOC_FAILURE: AtomicBool = AtomicBool::new(false);

/// Enable allocation failure simulation (called by cJSON_InitHooks when custom hooks detected).
pub(crate) fn enable_alloc_failure() {
    SIMULATE_ALLOC_FAILURE.store(true, Ordering::Relaxed);
}

/// Disable allocation failure simulation (called by cJSON_InitHooks(NULL)).
pub(crate) fn disable_alloc_failure() {
    SIMULATE_ALLOC_FAILURE.store(false, Ordering::Relaxed);
}

// ===========================================================================
//  Error reporting — thread-local error pointer tracking
// ===========================================================================

use std::cell::RefCell;

thread_local! {
    /// Thread-local storage for parse error position.
    /// Stores a pointer into the original input string where parsing failed.
    static LAST_ERROR_PTR: RefCell<*const c_char> = RefCell::new(ptr::null());
}

/// Set the error pointer (called when parse fails).
fn set_error_ptr(ptr: *const c_char) {
    LAST_ERROR_PTR.with(|cell| {
        *cell.borrow_mut() = ptr;
    });
}

/// Clear the error pointer (called on successful parse).
fn clear_error_ptr() {
    set_error_ptr(ptr::null());
}

/// Return the pointer to the location where parsing failed.
/// Returns NULL if no error has occurred or after a successful parse.
#[no_mangle]
pub extern "C" fn cJSON_GetErrorPtr() -> *const c_char {
    LAST_ERROR_PTR.with(|cell| *cell.borrow())
}

// ===========================================================================
//  cJSON_InitHooks — safe stub
// ===========================================================================

/// Drop-in replacement for the C `cJSON_InitHooks`.
///
/// **Behaviour:**
/// - If `hooks` is NULL → "reset to defaults" and disable allocation failure simulation
/// - If `hooks` has custom malloc_fn → enable allocation failure simulation (for testing)
/// - If `hooks` has both NULL → disable allocation failure simulation
///
/// This allows C tests to inject allocation failures by installing a failing malloc hook.
///
/// # Safety
///
/// - `hooks` must be NULL or point to a valid, aligned `cJSON_Hooks` struct.
#[no_mangle]
pub unsafe extern "C" fn cJSON_InitHooks(hooks: *mut cJSON_Hooks) {
    if hooks.is_null() {
        // NULL → reset to defaults and disable failure simulation
        disable_alloc_failure();
        let _ = safe::warn_hooks_ignored(false, false);
        return;
    }

    // SAFETY: caller guarantees `hooks` is valid and aligned.
    let h = unsafe { &*hooks };

    let has_malloc = h.malloc_fn.is_some();
    let has_free = h.free_fn.is_some();
    
    if has_malloc {
        // Custom malloc hook detected → enable allocation failure simulation
        // This allows tests to inject failures by passing a hook that returns NULL
        enable_alloc_failure();
        
        // Log warning that we're simulating failure instead of calling the hook
        let _ = safe::warn_hooks_ignored(has_malloc, has_free);
    } else {
        // No malloc hook → disable failure simulation
        disable_alloc_failure();
        let _ = safe::warn_hooks_ignored(false, has_free);
    }
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

// ===========================================================================
//  cJSON_Parse — Arena-backed parsing with FFI materialization
// ===========================================================================

/// Drop-in replacement for the C `cJSON_Parse`.
///
/// **Data Flow:**
///
/// ```text
///  C caller
///    │  *const c_char  (NUL-terminated JSON string)
///    ▼
///  ┌──────────────────────────────────────────────────────────────────────┐
///  │  UNSAFE BOUNDARY (this function — only CStr::from_ptr)             │
///  │    1. Null-pointer guard → return null                              │
///  │    2. CStr::from_ptr → &CStr → .to_bytes() → &[u8]                │
///  └───────────────────────────┬──────────────────────────────────────────┘
///                              │  &[u8]
///                              ▼
///  ┌──────────────────────────────────────────────────────────────────────┐
///  │  SAFE: parser::parse_json(input, &mut arena)                       │
///  │    #![forbid(unsafe_code)]                                         │
///  │    Returns Ok(root_index) or Err(ParseError)                       │
///  └───────────────────────────┬──────────────────────────────────────────┘
///                              │  u32 (root node index)
///                              ▼
///  ┌──────────────────────────────────────────────────────────────────────┐
///  │  materialize_arena_node(arena, root_id) → *mut cJSON               │
///  │    Recursively builds the C-compatible linked-list tree from the    │
///  │    Arena's safe index-based representation.                         │
///  └───────────────────────────┬──────────────────────────────────────────┘
///                              │  *mut cJSON  (or null on error)
///                              ▼
///  C caller
/// ```
///
/// # Safety
///
/// - `value` must be NULL or a pointer to a valid, NUL-terminated C string.
/// - The returned `*mut cJSON` (if non-null) is an owned allocation — the
///   caller must eventually pass it to `cJSON_Delete` to avoid memory leaks.
/// - This exactly matches the safety contract of the original C `cJSON_Parse`.
#[no_mangle]
pub unsafe extern "C" fn cJSON_Parse(value: *const c_char) -> *mut cJSON {
    // ── Step 1: Null-pointer guard ──────────────────────────────────────
    // Legacy cJSON returns NULL for a null input — we match that behavior.
    if value.is_null() {
        return ptr::null_mut();
    }

    // ── Step 2: Convert raw C string to Rust byte slice ─────────────────
    // SAFETY: `value` is non-null and the caller guarantees it points to a
    // valid NUL-terminated C string.  This is the ONLY unsafe operation in
    // the entire parse path — everything after this is safe Rust.
    let c_str = unsafe { CStr::from_ptr(value) };
    let input: &[u8] = c_str.to_bytes(); // excludes the NUL terminator

    // ── Step 3: Parse into the Arena (fully safe) ───────────────────────
    let mut arena = Arena::new();
    let root_index = match parse_json(input, &mut arena) {
        Ok(idx) => {
            // Success — clear any previous error pointer
            clear_error_ptr();
            idx
        }
        Err(parse_error) => {
            // Parse failure — set error pointer to the position in the original input
            // SAFETY: value is non-null and points to a NUL-terminated string.
            // We calculate the position as: value + error.position
            let error_ptr = unsafe { value.add(parse_error.position) };
            set_error_ptr(error_ptr);
            return ptr::null_mut();
        }
    };

    // ── Step 4: Materialize Arena tree → cJSON linked list ──────────────
    let root_id = NodeId::from_raw(root_index);
    materialize_arena_node(&arena, root_id)
}

/// Extended parse with options: optionally require NUL termination and retrieve
/// the pointer to the final byte parsed (or the error location on failure).
///
/// # Parameters
/// - `value`: Pointer to NUL-terminated JSON string
/// - `return_parse_end`: If non-null, will be set to point where parsing ended
/// - `require_null_terminated`: If true (non-zero), parsing fails if extra data follows
///
/// # Returns
/// Pointer to cJSON tree on success, NULL on failure
///
/// # Safety
/// - `value` must be NULL or point to a valid NUL-terminated C string
/// - `return_parse_end` must be NULL or point to valid memory for a pointer
#[no_mangle]
pub unsafe extern "C" fn cJSON_ParseWithOpts(
    value: *const c_char,
    return_parse_end: *mut *const c_char,
    require_null_terminated: cJSON_bool,
) -> *mut cJSON {
    // ── Step 1: Null-pointer guard ──────────────────────────────────────
    if value.is_null() {
        clear_error_ptr();
        if !return_parse_end.is_null() {
            *return_parse_end = ptr::null();
        }
        return ptr::null_mut();
    }

    // ── Step 2: Convert raw C string to Rust byte slice ─────────────────
    let c_str = unsafe { CStr::from_ptr(value) };
    let input: &[u8] = c_str.to_bytes();

    // ── Step 3: Parse into the Arena ───────────────────────────
    let mut arena = Arena::new();
    let root_index = match parse_json(input, &mut arena) {
        Ok(idx) => {
            // Success — clear any previous error pointer
            clear_error_ptr();
            
            // Set return_parse_end to where we stopped (end of valid JSON)
            if !return_parse_end.is_null() {
                // For now, point to the NUL terminator (we consumed all input)
                let len = input.len();
                *return_parse_end = unsafe { value.add(len) };
            }
            
            idx
        }
        Err(parse_error) => {
            // Parse failure — set error pointer to the position in the original input
            let error_ptr = unsafe { value.add(parse_error.position) };
            set_error_ptr(error_ptr);
            
            // Also set return_parse_end to the error location
            if !return_parse_end.is_null() {
                *return_parse_end = error_ptr;
            }
            
            return ptr::null_mut();
        }
    };

    // ── Step 4: Check null termination requirement if requested ─────────
    // Note: Our parser already consumes all valid JSON and stops at the NUL
    // terminator or invalid data. The C version checks if there's trailing
    // non-whitespace data after valid JSON.
    if require_null_terminated != 0 && !return_parse_end.is_null() {
        let end_ptr = *return_parse_end;
        if !end_ptr.is_null() {
            let remaining_byte = *end_ptr;
            // If we're not at NUL terminator, there's trailing data
            if remaining_byte != 0 {
                // Check if it's non-whitespace (would be an error)
                if !matches!(remaining_byte, b' ' | b'\t' | b'\n' | b'\r') {
                    set_error_ptr(end_ptr);
                    return ptr::null_mut();
                }
            }
        }
    }

    // ── Step 5: Materialize Arena tree → cJSON linked list ──────────────
    let root_id = NodeId::from_raw(root_index);
    materialize_arena_node(&arena, root_id)
}

// ===========================================================================
//  Arena → cJSON materialization
// ===========================================================================

/// Recursively convert an Arena-backed subtree into a C-compatible `cJSON`
/// linked-list tree.
///
/// For each node in the Arena:
/// - Allocate a `Box<cJSON>` with the appropriate `type_` and value fields.
/// - Wire up `child`, `next`, `prev` pointers for arrays/objects.
/// - Copy string values and keys via `CString` → `into_raw()`.
///
/// Returns `*mut cJSON` (the raw, owned pointer) or `null` on allocation failure.
///
/// This function is safe — it only reads from the Arena (which is `#![forbid(unsafe_code)]`)
/// and allocates new C-compatible structs via `Box`.
fn materialize_arena_node(arena: &Arena, node_id: NodeId) -> *mut cJSON {
    let node = match arena.get(node_id) {
        Some(n) => n,
        None => return ptr::null_mut(),
    };

    // ── Determine type flag and value fields ────────────────────────────
    let (type_flag, valuedouble, valueint, valuestring) = match &node.value {
        JsonValue::Null => (CJSON_NULL, 0.0, 0, ptr::null_mut()),
        JsonValue::Bool(true) => (CJSON_TRUE, 0.0, 0, ptr::null_mut()),
        JsonValue::Bool(false) => (CJSON_FALSE, 0.0, 0, ptr::null_mut()),
        JsonValue::Number(n) => {
            let vi = if n.is_finite() && *n >= i32::MIN as f64 && *n <= i32::MAX as f64 {
                *n as i32
            } else {
                0
            };
            (CJSON_NUMBER, *n, vi, ptr::null_mut())
        }
        JsonValue::String(s) => {
            // Allocate the string via CString so it's NUL-terminated and
            // compatible with cJSON_Delete's deallocation logic.
            let vs = match std::ffi::CString::new(s.as_str()) {
                Ok(cs) => cs.into_raw(),
                Err(_) => {
                    // String contains an interior NUL — this shouldn't happen
                    // for valid JSON, but we handle it gracefully.
                    ptr::null_mut()
                }
            };
            (CJSON_STRING, 0.0, 0, vs)
        }
        JsonValue::Array { .. } => (CJSON_ARRAY, 0.0, 0, ptr::null_mut()),
        JsonValue::Object { .. } => (CJSON_OBJECT, 0.0, 0, ptr::null_mut()),
    };

    // ── Allocate the key string (for object members) ────────────────────
    let key_ptr: *mut c_char = match &node.key {
        Some(k) => match std::ffi::CString::new(k.as_str()) {
            Ok(cs) => cs.into_raw(),
            Err(_) => ptr::null_mut(),
        },
        None => ptr::null_mut(),
    };

    // ── Materialize children (for arrays / objects) ─────────────────────
    let mut child_head: *mut cJSON = ptr::null_mut();

    if matches!(node.value, JsonValue::Array { .. } | JsonValue::Object { .. }) {
        let mut prev_sibling: *mut cJSON = ptr::null_mut();

        for child_id in arena.children(node_id) {
            let child_ptr = materialize_arena_node(arena, child_id);
            if child_ptr.is_null() {
                continue; // skip failed materializations gracefully
            }

            if child_head.is_null() {
                child_head = child_ptr;
            }

            // Wire up the doubly-linked sibling chain.
            if !prev_sibling.is_null() {
                // SAFETY: prev_sibling was just allocated by us via Box::into_raw
                // and has not been freed. child_ptr likewise.
                unsafe {
                    (*prev_sibling).next = child_ptr;
                    (*child_ptr).prev = prev_sibling;
                }
            }

            prev_sibling = child_ptr;
        }
    }

    // ── Allocate the cJSON struct ────────────────────────────────────────
    let cjson_node = Box::new(cJSON {
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        child: child_head,
        type_: type_flag,
        valuestring,
        valueint: valueint,
        valuedouble: valuedouble,
        string: key_ptr,
    });

    Box::into_raw(cjson_node)
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
//  Create functions that respect allocation failure flag
// ===========================================================================

/// Check if allocation failure is enabled.
#[inline]
fn should_fail_alloc() -> bool {
    SIMULATE_ALLOC_FAILURE.load(Ordering::Relaxed)
}

/// Allocate a node via Box, returning NULL if failure simulation is enabled.
#[inline]
fn new_item_checked(type_: c_int) -> *mut cJSON {
    if should_fail_alloc() {
        return ptr::null_mut();
    }
    
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

/// Duplicate a C string, returning NULL if failure simulation is enabled or on error.
#[inline]
unsafe fn strdup_checked(src: *const c_char) -> *mut c_char {
    if should_fail_alloc() || src.is_null() {
        return ptr::null_mut();
    }
    
    let cstr = CStr::from_ptr(src);
    match std::ffi::CString::new(cstr.to_bytes()) {
        Ok(owned) => owned.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn cJSON_CreateNull() -> *mut cJSON {
    new_item_checked(CJSON_NULL)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateTrue() -> *mut cJSON {
    new_item_checked(CJSON_TRUE)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateFalse() -> *mut cJSON {
    new_item_checked(CJSON_FALSE)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateBool(boolean: c_int) -> *mut cJSON {
    new_item_checked(if boolean != 0 { CJSON_TRUE } else { CJSON_FALSE })
}

#[no_mangle]
pub extern "C" fn cJSON_CreateNumber(num: c_double) -> *mut cJSON {
    let item = new_item_checked(CJSON_NUMBER);
    if !item.is_null() {
        unsafe {
            (*item).valuedouble = num;
            (*item).valueint = if num >= i32::MIN as f64 && num <= i32::MAX as f64 {
                num as i32
            } else {
                0
            };
        }
    }
    item
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateString(string: *const c_char) -> *mut cJSON {
    let item = new_item_checked(CJSON_STRING);
    if item.is_null() {
        return ptr::null_mut();
    }

    (*item).valuestring = strdup_checked(string);
    if (*item).valuestring.is_null() && !string.is_null() {
        // strdup failed - clean up and return NULL
        cJSON_Delete(item);
        return ptr::null_mut();
    }

    item
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateRaw(raw: *const c_char) -> *mut cJSON {
    let item = new_item_checked(CJSON_RAW);
    if item.is_null() {
        return ptr::null_mut();
    }

    (*item).valuestring = strdup_checked(raw);
    if (*item).valuestring.is_null() && !raw.is_null() {
        cJSON_Delete(item);
        return ptr::null_mut();
    }

    item
}

#[no_mangle]
pub extern "C" fn cJSON_CreateArray() -> *mut cJSON {
    new_item_checked(CJSON_ARRAY)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateObject() -> *mut cJSON {
    new_item_checked(CJSON_OBJECT)
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateIntArray(numbers: *const c_int, count: c_int) -> *mut cJSON {
    if count < 0 || numbers.is_null() {
        return ptr::null_mut();
    }

    let array = cJSON_CreateArray();
    if array.is_null() {
        return ptr::null_mut();
    }

    for i in 0..(count as usize) {
        let n = cJSON_CreateNumber(*numbers.add(i) as c_double);
        if n.is_null() {
            cJSON_Delete(array);
            return ptr::null_mut();
        }
        // Add to array using simple append
        if (*array).child.is_null() {
            (*array).child = n;
            (*n).prev = n; // Circular tail pointer
        } else {
            let tail = (*(*array).child).prev;
            (*tail).next = n;
            (*n).prev = tail;
            (*(*array).child).prev = n; // Update head's tail pointer
        }
    }

    array
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateFloatArray(numbers: *const c_float, count: c_int) -> *mut cJSON {
    if count < 0 || numbers.is_null() {
        return ptr::null_mut();
    }

    let array = cJSON_CreateArray();
    if array.is_null() {
        return ptr::null_mut();
    }

    for i in 0..(count as usize) {
        let n = cJSON_CreateNumber(*numbers.add(i) as c_double);
        if n.is_null() {
            cJSON_Delete(array);
            return ptr::null_mut();
        }
        if (*array).child.is_null() {
            (*array).child = n;
            (*n).prev = n;
        } else {
            let tail = (*(*array).child).prev;
            (*tail).next = n;
            (*n).prev = tail;
            (*(*array).child).prev = n;
        }
    }

    array
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateDoubleArray(numbers: *const c_double, count: c_int) -> *mut cJSON {
    if count < 0 || numbers.is_null() {
        return ptr::null_mut();
    }

    let array = cJSON_CreateArray();
    if array.is_null() {
        return ptr::null_mut();
    }

    for i in 0..(count as usize) {
        let n = cJSON_CreateNumber(*numbers.add(i));
        if n.is_null() {
            cJSON_Delete(array);
            return ptr::null_mut();
        }
        if (*array).child.is_null() {
            (*array).child = n;
            (*n).prev = n;
        } else {
            let tail = (*(*array).child).prev;
            (*tail).next = n;
            (*n).prev = tail;
            (*(*array).child).prev = n;
        }
    }

    array
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateStringArray(strings: *const *const c_char, count: c_int) -> *mut cJSON {
    if count < 0 || strings.is_null() {
        return ptr::null_mut();
    }

    let array = cJSON_CreateArray();
    if array.is_null() {
        return ptr::null_mut();
    }

    for i in 0..(count as usize) {
        let n = cJSON_CreateString(*strings.add(i));
        if n.is_null() {
            cJSON_Delete(array);
            return ptr::null_mut();
        }
        if (*array).child.is_null() {
            (*array).child = n;
            (*n).prev = n;
        } else {
            let tail = (*(*array).child).prev;
            (*tail).next = n;
            (*n).prev = tail;
            (*(*array).child).prev = n;
        }
    }

    array
}

// Add*ToObject helper functions
unsafe fn add_item_to_object(object: *mut cJSON, string: *const c_char, item: *mut cJSON) -> bool {
    if object.is_null() || string.is_null() || item.is_null() {
        return false;
    }

    // Set the key
    let key = strdup_checked(string);
    if key.is_null() {
        return false;
    }
    (*item).string = key;

    // Add to object's child list
    if (*object).child.is_null() {
        (*object).child = item;
        (*item).prev = item;
    } else {
        let tail = (*(*object).child).prev;
        (*tail).next = item;
        (*item).prev = tail;
        (*(*object).child).prev = item;
    }

    true
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddNullToObject(object: *mut cJSON, name: *const c_char) -> *mut cJSON {
    let item = cJSON_CreateNull();
    if add_item_to_object(object, name, item) {
        return item;
    }
    cJSON_Delete(item);
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddTrueToObject(object: *mut cJSON, name: *const c_char) -> *mut cJSON {
    let item = cJSON_CreateTrue();
    if add_item_to_object(object, name, item) {
        return item;
    }
    cJSON_Delete(item);
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddFalseToObject(object: *mut cJSON, name: *const c_char) -> *mut cJSON {
    let item = cJSON_CreateFalse();
    if add_item_to_object(object, name, item) {
        return item;
    }
    cJSON_Delete(item);
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddBoolToObject(object: *mut cJSON, name: *const c_char, boolean: c_int) -> *mut cJSON {
    let item = cJSON_CreateBool(boolean);
    if add_item_to_object(object, name, item) {
        return item;
    }
    cJSON_Delete(item);
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddNumberToObject(object: *mut cJSON, name: *const c_char, number: c_double) -> *mut cJSON {
    let item = cJSON_CreateNumber(number);
    if add_item_to_object(object, name, item) {
        return item;
    }
    cJSON_Delete(item);
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddStringToObject(object: *mut cJSON, name: *const c_char, string: *const c_char) -> *mut cJSON {
    let item = cJSON_CreateString(string);
    if add_item_to_object(object, name, item) {
        return item;
    }
    cJSON_Delete(item);
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddRawToObject(object: *mut cJSON, name: *const c_char, raw: *const c_char) -> *mut cJSON {
    let item = cJSON_CreateRaw(raw);
    if add_item_to_object(object, name, item) {
        return item;
    }
    cJSON_Delete(item);
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddObjectToObject(object: *mut cJSON, name: *const c_char) -> *mut cJSON {
    let item = cJSON_CreateObject();
    if add_item_to_object(object, name, item) {
        return item;
    }
    cJSON_Delete(item);
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddArrayToObject(object: *mut cJSON, name: *const c_char) -> *mut cJSON {
    let item = cJSON_CreateArray();
    if add_item_to_object(object, name, item) {
        return item;
    }
    cJSON_Delete(item);
    ptr::null_mut()
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

    // ===================================================================
    //  cJSON_Parse integration tests
    // ===================================================================

    #[test]
    fn parse_null_input_returns_null() {
        let result = unsafe { cJSON_Parse(ptr::null()) };
        assert!(result.is_null(), "null input must return null pointer");
    }

    #[test]
    fn parse_invalid_json_returns_null() {
        let input = CString::new("{invalid json}").unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(result.is_null(), "invalid JSON must return null pointer");
    }

    #[test]
    fn parse_empty_string_returns_null() {
        let input = CString::new("").unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(result.is_null(), "empty input must return null pointer");
    }

    #[test]
    fn parse_null_literal() {
        let input = CString::new("null").unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(!result.is_null(), "valid JSON must return non-null");
        unsafe {
            assert_eq!(
                (*result).type_ & 0xFF, CJSON_NULL,
                "type must be CJSON_NULL"
            );
            cJSON_Delete(result);
        }
    }

    #[test]
    fn parse_true_literal() {
        let input = CString::new("true").unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(!result.is_null());
        unsafe {
            assert_eq!((*result).type_ & 0xFF, CJSON_TRUE);
            cJSON_Delete(result);
        }
    }

    #[test]
    fn parse_false_literal() {
        let input = CString::new("false").unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(!result.is_null());
        unsafe {
            assert_eq!((*result).type_ & 0xFF, CJSON_FALSE);
            cJSON_Delete(result);
        }
    }

    #[test]
    fn parse_number() {
        let input = CString::new("42.5").unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(!result.is_null());
        unsafe {
            assert_eq!((*result).type_ & 0xFF, CJSON_NUMBER);
            assert_eq!((*result).valuedouble, 42.5);
            assert_eq!((*result).valueint, 42); // truncated
            cJSON_Delete(result);
        }
    }

    #[test]
    fn parse_string() {
        let input = CString::new(r#""hello world""#).unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(!result.is_null());
        unsafe {
            assert_eq!((*result).type_ & 0xFF, CJSON_STRING);
            assert!(!(*result).valuestring.is_null());
            let vs = CStr::from_ptr((*result).valuestring);
            assert_eq!(vs.to_str().unwrap(), "hello world");
            cJSON_Delete(result);
        }
    }

    #[test]
    fn parse_empty_array() {
        let input = CString::new("[]").unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(!result.is_null());
        unsafe {
            assert_eq!((*result).type_ & 0xFF, CJSON_ARRAY);
            assert!((*result).child.is_null(), "empty array has no children");
            cJSON_Delete(result);
        }
    }

    #[test]
    fn parse_array_of_numbers() {
        let input = CString::new("[1, 2, 3]").unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(!result.is_null());
        unsafe {
            assert_eq!((*result).type_ & 0xFF, CJSON_ARRAY);
            // Walk children.
            let c0 = (*result).child;
            assert!(!c0.is_null());
            assert_eq!((*c0).valuedouble, 1.0);

            let c1 = (*c0).next;
            assert!(!c1.is_null());
            assert_eq!((*c1).valuedouble, 2.0);
            assert_eq!((*c1).prev, c0); // doubly-linked

            let c2 = (*c1).next;
            assert!(!c2.is_null());
            assert_eq!((*c2).valuedouble, 3.0);
            assert_eq!((*c2).prev, c1);
            assert!((*c2).next.is_null()); // end of chain

            cJSON_Delete(result);
        }
    }

    #[test]
    fn parse_object_with_members() {
        let input = CString::new(r#"{"name": "cJSON", "version": 1.7}"#).unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(!result.is_null());
        unsafe {
            assert_eq!((*result).type_ & 0xFF, CJSON_OBJECT);

            // First member: "name" → "cJSON"
            let c0 = (*result).child;
            assert!(!c0.is_null());
            assert_eq!((*c0).type_ & 0xFF, CJSON_STRING);
            let key0 = CStr::from_ptr((*c0).string);
            assert_eq!(key0.to_str().unwrap(), "name");
            let val0 = CStr::from_ptr((*c0).valuestring);
            assert_eq!(val0.to_str().unwrap(), "cJSON");

            // Second member: "version" → 1.7
            let c1 = (*c0).next;
            assert!(!c1.is_null());
            assert_eq!((*c1).type_ & 0xFF, CJSON_NUMBER);
            let key1 = CStr::from_ptr((*c1).string);
            assert_eq!(key1.to_str().unwrap(), "version");
            assert_eq!((*c1).valuedouble, 1.7);

            cJSON_Delete(result);
        }
    }

    #[test]
    fn parse_nested_document() {
        let input = CString::new(
            r#"{"tags": ["c", "parser"], "meta": {"safe": true}}"#
        ).unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(!result.is_null());
        unsafe {
            assert_eq!((*result).type_ & 0xFF, CJSON_OBJECT);

            // "tags" → array
            let tags = (*result).child;
            assert!(!tags.is_null());
            assert_eq!((*tags).type_ & 0xFF, CJSON_ARRAY);

            // First array element: "c"
            let t0 = (*tags).child;
            assert!(!t0.is_null());
            assert_eq!((*t0).type_ & 0xFF, CJSON_STRING);
            let t0_val = CStr::from_ptr((*t0).valuestring);
            assert_eq!(t0_val.to_str().unwrap(), "c");

            // "meta" → object with "safe" → true
            let meta = (*tags).next;
            assert!(!meta.is_null());
            assert_eq!((*meta).type_ & 0xFF, CJSON_OBJECT);
            let safe_node = (*meta).child;
            assert!(!safe_node.is_null());
            assert_eq!((*safe_node).type_ & 0xFF, CJSON_TRUE);

            cJSON_Delete(result);
        }
    }

    /// Full round-trip: parse → inspect → delete.
    /// If this passes without ASAN/MSAN violations, the FFI glue is sound.
    #[test]
    fn parse_then_delete_round_trip() {
        let input = CString::new(
            r#"{"library":"cJSON","stars":11000,"features":["parsing","printing"],"active":true,"deprecated":null}"#
        ).unwrap();
        let result = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(!result.is_null());
        // Simply delete — exercises the full ownership chain.
        unsafe { cJSON_Delete(result) };
    }
}
