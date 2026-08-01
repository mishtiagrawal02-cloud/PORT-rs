//! # cJSON FFI Bindings — Port Mortem 2026 Hackathon
//!
//! This module provides the raw `extern "C"` FFI boundary that **exactly** matches the
//! ABI layout of [`cJSON.h`](file:///Users/manshaagarwal/github_new/cJSON/cJSON.h) (v1.7.19) and
//! [`cJSON_Utils.h`](file:///Users/manshaagarwal/github_new/cJSON/cJSON_Utils.h).
//!
//! ## Design Principles
//!
//! 1. Every struct is `#[repr(C)]` so its in-memory layout is identical to the C compiler's.
//! 2. Raw C pointers (`*mut cJSON`, `*const c_char`, …) are used at the boundary.
//! 3. **No parsing / mutation logic lives here** — this crate is the FFI *skeleton* only.
//! 4. A future safe wrapper module (`crate::safe`) will convert raw pointers into
//!    owned/borrowed Rust types (see per-function comments below).
//!
//! ## Safe-Wrapper Strategy (TODO: implement in `safe.rs`)
//!
//! | C type               | FFI type              | Safe Rust wrapper                        |
//! |----------------------|-----------------------|------------------------------------------|
//! | `cJSON *`            | `*mut cJSON`          | `Box<CJson>` (owned) or `&CJson` (ref)   |
//! | `const cJSON *`      | `*const cJSON`        | `&CJson`                                 |
//! | `char *` (returned)  | `*mut c_char`         | `CString` via `CString::from_raw`        |
//! | `const char *`       | `*const c_char`       | `&CStr` via `CStr::from_ptr`             |
//! | `cJSON_bool` / `int` | `c_int`               | `bool` with `!= 0` conversion            |
//!
//! Every function that returns a `*mut cJSON` transfers ownership to the caller.
//! The safe layer must guarantee that `cJSON_Delete` is called exactly once
//! (via `Drop` on the owning wrapper) and that no aliased `&mut` references exist.

#![allow(non_camel_case_types, non_snake_case)]

use std::os::raw::{c_char, c_double, c_float, c_int, c_void};

// ---------------------------------------------------------------------------
// Sub-modules
// ---------------------------------------------------------------------------

/// Safe memory management layer — `#![forbid(unsafe_code)]`.
/// Contains allocator-hook policy, deallocation planning, and Drop-based cleanup.
pub mod safe;

/// Arena-backed JSON AST — `#![forbid(unsafe_code)]`.
/// Uses `u32` indices instead of pointers/Box for tree structure.
pub mod arena;

/// Recursive descent JSON parser — `#![forbid(unsafe_code)]`.
/// Parses `&[u8]` into the arena-backed AST with IEEE 754 f64 precision.
pub mod parser;

/// FFI implementations of `cJSON_InitHooks` and `cJSON_Delete`.
/// These are `#[no_mangle] extern "C"` functions that C code links against.
/// Minimal `unsafe` at the boundary only; delegates to `safe` for all logic.
mod ffi_impl;

// Re-export the implemented functions so they can be used from examples
pub use ffi_impl::{cJSON_InitHooks, cJSON_Delete, cJSON_Parse};

// ---------------------------------------------------------------------------
// Version constants (cJSON.h lines 82-84)
// ---------------------------------------------------------------------------

pub const CJSON_VERSION_MAJOR: c_int = 1;
pub const CJSON_VERSION_MINOR: c_int = 7;
pub const CJSON_VERSION_PATCH: c_int = 19;

// ---------------------------------------------------------------------------
// Type-flag constants (cJSON.h lines 89-100)
//
// These are bit-flags; a node's `type` field is a bitwise-OR of exactly one
// value-type flag and zero or more modifier flags.
// ---------------------------------------------------------------------------

