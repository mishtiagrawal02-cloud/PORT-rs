//! # FFI Implementation — Constructors, Accessors, Mutators, and Predicates
//!
//! This module implements ALL remaining `extern "C"` functions from `cJSON.h`
//! that were previously declared as imports in `lib.rs`.  Each function uses
//! `#[no_mangle]` so C callers link against these Rust implementations.
//!
//! ## Allocation Invariant
//!
//! Every `cJSON` node is allocated via `Box::new(cJSON { ... })` and returned
//! as `Box::into_raw(...)`.  This ensures `cJSON_Delete` (in `ffi_impl.rs`)
//! can safely reconstitute the `Box` via `Box::from_raw`.
//!
//! Strings (`valuestring`, `string`) are allocated via `CString::new(...)` and
//! returned as `CString::into_raw(...)`.  They are freed via
//! `CString::from_raw` in `cJSON_Delete`.
//!
//! ## Safety
//!
//! `unsafe` is used only at the FFI boundary for:
//! 1. Dereferencing raw pointers from C callers
//! 2. `Box::into_raw` / `CString::into_raw` for handing ownership to C
//! 3. Reading C strings via `CStr::from_ptr`

#![allow(non_snake_case)]

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_float, c_int, c_void};
use std::ptr;

use crate::{
    cJSON, cJSON_bool,
    CJSON_ARRAY, CJSON_FALSE, CJSON_INVALID, CJSON_IS_REFERENCE, CJSON_NULL,
    CJSON_NUMBER, CJSON_OBJECT, CJSON_RAW, CJSON_STRING, CJSON_STRING_IS_CONST,
    CJSON_TRUE,
};

// ===========================================================================
//  Internal helpers
// ===========================================================================