pub const CJSON_INVALID: c_int = 0;
pub const CJSON_FALSE:   c_int = 1 << 0;  // 1
pub const CJSON_TRUE:    c_int = 1 << 1;  // 2
pub const CJSON_NULL:    c_int = 1 << 2;  // 4
pub const CJSON_NUMBER:  c_int = 1 << 3;  // 8
pub const CJSON_STRING:  c_int = 1 << 4;  // 16
pub const CJSON_ARRAY:   c_int = 1 << 5;  // 32
pub const CJSON_OBJECT:  c_int = 1 << 6;  // 64
pub const CJSON_RAW:     c_int = 1 << 7;  // 128

/// Modifier: the `child` / `valuestring` pointer is a borrowed reference,
/// not owned — `cJSON_Delete` must NOT free it.
pub const CJSON_IS_REFERENCE:    c_int = 256;
/// Modifier: `string` (the key name) points to a `const` / static string.
pub const CJSON_STRING_IS_CONST: c_int = 512;

// ---------------------------------------------------------------------------
// Safety-limit constants (cJSON.h lines 136-144)
// ---------------------------------------------------------------------------

pub const CJSON_NESTING_LIMIT:  c_int = 1000;
pub const CJSON_CIRCULAR_LIMIT: c_int = 10000;

// ---------------------------------------------------------------------------
// cJSON_bool typedef (cJSON.h line 132)
// ---------------------------------------------------------------------------

/// Direct alias for the C `typedef int cJSON_bool;`
pub type cJSON_bool = c_int;

// ---------------------------------------------------------------------------
// Core struct: cJSON (cJSON.h lines 103-123)
//
// Intrusive doubly-linked list + child pointer for tree structure.
//
// ## Safe-wrapper note
// In the safe layer we will wrap `*mut cJSON` in an owning `CJson` struct
// that implements `Drop` → `cJSON_Delete`.  Traversal helpers will yield
// `&CJson` borrows tied to the root's lifetime so the borrow checker
// prevents use-after-free at compile time.
// ---------------------------------------------------------------------------

/// ABI-compatible mirror of `struct cJSON` (cJSON.h:103-123).
///
/// All pointer fields are nullable (`*mut` / `*mut c_char`).
/// The `type` field is a bitwise-OR of the `CJSON_*` constants above.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct cJSON {
    /// Next sibling in the array / object linked list.
    pub next: *mut cJSON,
    /// Previous sibling in the array / object linked list.
    pub prev: *mut cJSON,
    /// First child (head of the sub-list for arrays / objects).
    pub child: *mut cJSON,

    /// Bitwise-OR of `CJSON_*` type and modifier flags.
    pub type_: c_int,

    /// String payload when `type_ & CJSON_STRING` or `type_ & CJSON_RAW`.
    /// Owned by cJSON unless `CJSON_IS_REFERENCE` is set.
    pub valuestring: *mut c_char,
    /// **DEPRECATED** — integer snapshot of `valuedouble`. Use
    /// `cJSON_SetNumberValue` instead.
    pub valueint: c_int,
    /// Numeric payload when `type_ & CJSON_NUMBER`.
    pub valuedouble: c_double,

    /// Key name when this node is a child of an object.
    /// Owned by cJSON unless `CJSON_STRING_IS_CONST` is set.
    pub string: *mut c_char,
}

// ---------------------------------------------------------------------------
// Hook struct: cJSON_Hooks (cJSON.h lines 125-130)
//
// Allows callers to redirect the allocator. On *nix the calling convention
// is the default C ABI (`extern "C"`); on Windows the C header forces
// `__cdecl` which is also the Rust default for `extern "C"` on that target.
// ---------------------------------------------------------------------------

/// Function-pointer type matching `void *(CJSON_CDECL *malloc_fn)(size_t)`.
pub type cJSON_MallocFn = Option<unsafe extern "C" fn(sz: usize) -> *mut c_void>;
/// Function-pointer type matching `void (CJSON_CDECL *free_fn)(void *)`.
pub type cJSON_FreeFn = Option<unsafe extern "C" fn(ptr: *mut c_void)>;

/// ABI-compatible mirror of `struct cJSON_Hooks` (cJSON.h:125-130).
///
/// Both fields are `Option<…>` so a NULL function pointer is representable.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct cJSON_Hooks {
    pub malloc_fn: cJSON_MallocFn,
    pub free_fn:   cJSON_FreeFn,
}

// ===========================================================================
//  extern "C" function signatures — cJSON.h
// ===========================================================================
//
//  ## Safe-wrapper strategy for pointer-returning functions
//
//  Every function that returns `*mut cJSON` **transfers ownership** to the
//  caller.  In the safe layer each such return will be wrapped in:
//
//  ```rust,ignore
//  pub struct CJson {
//      raw: ptr::NonNull<cJSON>,  // invariant: always obtained from cJSON_*Create / cJSON_Parse
//  }
//  impl Drop for CJson {
//      fn drop(&mut self) { unsafe { cJSON_Delete(self.raw.as_ptr()) } }
//  }
//  ```
//
//  Functions returning `*mut c_char` (e.g. `cJSON_Print`) produce strings
//  that must be freed with `cJSON_free` (or the hook-provided `free_fn`).
//  The safe wrapper will return `CString` and arrange cleanup accordingly.
// ===========================================================================

extern "C" {
    // -----------------------------------------------------------------------
    // Version
    // -----------------------------------------------------------------------

    /// Returns the version of cJSON as a NUL-terminated static string.
    pub fn cJSON_Version() -> *const c_char;

    // -----------------------------------------------------------------------
    // Allocator hooks
    //
    // NOTE: cJSON_InitHooks is now IMPLEMENTED in Rust (see ffi_impl.rs).
    // It is exported as #[no_mangle] extern "C" — NOT imported here.
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Parsing
    //
    // NOTE: cJSON_Parse is now IMPLEMENTED in Rust (see ffi_impl.rs).
    // It is exported as #[no_mangle] extern "C" — NOT imported here.
    //
    // Safe-wrapper note:  Each parser returns a *mut cJSON that the caller
    // owns.  The safe layer will return `Option<CJson>` (None on parse
    // failure) and the Drop impl calls cJSON_Delete.
    // -----------------------------------------------------------------------

    /// Parse JSON from a buffer of known length. Returns NULL on failure.
    pub fn cJSON_ParseWithLength(value: *const c_char, buffer_length: usize) -> *mut cJSON;

    /// Extended parse: optionally require NUL termination and retrieve the
    /// pointer to the final byte parsed (or the error location on failure).
    pub fn cJSON_ParseWithOpts(
        value: *const c_char,
        return_parse_end: *mut *const c_char,
        require_null_terminated: cJSON_bool,
    ) -> *mut cJSON;

    /// Combines `ParseWithLength` + `ParseWithOpts`.
    pub fn cJSON_ParseWithLengthOpts(
        value: *const c_char,
        buffer_length: usize,
        return_parse_end: *mut *const c_char,
        require_null_terminated: cJSON_bool,
    ) -> *mut cJSON;

    // -----------------------------------------------------------------------
    // Rendering (printing)
    //
    // Safe-wrapper note:  Returned `*mut c_char` must be freed via
    // `cJSON_free` (or stdlib `free` / hook `free_fn`).  The safe layer
    // will wrap in a `CString` and free in its Drop.
    // -----------------------------------------------------------------------

    /// Render a cJSON tree to a pretty-printed, NUL-terminated string.
    pub fn cJSON_Print(item: *const cJSON) -> *mut c_char;

    /// Render without whitespace formatting.
    pub fn cJSON_PrintUnformatted(item: *const cJSON) -> *mut c_char;

    /// Render with a pre-allocated buffer hint.  `fmt` = 1 for formatted.
    pub fn cJSON_PrintBuffered(
        item: *const cJSON,
        prebuffer: c_int,
        fmt: cJSON_bool,
    ) -> *mut c_char;

    /// Render into a caller-supplied buffer. Returns 1 on success, 0 on failure.
    pub fn cJSON_PrintPreallocated(
        item: *mut cJSON,
        buffer: *mut c_char,
        length: c_int,
        format: cJSON_bool,
    ) -> cJSON_bool;

    // -----------------------------------------------------------------------
    // Lifetime management
    //
    // NOTE: cJSON_Delete is now IMPLEMENTED in Rust (see ffi_impl.rs).
    // It is exported as #[no_mangle] extern "C" — NOT imported here.
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Array / Object accessors
    // -----------------------------------------------------------------------

    /// Returns the number of items in an array (or object).
    pub fn cJSON_GetArraySize(array: *const cJSON) -> c_int;

    /// Retrieve item at `index` from an array.  Returns NULL if out of bounds.
    ///
    /// Safe-wrapper note: returns `Option<&CJson>` tied to the parent's
    /// lifetime — no ownership transfer.
    pub fn cJSON_GetArrayItem(array: *const cJSON, index: c_int) -> *mut cJSON;

    /// Case-**insensitive** key lookup on an object.
    pub fn cJSON_GetObjectItem(
        object: *const cJSON,
        string: *const c_char,
    ) -> *mut cJSON;

    /// Case-**sensitive** key lookup on an object.
    pub fn cJSON_GetObjectItemCaseSensitive(
        object: *const cJSON,
        string: *const c_char,
    ) -> *mut cJSON;

    /// Returns non-zero if the object contains `string` as a key.
    pub fn cJSON_HasObjectItem(
        object: *const cJSON,
        string: *const c_char,
    ) -> cJSON_bool;

    // -----------------------------------------------------------------------
    // Error reporting
    // -----------------------------------------------------------------------

    /// Returns a pointer into the most-recently-failed parse input at the
    /// approximate error location.  Valid only after a failed `cJSON_Parse`.
    pub fn cJSON_GetErrorPtr() -> *const c_char;

    // -----------------------------------------------------------------------
    // Value getters
    // -----------------------------------------------------------------------

    /// Returns `valuestring` if the item is a `cJSON_String`, else NULL.
    pub fn cJSON_GetStringValue(item: *const cJSON) -> *mut c_char;

    /// Returns `valuedouble` if the item is a `cJSON_Number`, else NaN/0.
    pub fn cJSON_GetNumberValue(item: *const cJSON) -> c_double;

    // -----------------------------------------------------------------------
    // Type predicates
    //
    // Each returns non-zero (truthy) if the item matches the given type.
    // Safe-wrapper note: these become `fn is_xxx(&self) -> bool` methods.
    // -----------------------------------------------------------------------

    pub fn cJSON_IsInvalid(item: *const cJSON) -> cJSON_bool;
    pub fn cJSON_IsFalse(item: *const cJSON) -> cJSON_bool;
    pub fn cJSON_IsTrue(item: *const cJSON) -> cJSON_bool;
    pub fn cJSON_IsBool(item: *const cJSON) -> cJSON_bool;
    pub fn cJSON_IsNull(item: *const cJSON) -> cJSON_bool;
    pub fn cJSON_IsNumber(item: *const cJSON) -> cJSON_bool;
    pub fn cJSON_IsString(item: *const cJSON) -> cJSON_bool;
    pub fn cJSON_IsArray(item: *const cJSON) -> cJSON_bool;
    pub fn cJSON_IsObject(item: *const cJSON) -> cJSON_bool;
    pub fn cJSON_IsRaw(item: *const cJSON) -> cJSON_bool;

    // -----------------------------------------------------------------------
    // Constructors — atomic values
    //
    // Safe-wrapper note: each returns an owned `*mut cJSON`.  The safe
    // layer wraps in `CJson` (Drop → cJSON_Delete).
    // -----------------------------------------------------------------------

    pub fn cJSON_CreateNull() -> *mut cJSON;
    pub fn cJSON_CreateTrue() -> *mut cJSON;
    pub fn cJSON_CreateFalse() -> *mut cJSON;
    pub fn cJSON_CreateBool(boolean: cJSON_bool) -> *mut cJSON;
    pub fn cJSON_CreateNumber(num: c_double) -> *mut cJSON;
    pub fn cJSON_CreateString(string: *const c_char) -> *mut cJSON;
    pub fn cJSON_CreateRaw(raw: *const c_char) -> *mut cJSON;
    pub fn cJSON_CreateArray() -> *mut cJSON;
    pub fn cJSON_CreateObject() -> *mut cJSON;

    // -----------------------------------------------------------------------
    // Constructors — references (non-owning)
    //
    // The returned node has `CJSON_IS_REFERENCE` set; cJSON_Delete will
    // NOT free the referenced child / string.
    // -----------------------------------------------------------------------

    pub fn cJSON_CreateStringReference(string: *const c_char) -> *mut cJSON;
    pub fn cJSON_CreateObjectReference(child: *const cJSON) -> *mut cJSON;
    pub fn cJSON_CreateArrayReference(child: *const cJSON) -> *mut cJSON;

    // -----------------------------------------------------------------------
    // Constructors — bulk array creation
    // -----------------------------------------------------------------------

    pub fn cJSON_CreateIntArray(numbers: *const c_int, count: c_int) -> *mut cJSON;
    pub fn cJSON_CreateFloatArray(numbers: *const c_float, count: c_int) -> *mut cJSON;
    pub fn cJSON_CreateDoubleArray(numbers: *const c_double, count: c_int) -> *mut cJSON;
    pub fn cJSON_CreateStringArray(strings: *const *const c_char, count: c_int) -> *mut cJSON;

    // -----------------------------------------------------------------------
    // Mutation — adding items
    //
    // Safe-wrapper note: `cJSON_AddItemToArray` et al. **consume** `item`
    // (ownership transfers to the parent).  The safe layer must use
    // `std::mem::forget` on the wrapper to avoid double-free.
    // -----------------------------------------------------------------------

    pub fn cJSON_AddItemToArray(array: *mut cJSON, item: *mut cJSON) -> cJSON_bool;
    pub fn cJSON_AddItemToObject(
        object: *mut cJSON,
        string: *const c_char,
        item: *mut cJSON,
    ) -> cJSON_bool;
    /// Adds with a **const** key (sets `CJSON_STRING_IS_CONST` on the item).
    pub fn cJSON_AddItemToObjectCS(
        object: *mut cJSON,
        string: *const c_char,
        item: *mut cJSON,
    ) -> cJSON_bool;
    pub fn cJSON_AddItemReferenceToArray(array: *mut cJSON, item: *mut cJSON) -> cJSON_bool;
    pub fn cJSON_AddItemReferenceToObject(
        object: *mut cJSON,
        string: *const c_char,
        item: *mut cJSON,
    ) -> cJSON_bool;

    // -----------------------------------------------------------------------
    // Mutation — detaching / deleting items
    // -----------------------------------------------------------------------

    pub fn cJSON_DetachItemViaPointer(
        parent: *mut cJSON,
        item: *mut cJSON,
    ) -> *mut cJSON;
    pub fn cJSON_DetachItemFromArray(array: *mut cJSON, which: c_int) -> *mut cJSON;
    pub fn cJSON_DeleteItemFromArray(array: *mut cJSON, which: c_int);
    pub fn cJSON_DetachItemFromObject(
        object: *mut cJSON,
        string: *const c_char,
    ) -> *mut cJSON;
    pub fn cJSON_DetachItemFromObjectCaseSensitive(
        object: *mut cJSON,
        string: *const c_char,
    ) -> *mut cJSON;
    pub fn cJSON_DeleteItemFromObject(object: *mut cJSON, string: *const c_char);
    pub fn cJSON_DeleteItemFromObjectCaseSensitive(object: *mut cJSON, string: *const c_char);

    // -----------------------------------------------------------------------
    // Mutation — inserting / replacing items
    // -----------------------------------------------------------------------

    pub fn cJSON_InsertItemInArray(
        array: *mut cJSON,
        which: c_int,
        newitem: *mut cJSON,
    ) -> cJSON_bool;
    pub fn cJSON_ReplaceItemViaPointer(
        parent: *mut cJSON,
        item: *mut cJSON,
        replacement: *mut cJSON,
    ) -> cJSON_bool;
    pub fn cJSON_ReplaceItemInArray(
        array: *mut cJSON,
        which: c_int,
        newitem: *mut cJSON,
    ) -> cJSON_bool;
    pub fn cJSON_ReplaceItemInObject(
        object: *mut cJSON,
        string: *const c_char,
        newitem: *mut cJSON,
    ) -> cJSON_bool;
    pub fn cJSON_ReplaceItemInObjectCaseSensitive(
        object: *mut cJSON,
        string: *const c_char,
        newitem: *mut cJSON,
    ) -> cJSON_bool;

    // -----------------------------------------------------------------------
    // Duplication & comparison
    // -----------------------------------------------------------------------

    /// Deep-copy a cJSON tree.  If `recurse` is non-zero, children are
    /// cloned recursively.  `next` / `prev` on the returned root are NULL.
    pub fn cJSON_Duplicate(item: *const cJSON, recurse: cJSON_bool) -> *mut cJSON;

    /// Recursively compare two trees.  `case_sensitive` controls whether
    /// object keys are compared case-sensitively.
    pub fn cJSON_Compare(
        a: *const cJSON,
        b: *const cJSON,
        case_sensitive: cJSON_bool,
    ) -> cJSON_bool;

    // -----------------------------------------------------------------------
    // In-place minification
    // -----------------------------------------------------------------------

    /// Strips whitespace from a mutable JSON string **in place**.
    pub fn cJSON_Minify(json: *mut c_char);

    // -----------------------------------------------------------------------
    // Convenience helpers — create + add in one call
    //
    // Each returns the *added* child node (non-owning from the caller's
    // perspective — the parent now owns it).
    // -----------------------------------------------------------------------

    pub fn cJSON_AddNullToObject(
        object: *mut cJSON,
        name: *const c_char,
    ) -> *mut cJSON;
    pub fn cJSON_AddTrueToObject(
        object: *mut cJSON,
        name: *const c_char,
    ) -> *mut cJSON;
    pub fn cJSON_AddFalseToObject(
        object: *mut cJSON,
        name: *const c_char,
    ) -> *mut cJSON;
    pub fn cJSON_AddBoolToObject(
        object: *mut cJSON,
        name: *const c_char,
        boolean: cJSON_bool,
    ) -> *mut cJSON;
    pub fn cJSON_AddNumberToObject(
        object: *mut cJSON,
        name: *const c_char,
        number: c_double,
    ) -> *mut cJSON;
    pub fn cJSON_AddStringToObject(
        object: *mut cJSON,
        name: *const c_char,
        string: *const c_char,
    ) -> *mut cJSON;
    pub fn cJSON_AddRawToObject(
        object: *mut cJSON,
        name: *const c_char,
        raw: *const c_char,
    ) -> *mut cJSON;
    pub fn cJSON_AddObjectToObject(
        object: *mut cJSON,
        name: *const c_char,
    ) -> *mut cJSON;
    pub fn cJSON_AddArrayToObject(
        object: *mut cJSON,
        name: *const c_char,
    ) -> *mut cJSON;

    // -----------------------------------------------------------------------
    // Number / string value setters
    // -----------------------------------------------------------------------

    /// Helper for the C macro `cJSON_SetNumberValue`.  Sets both
    /// `valuedouble` and (truncated) `valueint`.
    pub fn cJSON_SetNumberHelper(object: *mut cJSON, number: c_double) -> c_double;

    /// Replace `valuestring` on a `cJSON_String` node.  Returns the new
    /// string pointer, or NULL on error.
    pub fn cJSON_SetValuestring(
        object: *mut cJSON,
        valuestring: *const c_char,
    ) -> *mut c_char;

    // -----------------------------------------------------------------------
    // Hook-aware allocator pass-throughs
    // -----------------------------------------------------------------------

    /// Allocate via the currently-installed malloc hook (or stdlib `malloc`).
    pub fn cJSON_malloc(size: usize) -> *mut c_void;

    /// Free via the currently-installed free hook (or stdlib `free`).
    pub fn cJSON_free(object: *mut c_void);
}