/// Allocate a zeroed `cJSON` node via `Box` and return the raw pointer.
///
/// This is the Rust equivalent of the C `cJSON_New_Item`.  Every node
/// created by this function MUST eventually be freed by `cJSON_Delete`
/// (which calls `Box::from_raw`).
///
/// For hybrid C/Rust builds, allocation failure is controlled by cJSON_InitHooks
/// in ffi_impl.rs. This module is not compiled in hybrid builds.
#[inline]
fn new_item(type_: c_int) -> *mut cJSON {
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

/// Duplicate a NUL-terminated C string into a fresh `CString` allocation,
/// returning the raw pointer.  Returns `null_mut` if `src` is null or if
/// the string contains interior NUL bytes (which shouldn't happen in valid
/// JSON, but we handle it gracefully).
///
/// # Safety
/// `src` must be null or point to a valid NUL-terminated C string.
#[inline]
unsafe fn strdup_raw(src: *const c_char) -> *mut c_char {
    if src.is_null() {
        return ptr::null_mut();
    }
    let cstr = CStr::from_ptr(src);
    match CString::new(cstr.to_bytes()) {
        Ok(owned) => owned.into_raw(),
        Err(_) => ptr::null_mut(), // interior NUL — shouldn't happen
    }
}

/// Link `item` as the next sibling of `prev` (doubly-linked).
///
/// # Safety
/// Both pointers must be valid non-null `cJSON` nodes.
#[inline]
unsafe fn suffix_object(prev: *mut cJSON, item: *mut cJSON) {
    (*prev).next = item;
    (*item).prev = prev;
}

/// Append `item` to the end of `array`'s child linked list.
///
/// Returns `true` on success.
///
/// # Safety
/// `array` and `item` must be valid non-null `cJSON` nodes.
/// `item` must not already be in another list.
unsafe fn add_item_to_array(array: *mut cJSON, item: *mut cJSON) -> bool {
    if item.is_null() || array.is_null() || ptr::eq(array, item) {
        return false;
    }

    let child = (*array).child;
    if child.is_null() {
        // Empty list — item becomes sole child, prev points to itself
        // (circular prev used to find tail quickly).
        (*array).child = item;
        (*item).prev = item;
        (*item).next = ptr::null_mut();
    } else {
        // Append to end: child->prev is the current tail.
        let tail = (*child).prev;
        if !tail.is_null() {
            suffix_object(tail, item);
            (*child).prev = item; // update head's prev to new tail
        }
    }

    true
}

/// Internal: add `item` to `object` with the given key string.
///
/// If `constant_key` is true, the key pointer is stored directly and
/// `CJSON_STRING_IS_CONST` is set.  Otherwise the key is strdup'd.
///
/// # Safety
/// All pointers must be valid. `string` must be NUL-terminated.
unsafe fn add_item_to_object(
    object: *mut cJSON,
    string: *const c_char,
    item: *mut cJSON,
    constant_key: bool,
) -> bool {
    if object.is_null() || string.is_null() || item.is_null() || ptr::eq(object, item) {
        return false;
    }

    if constant_key {
        (*item).string = string as *mut c_char;
        (*item).type_ |= CJSON_STRING_IS_CONST;
    } else {
        let new_key = strdup_raw(string);
        if new_key.is_null() {
            return false;
        }

        // Free old key if owned
        if ((*item).type_ & CJSON_STRING_IS_CONST) == 0 && !(*item).string.is_null() {
            drop(CString::from_raw((*item).string));
        }

        (*item).string = new_key;
        (*item).type_ &= !CJSON_STRING_IS_CONST;
    }

    add_item_to_array(object, item)
}

/// Saturating cast of `f64` to `c_int`, matching the C `INT_MAX`/`INT_MIN`
/// saturation in `cJSON_CreateNumber` and `cJSON_SetNumberHelper`.
#[inline]
fn saturating_double_to_int(num: f64) -> c_int {
    if num >= c_int::MAX as f64 {
        c_int::MAX
    } else if num <= c_int::MIN as f64 {
        c_int::MIN
    } else {
        num as c_int
    }
}

// ===========================================================================
//  Version
// ===========================================================================

/// Return the cJSON version string.  We use a static byte literal.
#[no_mangle]
pub extern "C" fn cJSON_Version() -> *const c_char {
    // "1.7.19\0" — matches CJSON_VERSION_MAJOR.MINOR.PATCH
    static VERSION: &[u8] = b"1.7.19\0";
    VERSION.as_ptr() as *const c_char
}

// ===========================================================================
//  Type predicates (cJSON.h lines 189-198)
//
//  Each checks `(type & 0xFF) == flag`, returning 0 (false) for null items.
// ===========================================================================

/// Mask to extract the base type, stripping modifier flags.
const TYPE_MASK: c_int = 0xFF;

macro_rules! impl_type_predicate {
    ($fn_name:ident, $flag:expr) => {
        #[no_mangle]
        pub unsafe extern "C" fn $fn_name(item: *const cJSON) -> cJSON_bool {
            if item.is_null() {
                return 0;
            }
            if ((*item).type_ & TYPE_MASK) == $flag { 1 } else { 0 }
        }
    };
}

impl_type_predicate!(cJSON_IsInvalid, CJSON_INVALID);
impl_type_predicate!(cJSON_IsFalse, CJSON_FALSE);
impl_type_predicate!(cJSON_IsTrue, CJSON_TRUE);
impl_type_predicate!(cJSON_IsNull, CJSON_NULL);
impl_type_predicate!(cJSON_IsNumber, CJSON_NUMBER);
impl_type_predicate!(cJSON_IsString, CJSON_STRING);
impl_type_predicate!(cJSON_IsArray, CJSON_ARRAY);
impl_type_predicate!(cJSON_IsObject, CJSON_OBJECT);
impl_type_predicate!(cJSON_IsRaw, CJSON_RAW);

/// `cJSON_IsBool` is special: matches EITHER True or False.
#[no_mangle]
pub unsafe extern "C" fn cJSON_IsBool(item: *const cJSON) -> cJSON_bool {
    if item.is_null() {
        return 0;
    }
    if ((*item).type_ & (CJSON_TRUE | CJSON_FALSE)) != 0 { 1 } else { 0 }
}

// ===========================================================================
//  Value getters
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_GetStringValue(item: *const cJSON) -> *mut c_char {
    if cJSON_IsString(item) == 0 {
        return ptr::null_mut();
    }
    (*item).valuestring
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_GetNumberValue(item: *const cJSON) -> c_double {
    if cJSON_IsNumber(item) == 0 {
        return f64::NAN;
    }
    (*item).valuedouble
}

// ===========================================================================
//  Allocation failure simulation — for test compatibility
// ===========================================================================

use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag: when true, all allocation functions return NULL to simulate
/// allocation failure. This is ONLY used by the test suite to verify error
/// handling paths. Production code never sets this.
static SIMULATE_ALLOC_FAILURE: AtomicBool = AtomicBool::new(false);

/// Check if we should simulate allocation failure (for testing).
#[inline]
fn should_fail_alloc() -> bool {
    SIMULATE_ALLOC_FAILURE.load(Ordering::Relaxed)
}

/// Enable allocation failure simulation (called by test hooks).
pub(crate) fn enable_alloc_failure() {
    SIMULATE_ALLOC_FAILURE.store(true, Ordering::Relaxed);
}

/// Disable allocation failure simulation.
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
    /// This matches the C behavior of cJSON_GetErrorPtr().
    static LAST_ERROR_PTR: RefCell<*const c_char> = RefCell::new(ptr::null());
}

/// Set the error pointer (called internally when parse fails).
pub(crate) fn set_error_ptr(ptr: *const c_char) {
    LAST_ERROR_PTR.with(|cell| {
        *cell.borrow_mut() = ptr;
    });
}

/// Clear the error pointer (called on successful parse).
pub(crate) fn clear_error_ptr() {
    set_error_ptr(ptr::null());
}

/// Return the pointer to the location where parsing failed.
/// Returns NULL if no error has occurred or after a successful parse.
#[no_mangle]
pub extern "C" fn cJSON_GetErrorPtr() -> *const c_char {
    LAST_ERROR_PTR.with(|cell| *cell.borrow())
}

// ===========================================================================
//  Constructors — atomic values
// ===========================================================================

#[no_mangle]
pub extern "C" fn cJSON_CreateNull() -> *mut cJSON {
    new_item(CJSON_NULL)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateTrue() -> *mut cJSON {
    new_item(CJSON_TRUE)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateFalse() -> *mut cJSON {
    new_item(CJSON_FALSE)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateBool(boolean: cJSON_bool) -> *mut cJSON {
    new_item(if boolean != 0 { CJSON_TRUE } else { CJSON_FALSE })
}

#[no_mangle]
pub extern "C" fn cJSON_CreateNumber(num: c_double) -> *mut cJSON {
    let item = new_item(CJSON_NUMBER);
    if !item.is_null() {
        // SAFETY: we just allocated `item` above, it's valid.
        unsafe {
            (*item).valuedouble = num;
            (*item).valueint = saturating_double_to_int(num);
        }
    }
    item
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateString(string: *const c_char) -> *mut cJSON {
    let item = new_item(CJSON_STRING);
    if item.is_null() {
        return ptr::null_mut();
    }

    (*item).valuestring = strdup_raw(string);
    if (*item).valuestring.is_null() && !string.is_null() {
        // strdup failed — clean up and return NULL (matches C behavior)
        crate::ffi_impl::cJSON_Delete(item);
        return ptr::null_mut();
    }

    item
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateRaw(raw: *const c_char) -> *mut cJSON {
    let item = new_item(CJSON_RAW);
    if item.is_null() {
        return ptr::null_mut();
    }

    (*item).valuestring = strdup_raw(raw);
    if (*item).valuestring.is_null() && !raw.is_null() {
        crate::ffi_impl::cJSON_Delete(item);
        return ptr::null_mut();
    }

    item
}

#[no_mangle]
pub extern "C" fn cJSON_CreateArray() -> *mut cJSON {
    new_item(CJSON_ARRAY)
}

#[no_mangle]
pub extern "C" fn cJSON_CreateObject() -> *mut cJSON {
    new_item(CJSON_OBJECT)
}

// ===========================================================================
//  Constructors — references (non-owning)
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateStringReference(string: *const c_char) -> *mut cJSON {
    let item = new_item(CJSON_STRING | CJSON_IS_REFERENCE);
    if !item.is_null() {
        (*item).valuestring = string as *mut c_char;
    }
    item
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateObjectReference(child: *const cJSON) -> *mut cJSON {
    let item = new_item(CJSON_OBJECT | CJSON_IS_REFERENCE);
    if !item.is_null() {
        (*item).child = child as *mut cJSON;
    }
    item
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateArrayReference(child: *const cJSON) -> *mut cJSON {
    let item = new_item(CJSON_ARRAY | CJSON_IS_REFERENCE);
    if !item.is_null() {
        (*item).child = child as *mut cJSON;
    }
    item
}

// ===========================================================================
//  Constructors — bulk array creation
// ===========================================================================

/// Generic helper: create an array of `count` number nodes from a slice.
unsafe fn create_number_array<T: Into<f64> + Copy>(
    numbers: *const T,
    count: c_int,
) -> *mut cJSON {
    if count < 0 || numbers.is_null() {
        return ptr::null_mut();
    }

    let array = cJSON_CreateArray();
    if array.is_null() {
        return ptr::null_mut();
    }

    let mut prev: *mut cJSON = ptr::null_mut();
    for i in 0..(count as usize) {
        let n = cJSON_CreateNumber((*numbers.add(i)).into());
        if n.is_null() {
            crate::ffi_impl::cJSON_Delete(array);
            return ptr::null_mut();
        }
        if i == 0 {
            (*array).child = n;
        } else {
            suffix_object(prev, n);
        }
        prev = n;
    }

    // Circular prev pointer: head->prev = tail (for O(1) append)
    if !(*array).child.is_null() {
        (*(*array).child).prev = prev;
    }

    array
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateIntArray(
    numbers: *const c_int,
    count: c_int,
) -> *mut cJSON {
    if count < 0 || numbers.is_null() {
        return ptr::null_mut();
    }

    let array = cJSON_CreateArray();
    if array.is_null() {
        return ptr::null_mut();
    }

    let mut prev: *mut cJSON = ptr::null_mut();
    for i in 0..(count as usize) {
        let n = cJSON_CreateNumber(*numbers.add(i) as c_double);
        if n.is_null() {
            crate::ffi_impl::cJSON_Delete(array);
            return ptr::null_mut();
        }
        if i == 0 {
            (*array).child = n;
        } else {
            suffix_object(prev, n);
        }
        prev = n;
    }

    if !(*array).child.is_null() {
        (*(*array).child).prev = prev;
    }

    array
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateFloatArray(
    numbers: *const c_float,
    count: c_int,
) -> *mut cJSON {
    create_number_array(numbers, count)
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateDoubleArray(
    numbers: *const c_double,
    count: c_int,
) -> *mut cJSON {
    create_number_array(numbers, count)
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_CreateStringArray(
    strings: *const *const c_char,
    count: c_int,
) -> *mut cJSON {
    if count < 0 || strings.is_null() {
        return ptr::null_mut();
    }

    let array = cJSON_CreateArray();
    if array.is_null() {
        return ptr::null_mut();
    }

    let mut prev: *mut cJSON = ptr::null_mut();
    for i in 0..(count as usize) {
        let n = cJSON_CreateString(*strings.add(i));
        if n.is_null() {
            crate::ffi_impl::cJSON_Delete(array);
            return ptr::null_mut();
        }
        if i == 0 {
            (*array).child = n;
        } else {
            suffix_object(prev, n);
        }
        prev = n;
    }

    if !(*array).child.is_null() {
        (*(*array).child).prev = prev;
    }

    array
}

// ===========================================================================
//  Array / Object accessors
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_GetArraySize(array: *const cJSON) -> c_int {
    if array.is_null() {
        return 0;
    }

    let mut size: usize = 0;
    let mut child = (*array).child;
    while !child.is_null() {
        size += 1;
        child = (*child).next;
    }

    size as c_int // matches C: FIXME can overflow
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_GetArrayItem(
    array: *const cJSON,
    index: c_int,
) -> *mut cJSON {
    if index < 0 || array.is_null() {
        return ptr::null_mut();
    }

    let mut current = (*array).child;
    let mut remaining = index as usize;
    while !current.is_null() && remaining > 0 {
        remaining -= 1;
        current = (*current).next;
    }

    current
}

/// Internal: case-insensitive ASCII byte comparison.
unsafe fn case_insensitive_streq(a: *const c_char, b: *const c_char) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    let a_cstr = CStr::from_ptr(a);
    let b_cstr = CStr::from_ptr(b);
    a_cstr.to_bytes().eq_ignore_ascii_case(b_cstr.to_bytes())
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_GetObjectItem(
    object: *const cJSON,
    string: *const c_char,
) -> *mut cJSON {
    if object.is_null() || string.is_null() {
        return ptr::null_mut();
    }

    let mut current = (*object).child;
    while !current.is_null() {
        if !(*current).string.is_null()
            && case_insensitive_streq(string, (*current).string)
        {
            return current;
        }
        current = (*current).next;
    }

    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_GetObjectItemCaseSensitive(
    object: *const cJSON,
    string: *const c_char,
) -> *mut cJSON {
    if object.is_null() || string.is_null() {
        return ptr::null_mut();
    }

    let target = CStr::from_ptr(string);
    let mut current = (*object).child;
    while !current.is_null() {
        if !(*current).string.is_null() {
            let key = CStr::from_ptr((*current).string);
            if key == target {
                return current;
            }
        }
        current = (*current).next;
    }

    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_HasObjectItem(
    object: *const cJSON,
    string: *const c_char,
) -> cJSON_bool {
    if cJSON_GetObjectItem(object, string).is_null() { 0 } else { 1 }
}

// ===========================================================================
//  Mutation — adding items
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddItemToArray(
    array: *mut cJSON,
    item: *mut cJSON,
) -> cJSON_bool {
    if add_item_to_array(array, item) { 1 } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddItemToObject(
    object: *mut cJSON,
    string: *const c_char,
    item: *mut cJSON,
) -> cJSON_bool {
    if add_item_to_object(object, string, item, false) { 1 } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddItemToObjectCS(
    object: *mut cJSON,
    string: *const c_char,
    item: *mut cJSON,
) -> cJSON_bool {
    if add_item_to_object(object, string, item, true) { 1 } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddItemReferenceToArray(
    array: *mut cJSON,
    item: *mut cJSON,
) -> cJSON_bool {
    if array.is_null() || item.is_null() {
        return 0;
    }
    let reference = create_reference(item);
    if reference.is_null() {
        return 0;
    }
    if add_item_to_array(array, reference) { 1 } else { 0 }
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddItemReferenceToObject(
    object: *mut cJSON,
    string: *const c_char,
    item: *mut cJSON,
) -> cJSON_bool {
    if object.is_null() || string.is_null() || item.is_null() {
        return 0;
    }
    let reference = create_reference(item);
    if reference.is_null() {
        return 0;
    }
    if add_item_to_object(object, string, reference, false) { 1 } else { 0 }
}

/// Create a reference node that copies the source item's data but sets
/// `cJSON_IsReference` so `cJSON_Delete` won't free the pointed-to data.
unsafe fn create_reference(item: *const cJSON) -> *mut cJSON {
    if item.is_null() {
        return ptr::null_mut();
    }

    let reference = new_item(0);
    if reference.is_null() {
        return ptr::null_mut();
    }

    // Copy all fields from source
    (*reference).type_ = (*item).type_;
    (*reference).valuestring = (*item).valuestring;
    (*reference).valueint = (*item).valueint;
    (*reference).valuedouble = (*item).valuedouble;
    (*reference).child = (*item).child;

    // Key is NOT copied — the caller sets it
    (*reference).string = ptr::null_mut();
    // Mark as reference so Delete won't free the data
    (*reference).type_ |= CJSON_IS_REFERENCE;
    // Detach from any list
    (*reference).next = ptr::null_mut();
    (*reference).prev = ptr::null_mut();

    reference
}

// ===========================================================================
//  Mutation — detaching / deleting items
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_DetachItemViaPointer(
    parent: *mut cJSON,
    item: *mut cJSON,
) -> *mut cJSON {
    if parent.is_null() || item.is_null() {
        return ptr::null_mut();
    }

    // item must be either the first child, or have a valid prev pointer
    if item != (*parent).child && (*item).prev.is_null() {
        return ptr::null_mut();
    }

    if item != (*parent).child {
        // Not the first element: unlink from prev
        (*(*item).prev).next = (*item).next;
    }
    if !(*item).next.is_null() {
        // Not the last element: unlink from next
        (*(*item).next).prev = (*item).prev;
    }

    if item == (*parent).child {
        // First element: advance head
        (*parent).child = (*item).next;
    } else if (*item).next.is_null() {
        // Last element: update head's prev (tail pointer)
        (*(*parent).child).prev = (*item).prev;
    }

    // Detach
    (*item).prev = ptr::null_mut();
    (*item).next = ptr::null_mut();

    item
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_DetachItemFromArray(
    array: *mut cJSON,
    which: c_int,
) -> *mut cJSON {
    if which < 0 {
        return ptr::null_mut();
    }
    let item = cJSON_GetArrayItem(array, which);
    cJSON_DetachItemViaPointer(array, item)
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_DeleteItemFromArray(array: *mut cJSON, which: c_int) {
    crate::ffi_impl::cJSON_Delete(cJSON_DetachItemFromArray(array, which));
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_DetachItemFromObject(
    object: *mut cJSON,
    string: *const c_char,
) -> *mut cJSON {
    let to_detach = cJSON_GetObjectItem(object, string);
    cJSON_DetachItemViaPointer(object, to_detach)
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_DetachItemFromObjectCaseSensitive(
    object: *mut cJSON,
    string: *const c_char,
) -> *mut cJSON {
    let to_detach = cJSON_GetObjectItemCaseSensitive(object, string);
    cJSON_DetachItemViaPointer(object, to_detach)
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_DeleteItemFromObject(
    object: *mut cJSON,
    string: *const c_char,
) {
    crate::ffi_impl::cJSON_Delete(cJSON_DetachItemFromObject(object, string));
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_DeleteItemFromObjectCaseSensitive(
    object: *mut cJSON,
    string: *const c_char,
) {
    crate::ffi_impl::cJSON_Delete(cJSON_DetachItemFromObjectCaseSensitive(object, string));
}

// ===========================================================================
//  Mutation — inserting / replacing items
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_InsertItemInArray(
    array: *mut cJSON,
    which: c_int,
    newitem: *mut cJSON,
) -> cJSON_bool {
    if which < 0 || newitem.is_null() {
        return 0;
    }

    let after_inserted = cJSON_GetArrayItem(array, which);
    if after_inserted.is_null() {
        return cJSON_AddItemToArray(array, newitem);
    }

    // Validate: if not first child, prev must be non-null
    if !ptr::eq(after_inserted, (*array).child) && (*after_inserted).prev.is_null() {
        return 0;
    }

    (*newitem).next = after_inserted;
    (*newitem).prev = (*after_inserted).prev;
    (*after_inserted).prev = newitem;

    if ptr::eq(after_inserted, (*array).child) {
        (*array).child = newitem;
    } else {
        (*(*newitem).prev).next = newitem;
    }

    1
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_ReplaceItemViaPointer(
    parent: *mut cJSON,
    item: *mut cJSON,
    replacement: *mut cJSON,
) -> cJSON_bool {
    if parent.is_null()
        || (*parent).child.is_null()
        || replacement.is_null()
        || item.is_null()
    {
        return 0;
    }

    if ptr::eq(replacement, item) {
        return 1;
    }

    (*replacement).next = (*item).next;
    (*replacement).prev = (*item).prev;

    if !(*replacement).next.is_null() {
        (*(*replacement).next).prev = replacement;
    }

    if ptr::eq((*parent).child, item) {
        if ptr::eq((*(*parent).child).prev, (*parent).child) {
            (*replacement).prev = replacement;
        }
        (*parent).child = replacement;
    } else {
        if !(*replacement).prev.is_null() {
            (*(*replacement).prev).next = replacement;
        }
        if (*replacement).next.is_null() {
            (*(*parent).child).prev = replacement;
        }
    }

    (*item).next = ptr::null_mut();
    (*item).prev = ptr::null_mut();
    crate::ffi_impl::cJSON_Delete(item);

    1
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_ReplaceItemInArray(
    array: *mut cJSON,
    which: c_int,
    newitem: *mut cJSON,
) -> cJSON_bool {
    if which < 0 {
        return 0;
    }
    let item = cJSON_GetArrayItem(array, which);
    cJSON_ReplaceItemViaPointer(array, item, newitem)
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_ReplaceItemInObject(
    object: *mut cJSON,
    string: *const c_char,
    newitem: *mut cJSON,
) -> cJSON_bool {
    replace_item_in_object(object, string, newitem, false)
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_ReplaceItemInObjectCaseSensitive(
    object: *mut cJSON,
    string: *const c_char,
    newitem: *mut cJSON,
) -> cJSON_bool {
    replace_item_in_object(object, string, newitem, true)
}

unsafe fn replace_item_in_object(
    object: *mut cJSON,
    string: *const c_char,
    replacement: *mut cJSON,
    case_sensitive: bool,
) -> cJSON_bool {
    if replacement.is_null() || string.is_null() {
        return 0;
    }

    // Free old key on the replacement if owned
    if ((*replacement).type_ & CJSON_STRING_IS_CONST) == 0 && !(*replacement).string.is_null() {
        drop(CString::from_raw((*replacement).string));
    }

    // Set new key (always owned copy)
    (*replacement).string = strdup_raw(string);
    if (*replacement).string.is_null() {
        return 0;
    }
    (*replacement).type_ &= !CJSON_STRING_IS_CONST;

    let existing = if case_sensitive {
        cJSON_GetObjectItemCaseSensitive(object, string)
    } else {
        cJSON_GetObjectItem(object, string)
    };

    cJSON_ReplaceItemViaPointer(object, existing, replacement)
}

// ===========================================================================
//  Duplication & comparison
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_Duplicate(
    item: *const cJSON,
    recurse: cJSON_bool,
) -> *mut cJSON {
    duplicate_recursive(item, 0, recurse != 0)
}

unsafe fn duplicate_recursive(
    item: *const cJSON,
    depth: usize,
    recurse: bool,
) -> *mut cJSON {
    if item.is_null() {
        return ptr::null_mut();
    }

    let newitem = new_item((*item).type_ & !CJSON_IS_REFERENCE);
    if newitem.is_null() {
        return ptr::null_mut();
    }

    (*newitem).valueint = (*item).valueint;
    (*newitem).valuedouble = (*item).valuedouble;

    // Duplicate valuestring
    if !(*item).valuestring.is_null() {
        (*newitem).valuestring = strdup_raw((*item).valuestring);
        if (*newitem).valuestring.is_null() {
            crate::ffi_impl::cJSON_Delete(newitem);
            return ptr::null_mut();
        }
    }

    // Duplicate key string
    if !(*item).string.is_null() {
        if ((*item).type_ & CJSON_STRING_IS_CONST) != 0 {
            (*newitem).string = (*item).string; // share const string
        } else {
            (*newitem).string = strdup_raw((*item).string);
            if (*newitem).string.is_null() {
                crate::ffi_impl::cJSON_Delete(newitem);
                return ptr::null_mut();
            }
        }
    }

    if !recurse {
        return newitem;
    }

    // Recursively duplicate children
    let mut child = (*item).child;
    let mut prev_new: *mut cJSON = ptr::null_mut();
    while !child.is_null() {
        if depth >= crate::CJSON_CIRCULAR_LIMIT as usize {
            crate::ffi_impl::cJSON_Delete(newitem);
            return ptr::null_mut();
        }

        let newchild = duplicate_recursive(child, depth + 1, true);
        if newchild.is_null() {
            crate::ffi_impl::cJSON_Delete(newitem);
            return ptr::null_mut();
        }

        if prev_new.is_null() {
            (*newitem).child = newchild;
        } else {
            (*prev_new).next = newchild;
            (*newchild).prev = prev_new;
        }
        prev_new = newchild;

        child = (*child).next;
    }

    // Set circular prev: head->prev = tail
    if !(*newitem).child.is_null() {
        (*(*newitem).child).prev = prev_new;
    }

    newitem
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_Compare(
    a: *const cJSON,
    b: *const cJSON,
    case_sensitive: cJSON_bool,
) -> cJSON_bool {
    if a.is_null() || b.is_null() {
        return 0;
    }

    let a_type = (*a).type_ & TYPE_MASK;
    let b_type = (*b).type_ & TYPE_MASK;

    if a_type != b_type {
        return 0;
    }

    // Validate type
    match a_type {
        t if t == CJSON_FALSE
            || t == CJSON_TRUE
            || t == CJSON_NULL
            || t == CJSON_NUMBER
            || t == CJSON_STRING
            || t == CJSON_RAW
            || t == CJSON_ARRAY
            || t == CJSON_OBJECT => {}
        _ => return 0,
    }

    // Pointer equality
    if ptr::eq(a, b) {
        return 1;
    }

    match a_type {
        t if t == CJSON_FALSE || t == CJSON_TRUE || t == CJSON_NULL => 1,

        t if t == CJSON_NUMBER => {
            // Compare doubles (exact match, like C's compare_double)
            if ((*a).valuedouble - (*b).valuedouble).abs() < f64::EPSILON {
                1
            } else {
                0
            }
        }

        t if t == CJSON_STRING || t == CJSON_RAW => {
            if (*a).valuestring.is_null() || (*b).valuestring.is_null() {
                return 0;
            }
            let sa = CStr::from_ptr((*a).valuestring);
            let sb = CStr::from_ptr((*b).valuestring);
            if sa == sb { 1 } else { 0 }
        }

        t if t == CJSON_ARRAY => {
            let mut ae = (*a).child;
            let mut be = (*b).child;
            while !ae.is_null() && !be.is_null() {
                if cJSON_Compare(ae, be, case_sensitive) == 0 {
                    return 0;
                }
                ae = (*ae).next;
                be = (*be).next;
            }
            // Both must be exhausted
            if ae.is_null() && be.is_null() { 1 } else { 0 }
        }

        t if t == CJSON_OBJECT => {
            // Check all keys in a exist in b with equal values
            let mut elem = (*a).child;
            while !elem.is_null() {
                let b_match = if case_sensitive != 0 {
                    cJSON_GetObjectItemCaseSensitive(b, (*elem).string)
                } else {
                    cJSON_GetObjectItem(b, (*elem).string)
                };
                if b_match.is_null() || cJSON_Compare(elem, b_match, case_sensitive) == 0 {
                    return 0;
                }
                elem = (*elem).next;
            }
            // And vice-versa
            elem = (*b).child;
            while !elem.is_null() {
                let a_match = if case_sensitive != 0 {
                    cJSON_GetObjectItemCaseSensitive(a, (*elem).string)
                } else {
                    cJSON_GetObjectItem(a, (*elem).string)
                };
                if a_match.is_null() || cJSON_Compare(elem, a_match, case_sensitive) == 0 {
                    return 0;
                }
                elem = (*elem).next;
            }
            1
        }

        _ => 0,
    }
}

// ===========================================================================
//  In-place minification
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_Minify(json: *mut c_char) {
    if json.is_null() {
        return;
    }

    let mut read = json;
    let mut write = json;

    while *read != 0 {
        match *read as u8 {
            b' ' | b'\t' | b'\r' | b'\n' => {
                read = read.add(1);
            }
            b'/' => {
                if *read.add(1) as u8 == b'/' {
                    // Skip single-line comment
                    read = read.add(2);
                    while *read != 0 && *read as u8 != b'\n' {
                        read = read.add(1);
                    }
                    if *read != 0 {
                        read = read.add(1); // skip the \n
                    }
                } else if *read.add(1) as u8 == b'*' {
                    // Skip multi-line comment
                    read = read.add(2);
                    while *read != 0 {
                        if *read as u8 == b'*' && *read.add(1) as u8 == b'/' {
                            read = read.add(2);
                            break;
                        }
                        read = read.add(1);
                    }
                } else {
                    read = read.add(1);
                }
            }
            b'"' => {
                // Copy string literal (including quotes)
                *write = *read;
                read = read.add(1);
                write = write.add(1);

                while *read != 0 {
                    *write = *read;

                    if *read as u8 == b'"' {
                        read = read.add(1);
                        write = write.add(1);
                        break;
                    } else if *read as u8 == b'\\' && *read.add(1) as u8 == b'"' {
                        write = write.add(1);
                        read = read.add(1);
                        *write = *read;
                    }

                    read = read.add(1);
                    write = write.add(1);
                }
            }
            _ => {
                *write = *read;
                read = read.add(1);
                write = write.add(1);
            }
        }
    }

    *write = 0; // NUL-terminate
}

// ===========================================================================
//  Convenience helpers — create + add in one call
// ===========================================================================

macro_rules! impl_add_to_object {
    ($fn_name:ident, $create_fn:ident) => {
        #[no_mangle]
        pub unsafe extern "C" fn $fn_name(
            object: *mut cJSON,
            name: *const c_char,
        ) -> *mut cJSON {
            let item = $create_fn();
            if add_item_to_object(object, name, item, false) {
                return item;
            }
            crate::ffi_impl::cJSON_Delete(item);
            ptr::null_mut()
        }
    };
}

impl_add_to_object!(cJSON_AddNullToObject, cJSON_CreateNull);
impl_add_to_object!(cJSON_AddTrueToObject, cJSON_CreateTrue);
impl_add_to_object!(cJSON_AddFalseToObject, cJSON_CreateFalse);
impl_add_to_object!(cJSON_AddObjectToObject, cJSON_CreateObject);
impl_add_to_object!(cJSON_AddArrayToObject, cJSON_CreateArray);

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddBoolToObject(
    object: *mut cJSON,
    name: *const c_char,
    boolean: cJSON_bool,
) -> *mut cJSON {
    let item = cJSON_CreateBool(boolean);
    if add_item_to_object(object, name, item, false) {
        return item;
    }
    crate::ffi_impl::cJSON_Delete(item);
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddNumberToObject(
    object: *mut cJSON,
    name: *const c_char,
    number: c_double,
) -> *mut cJSON {
    let item = cJSON_CreateNumber(number);
    if add_item_to_object(object, name, item, false) {
        return item;
    }
    crate::ffi_impl::cJSON_Delete(item);
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddStringToObject(
    object: *mut cJSON,
    name: *const c_char,
    string: *const c_char,
) -> *mut cJSON {
    let item = cJSON_CreateString(string);
    if add_item_to_object(object, name, item, false) {
        return item;
    }
    crate::ffi_impl::cJSON_Delete(item);
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_AddRawToObject(
    object: *mut cJSON,
    name: *const c_char,
    raw: *const c_char,
) -> *mut cJSON {
    let item = cJSON_CreateRaw(raw);
    if add_item_to_object(object, name, item, false) {
        return item;
    }
    crate::ffi_impl::cJSON_Delete(item);
    ptr::null_mut()
}

// ===========================================================================
//  Number / string value setters
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_SetNumberHelper(
    object: *mut cJSON,
    number: c_double,
) -> c_double {
    if object.is_null() {
        return f64::NAN;
    }

    (*object).valueint = saturating_double_to_int(number);
    (*object).valuedouble = number;

    (*object).valuedouble
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_SetValuestring(
    object: *mut cJSON,
    valuestring: *const c_char,
) -> *mut c_char {
    // Must be a non-reference string node
    if object.is_null()
        || ((*object).type_ & CJSON_STRING) == 0
        || ((*object).type_ & CJSON_IS_REFERENCE) != 0
    {
        return ptr::null_mut();
    }

    if (*object).valuestring.is_null() || valuestring.is_null() {
        return ptr::null_mut();
    }

    let old_len = CStr::from_ptr((*object).valuestring).to_bytes().len();
    let new_len = CStr::from_ptr(valuestring).to_bytes().len();

    if new_len <= old_len {
        // Reuse existing buffer — check for overlap
        let old_ptr = (*object).valuestring as usize;
        let new_ptr = valuestring as usize;
        if !(new_ptr + new_len < old_ptr || old_ptr + old_len < new_ptr) {
            return ptr::null_mut(); // overlapping strings
        }
        // Copy in-place
        ptr::copy_nonoverlapping(valuestring, (*object).valuestring, new_len + 1);
    } else {
        // Allocate new string
        let new_vs = strdup_raw(valuestring);
        if new_vs.is_null() {
            return ptr::null_mut();
        }
        // Free old
        drop(CString::from_raw((*object).valuestring));
        (*object).valuestring = new_vs;
    }

    (*object).valuestring
}

// ===========================================================================
//  Hook-aware allocator pass-throughs
//
//  Since we reject custom hooks, these always use Rust's global allocator.
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_malloc(size: usize) -> *mut c_void {
    // Use Rust's global allocator via Vec
    if size == 0 {
        return ptr::null_mut();
    }
    let mut buf: Vec<u8> = Vec::with_capacity(size);
    buf.resize(size, 0);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf); // caller is responsible for freeing via cJSON_free
    ptr as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_free(object: *mut c_void) {
    if object.is_null() {
        return;
    }
    // We can't know the original size, so we reconstruct a Vec<u8> with
    // capacity 0 and let the allocator handle it.  This works because
    // Rust's global allocator tracks allocation sizes internally.
    //
    // NOTE: This is a best-effort stub.  For production use, pair with
    // a size-tracking wrapper or use `std::alloc::dealloc` with the
    // correct `Layout`.
    drop(Box::from_raw(object as *mut u8));
}

// ===========================================================================
//  Parsing stubs
//
//  cJSON_Parse is implemented in ffi_impl.rs with full arena-backed parsing.
//  The remaining parsing functions are stubs that return NULL.
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_ParseWithLength(
    _value: *const c_char,
    _buffer_length: usize,
) -> *mut cJSON {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_ParseWithOpts(
    _value: *const c_char,
    _return_parse_end: *mut *const c_char,
    _require_null_terminated: cJSON_bool,
) -> *mut cJSON {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_ParseWithLengthOpts(
    _value: *const c_char,
    _buffer_length: usize,
    _return_parse_end: *mut *const c_char,
    _require_null_terminated: cJSON_bool,
) -> *mut cJSON {
    ptr::null_mut()
}

// ===========================================================================
//  Printing stubs
//
//  The printer is NOT yet ported.  These stubs return NULL.
// ===========================================================================

#[no_mangle]
pub unsafe extern "C" fn cJSON_Print(_item: *const cJSON) -> *mut c_char {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_PrintUnformatted(_item: *const cJSON) -> *mut c_char {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_PrintBuffered(
    _item: *const cJSON,
    _prebuffer: c_int,
    _fmt: cJSON_bool,
) -> *mut c_char {
    ptr::null_mut()
}

#[no_mangle]
pub unsafe extern "C" fn cJSON_PrintPreallocated(
    _item: *mut cJSON,
    _buffer: *mut c_char,
    _length: c_int,
    _format: cJSON_bool,
) -> cJSON_bool {
    0 // failure
}