// ===========================================================================
//  extern "C" function signatures — cJSON_Utils.h
//
//  RFC 6901 (JSON Pointer), RFC 6902 (JSON Patch), RFC 7386 (Merge Patch)
// ===========================================================================

extern "C" {
    // -----------------------------------------------------------------------
    // JSON Pointer (RFC 6901)
    // -----------------------------------------------------------------------

    pub fn cJSONUtils_GetPointer(
        object: *mut cJSON,
        pointer: *const c_char,
    ) -> *mut cJSON;

    pub fn cJSONUtils_GetPointerCaseSensitive(
        object: *mut cJSON,
        pointer: *const c_char,
    ) -> *mut cJSON;

    // -----------------------------------------------------------------------
    // JSON Patch (RFC 6902)
    // -----------------------------------------------------------------------

    /// Generate a JSON Patch array describing the diff from `from` → `to`.
    /// **Warning:** sorts the elements of both `from` and `to` in place.
    pub fn cJSONUtils_GeneratePatches(
        from: *mut cJSON,
        to: *mut cJSON,
    ) -> *mut cJSON;

    pub fn cJSONUtils_GeneratePatchesCaseSensitive(
        from: *mut cJSON,
        to: *mut cJSON,
    ) -> *mut cJSON;

    pub fn cJSONUtils_AddPatchToArray(
        array: *mut cJSON,
        operation: *const c_char,
        path: *const c_char,
        value: *const cJSON,
    );

    /// Apply a JSON Patch array to `object`.  Returns 0 on success.
    /// **Not atomic** — on failure, `object` may be partially modified.
    pub fn cJSONUtils_ApplyPatches(
        object: *mut cJSON,
        patches: *const cJSON,
    ) -> c_int;

    pub fn cJSONUtils_ApplyPatchesCaseSensitive(
        object: *mut cJSON,
        patches: *const cJSON,
    ) -> c_int;

    // -----------------------------------------------------------------------
    // JSON Merge Patch (RFC 7386)
    // -----------------------------------------------------------------------

    /// Apply a merge-patch to `target` **in place**.  Returns the (possibly
    /// new) root pointer — the caller must update their pointer.
    pub fn cJSONUtils_MergePatch(
        target: *mut cJSON,
        patch: *const cJSON,
    ) -> *mut cJSON;

    pub fn cJSONUtils_MergePatchCaseSensitive(
        target: *mut cJSON,
        patch: *const cJSON,
    ) -> *mut cJSON;

    /// Generate a merge-patch that transforms `from` into `to`.
    /// **Warning:** sorts both trees' keys in place.
    pub fn cJSONUtils_GenerateMergePatch(
        from: *mut cJSON,
        to: *mut cJSON,
    ) -> *mut cJSON;

    pub fn cJSONUtils_GenerateMergePatchCaseSensitive(
        from: *mut cJSON,
        to: *mut cJSON,
    ) -> *mut cJSON;

    // -----------------------------------------------------------------------
    // Pointer path resolution
    // -----------------------------------------------------------------------

    /// Walk `object` to find `target` and return the JSON Pointer string
    /// from root to target.  Caller must free the returned string.
    pub fn cJSONUtils_FindPointerFromObjectTo(
        object: *const cJSON,
        target: *const cJSON,
    ) -> *mut c_char;

    // -----------------------------------------------------------------------
    // Object key sorting
    // -----------------------------------------------------------------------

    pub fn cJSONUtils_SortObject(object: *mut cJSON);
    pub fn cJSONUtils_SortObjectCaseSensitive(object: *mut cJSON);
}

// ===========================================================================
//  Rust-side inline helpers — mirror the C macros from cJSON.h
// ===========================================================================

/// Rust equivalent of the C macro `cJSON_SetIntValue(object, number)`.
///
/// # Safety
/// `object` must be a valid, non-null pointer to a `cJSON` node.
#[inline]
pub unsafe fn cJSON_SetIntValue(object: *mut cJSON, number: c_double) {
    if !object.is_null() {
        (*object).valueint = number as c_int;
        (*object).valuedouble = number;
    }
}

/// Rust equivalent of the C macro `cJSON_SetNumberValue(object, number)`.
///
/// # Safety
/// `object` must be a valid, non-null pointer to a `cJSON` node.
#[inline]
pub unsafe fn cJSON_SetNumberValue(object: *mut cJSON, number: c_double) -> c_double {
    if !object.is_null() {
        cJSON_SetNumberHelper(object, number)
    } else {
        number
    }
}

/// Rust equivalent of the C macro `cJSON_SetBoolValue(object, bool_value)`.
///
/// # Safety
/// `object` must be a valid, non-null pointer to a `cJSON` node.
#[inline]
pub unsafe fn cJSON_SetBoolValue(object: *mut cJSON, bool_value: bool) -> c_int {
    if !object.is_null() && ((*object).type_ & (CJSON_FALSE | CJSON_TRUE)) != 0 {
        (*object).type_ = ((*object).type_ & !(CJSON_FALSE | CJSON_TRUE))
            | if bool_value { CJSON_TRUE } else { CJSON_FALSE };
        (*object).type_
    } else {
        CJSON_INVALID
    }
}

// ===========================================================================
//  Iterator helper — mirrors `cJSON_ArrayForEach` macro
// ===========================================================================

/// Yields each child of `array` as `*mut cJSON`, walking the linked list.
///
/// # Safety
/// `array` must be a valid pointer to a `cJSON` array/object node (or null).
///
/// # Example (unsafe FFI code)
/// ```rust,ignore
/// unsafe {
///     for child in CJsonIter::new(array_ptr) {
///         // child: *mut cJSON
///     }
/// }
/// ```
pub struct CJsonIter {
    current: *mut cJSON,
}

impl CJsonIter {
    /// # Safety
    /// `array` must be null or point to a valid `cJSON` node.
    #[inline]
    pub unsafe fn new(array: *mut cJSON) -> Self {
        CJsonIter {
            current: if array.is_null() {
                std::ptr::null_mut()
            } else {
                (*array).child
            },
        }
    }
}

impl Iterator for CJsonIter {
    type Item = *mut cJSON;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() {
            None
        } else {
            let node = self.current;
            // SAFETY: caller guarantees the linked list is well-formed.
            self.current = unsafe { (*node).next };
            Some(node)
        }
    }
}

// ===========================================================================
//  Compile-time sanity checks
// ===========================================================================

#[cfg(test)]
mod layout_tests {
    use super::*;
    use std::mem;

    /// Verify that our `cJSON` struct has the same size / alignment that the
    /// C compiler would produce.  These values are for 64-bit targets
    /// (LP64 / LLP64).  If this crate is used on 32-bit, update accordingly.
    #[test]
    fn cjson_struct_size_and_alignment() {
        // On 64-bit: 6 pointers (48) + int (4) + padding (4) + int (4) +
        // padding (4) + double (8) = 72 bytes, 8-byte aligned.
        let size = mem::size_of::<cJSON>();
        let align = mem::align_of::<cJSON>();
        assert!(
            align <= 8,
            "cJSON alignment {align} exceeds expected max of 8"
        );
        // Size must be a multiple of alignment
        assert_eq!(
            size % align,
            0,
            "cJSON size {size} is not a multiple of alignment {align}"
        );
        // Sanity: struct must contain at least 5 pointers + 1 int + 1 double
        let min_size = 5 * mem::size_of::<*mut u8>()
            + mem::size_of::<c_int>()
            + mem::size_of::<c_double>();
        assert!(
            size >= min_size,
            "cJSON size {size} is smaller than theoretical minimum {min_size}"
        );
    }

    #[test]
    fn hooks_struct_is_two_pointers() {
        assert_eq!(
            mem::size_of::<cJSON_Hooks>(),
            2 * mem::size_of::<*mut u8>(),
            "cJSON_Hooks must be exactly two pointer-sized fields"
        );
    }

    #[test]
    fn cjson_bool_is_c_int() {
        assert_eq!(mem::size_of::<cJSON_bool>(), mem::size_of::<c_int>());
    }
}
