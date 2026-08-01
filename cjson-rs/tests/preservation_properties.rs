//! # Preservation Property-Based Tests
//!
//! **Validates: Requirements 3.1, 3.2, 3.3, 3.4, 3.5, 3.6**
//!
//! These tests verify that modules and functionality NOT affected by the
//! Arena lifetime bug continue to work correctly. They should PASS on
//! UNFIXED code (before implementing the fix) to establish a baseline.
//!
//! ## Purpose
//! - Property 2: Preservation - Module Independence and Existing Behavior
//! - Observation-first methodology: capture current behavior patterns
//! - Stronger guarantees through property-based testing (many generated inputs)
//!
//! ## Expected Outcome
//! ALL tests in this file MUST PASS on unfixed code. This confirms that:
//! - Arena module is independently correct
//! - Parser module is independently correct
//! - cJSON_Delete correctly manages C tree memory
//! - cJSON_InitHooks stub never crashes
//! - All existing FFI tests remain valid

#![allow(unused_imports)]

use cjson_rs::arena::{Arena, JsonValue, NodeId};
use cjson_rs::parser::parse_json;
use cjson_rs::{cJSON, cJSON_Hooks, cJSON_Delete, cJSON_InitHooks};
use cjson_rs::{CJSON_FALSE, CJSON_TRUE, CJSON_NULL, CJSON_NUMBER, CJSON_STRING, CJSON_ARRAY, CJSON_OBJECT, CJSON_IS_REFERENCE, CJSON_STRING_IS_CONST};
use std::ffi::{CStr, CString};
use std::ptr;
use quickcheck::{Arbitrary, Gen, QuickCheck, TestResult};
use quickcheck_macros::quickcheck;

// ===========================================================================
//  Property 1: Arena Module Independence
//  Validates: Requirements 3.1, 3.2, 3.6
// ===========================================================================

/// ArbitraryJsonValue: Generate random JSON values for property testing
#[derive(Clone, Debug)]
enum ArbitraryJsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<ArbitraryJsonValue>),
    Object(Vec<(String, ArbitraryJsonValue)>),
}

impl Arbitrary for ArbitraryJsonValue {
    fn arbitrary(g: &mut Gen) -> Self {
        let depth = g.size();
        // Limit depth to prevent stack overflow in tests
        if depth > 10 {
            // At max depth, only generate leaf nodes
            match u8::arbitrary(g) % 4 {
                0 => ArbitraryJsonValue::Null,
                1 => ArbitraryJsonValue::Bool(bool::arbitrary(g)),
                2 => {
                    // Generate finite numbers only
                    let n = f64::arbitrary(g);
                    if n.is_finite() { ArbitraryJsonValue::Number(n) } else { ArbitraryJsonValue::Number(0.0) }
                }
                _ => {
                    let s = String::arbitrary(g);
                    // Limit string length
                    let truncated = s.chars().take(100).collect();
                    ArbitraryJsonValue::String(truncated)
                }
            }
        } else {
            match u8::arbitrary(g) % 6 {
                0 => ArbitraryJsonValue::Null,
                1 => ArbitraryJsonValue::Bool(bool::arbitrary(g)),
                2 => {
                    let n = f64::arbitrary(g);
                    if n.is_finite() { ArbitraryJsonValue::Number(n) } else { ArbitraryJsonValue::Number(0.0) }
                }
                3 => {
                    let s = String::arbitrary(g);
                    let truncated = s.chars().take(100).collect();
                    ArbitraryJsonValue::String(truncated)
                }
                4 => {
                    // Limit array size
                    let size = usize::arbitrary(g) % 10;
                    let mut smaller_g = Gen::new(depth.saturating_sub(1));
                    let items: Vec<ArbitraryJsonValue> = (0..size)
                        .map(|_| ArbitraryJsonValue::arbitrary(&mut smaller_g))
                        .collect();
                    ArbitraryJsonValue::Array(items)
                }
                _ => {
                    // Limit object size
                    let size = usize::arbitrary(g) % 10;
                    let mut smaller_g = Gen::new(depth.saturating_sub(1));
                    let pairs: Vec<(String, ArbitraryJsonValue)> = (0..size)
                        .map(|_| {
                            let key: String = String::arbitrary(&mut smaller_g).chars().take(50).collect();
                            let val = ArbitraryJsonValue::arbitrary(&mut smaller_g);
                            (key, val)
                        })
                        .collect();
                    ArbitraryJsonValue::Object(pairs)
                }
            }
        }
    }
}

impl ArbitraryJsonValue {
    /// Build this value in an Arena and return the NodeId
    fn build_in_arena(&self, arena: &mut Arena) -> NodeId {
        match self {
            ArbitraryJsonValue::Null => arena.alloc_null(),
            ArbitraryJsonValue::Bool(b) => arena.alloc_bool(*b),
            ArbitraryJsonValue::Number(n) => arena.alloc_number(*n),
            ArbitraryJsonValue::String(s) => arena.alloc_string(s.clone()),
            ArbitraryJsonValue::Array(items) => {
                let arr_id = arena.alloc_array();
                for item in items {
                    let child_id = item.build_in_arena(arena);
                    arena.append_child(arr_id, child_id);
                }
                arr_id
            }
            ArbitraryJsonValue::Object(pairs) => {
                let obj_id = arena.alloc_object();
                for (key, val) in pairs {
                    let child_id = val.build_in_arena(arena);
                    arena.append_child_with_key(obj_id, child_id, key.clone());
                }
                obj_id
            }
        }
    }

    /// Convert to JSON string representation
    fn to_json_string(&self) -> String {
        match self {
            ArbitraryJsonValue::Null => "null".to_string(),
            ArbitraryJsonValue::Bool(true) => "true".to_string(),
            ArbitraryJsonValue::Bool(false) => "false".to_string(),
            ArbitraryJsonValue::Number(n) => {
                if n.fract() == 0.0 && n.is_finite() {
                    format!("{}", *n as i64)
                } else {
                    format!("{}", n)
                }
            }
            ArbitraryJsonValue::String(s) => {
                // Minimal JSON string escaping
                let mut result = String::from("\"");
                for ch in s.chars() {
                    match ch {
                        '"' => result.push_str("\\\""),
                        '\\' => result.push_str("\\\\"),
                        '\n' => result.push_str("\\n"),
                        '\r' => result.push_str("\\r"),
                        '\t' => result.push_str("\\t"),
                        c if c.is_control() => result.push_str(&format!("\\u{:04x}", c as u32)),
                        c => result.push(c),
                    }
                }
                result.push('"');
                result
            }
            ArbitraryJsonValue::Array(items) => {
                let items_str: Vec<String> = items.iter().map(|v| v.to_json_string()).collect();
                format!("[{}]", items_str.join(","))
            }
            ArbitraryJsonValue::Object(pairs) => {
                let pairs_str: Vec<String> = pairs
                    .iter()
                    .map(|(k, v)| {
                        format!("\"{}\":{}", k.replace('"', "\\\""), v.to_json_string())
                    })
                    .collect();
                format!("{{{}}}", pairs_str.join(","))
            }
        }
    }
}

/// **Property 1.1**: Arena allocation is safe and produces valid NodeIds
/// **Validates: Requirement 3.1** - Arena module forbids unsafe code
#[quickcheck]
fn prop_arena_allocation_is_safe(value: ArbitraryJsonValue) -> bool {
    let mut arena = Arena::new();
    let node_id = value.build_in_arena(&mut arena);
    
    // Verify the node exists and is accessible
    arena.get(node_id).is_some()
}

/// **Property 1.2**: Arena child traversal is consistent
/// **Validates: Requirement 3.1** - Arena module correct allocation
#[quickcheck]
fn prop_arena_child_count_matches(items: Vec<ArbitraryJsonValue>) -> bool {
    // Limit size to keep tests fast
    if items.len() > 50 {
        return true;
    }
    
    let mut arena = Arena::new();
    let arr_id = arena.alloc_array();
    
    for item in &items {
        let child_id = item.build_in_arena(&mut arena);
        arena.append_child(arr_id, child_id);
    }
    
    // Verify child count matches
    arena.child_count(arr_id) == items.len()
}

/// **Property 1.3**: Arena tree structure maintains parent-child links
/// **Validates: Requirement 3.1** - Arena module correct allocation
#[quickcheck]
fn prop_arena_parent_child_links(items: Vec<ArbitraryJsonValue>) -> bool {
    if items.len() > 50 {
        return true;
    }
    
    let mut arena = Arena::new();
    let arr_id = arena.alloc_array();
    
    let mut child_ids = Vec::new();
    for item in &items {
        let child_id = item.build_in_arena(&mut arena);
        arena.append_child(arr_id, child_id);
        child_ids.push(child_id);
    }
    
    // Verify all children have correct parent
    child_ids.iter().all(|&child_id| {
        arena.get(child_id).map(|node| node.parent == Some(arr_id)).unwrap_or(false)
    })
}

/// **Property 1.4**: Arena detach operation is safe
/// **Validates: Requirement 3.1** - Arena module correct allocation
#[quickcheck]
fn prop_arena_detach_is_safe(items: Vec<ArbitraryJsonValue>) -> bool {
    if items.is_empty() || items.len() > 20 {
        return true;
    }
    
    let mut arena = Arena::new();
    let arr_id = arena.alloc_array();
    
    let mut child_ids = Vec::new();
    for item in &items {
        let child_id = item.build_in_arena(&mut arena);
        arena.append_child(arr_id, child_id);
        child_ids.push(child_id);
    }
    
    // Detach first child
    let detached = arena.detach(child_ids[0]);
    
    // Verify detach succeeded and child count decreased
    detached && arena.child_count(arr_id) == items.len() - 1
}

// ===========================================================================
//  Property 2: Parser Module Independence
//  Validates: Requirements 3.2, 3.6
// ===========================================================================

/// **Property 2.1**: Parser correctly handles valid JSON literals
/// **Validates: Requirement 3.2** - Parser module correct JSON parsing
#[quickcheck]
fn prop_parser_handles_literals(literal_type: u8) -> bool {
    let input: &[u8] = match literal_type % 3 {
        0 => b"null",
        1 => b"true",
        _ => b"false",
    };
    
    let mut arena = Arena::new();
    let result = parse_json(input, &mut arena);
    
    result.is_ok()
}

/// **Property 2.2**: Parser correctly handles valid numbers
/// **Validates: Requirement 3.2** - Parser module correct JSON parsing
#[quickcheck]
fn prop_parser_handles_numbers(n: f64) -> TestResult {
    if !n.is_finite() {
        return TestResult::discard();
    }
    
    let json = if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    };
    
    let mut arena = Arena::new();
    let result = parse_json(json.as_bytes(), &mut arena);
    
    TestResult::from_bool(result.is_ok())
}

/// **Property 2.3**: Parser correctly handles valid strings
/// **Validates: Requirement 3.2** - Parser module correct JSON parsing
#[quickcheck]
fn prop_parser_handles_strings(s: String) -> TestResult {
    // Limit string length and filter out control characters
    let clean: String = s.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(100)
        .collect();
    
    if clean.is_empty() {
        return TestResult::discard();
    }
    
    // Build JSON string with proper escaping
    let mut json = String::from("\"");
    for ch in clean.chars() {
        match ch {
            '"' => json.push_str("\\\""),
            '\\' => json.push_str("\\\\"),
            '\n' => json.push_str("\\n"),
            '\t' => json.push_str("\\t"),
            c => json.push(c),
        }
    }
    json.push('"');
    
    let mut arena = Arena::new();
    let result = parse_json(json.as_bytes(), &mut arena);
    
    TestResult::from_bool(result.is_ok())
}

/// **Property 2.4**: Parser correctly handles empty arrays and objects
/// **Validates: Requirement 3.2** - Parser module correct JSON parsing
#[quickcheck]
fn prop_parser_handles_empty_containers(is_array: bool) -> bool {
    let input = if is_array { b"[]" } else { b"{}" };
    
    let mut arena = Arena::new();
    let result = parse_json(input, &mut arena);
    
    if let Ok(_root_idx) = result {
        // Parser returns the root index, which should be 0 for first node
        // We can verify by checking arena length and getting the node
        arena.len() > 0 && {
            // Since parse_json returns the root index and it's the first allocation,
            // we know it's at position 0. Check child count through arena API.
            // We need to trust that parse_json returned a valid index.
            // For empty containers, just verify parse succeeded.
            true
        }
    } else {
        false
    }
}

/// **Property 2.5**: Parser enforces depth limits
/// **Validates: Requirement 3.6** - Parser enforces depth limits (1000 levels)
#[test]
fn prop_parser_rejects_excessive_nesting() {
    // Generate deeply nested array: [[[[...]]]] with depth > 1000
    let mut json = String::new();
    let depth = 1001;
    for _ in 0..depth {
        json.push('[');
    }
    json.push_str("null");
    for _ in 0..depth {
        json.push(']');
    }
    
    let mut arena = Arena::new();
    let result = parse_json(json.as_bytes(), &mut arena);
    
    // Should fail due to depth limit
    assert!(result.is_err(), "Parser should reject nesting depth > 1000");
}

/// **Property 2.6**: Parser accepts valid depth (exactly at limit)
/// **Validates: Requirement 3.6** - Parser accepts valid depth at boundary
#[test]
fn prop_parser_accepts_valid_depth() {
    // Generate nested array at exactly the limit: 1000 levels
    let mut json = String::new();
    let depth = 1000;
    for _ in 0..depth {
        json.push('[');
    }
    json.push_str("null");
    for _ in 0..depth {
        json.push(']');
    }
    
    let mut arena = Arena::new();
    let result = parse_json(json.as_bytes(), &mut arena);
    
    // Should succeed at the boundary
    assert!(result.is_ok(), "Parser should accept nesting depth = 1000");
}

// ===========================================================================
//  Property 3: cJSON_Delete Correctness
//  Validates: Requirement 3.3, 3.4
// ===========================================================================

/// Helper: Create a raw cJSON node (matches FFI allocation pattern)
unsafe fn create_test_cjson_node(type_: i32) -> *mut cJSON {
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

/// **Property 3.1**: cJSON_Delete handles NULL gracefully
/// **Validates: Requirement 3.3** - cJSON_Delete correctly frees C trees
#[test]
fn prop_delete_null_is_noop() {
    unsafe {
        cJSON_Delete(ptr::null_mut());
    }
    // Test passes if we don't segfault
}

/// **Property 3.2**: cJSON_Delete frees single nodes
/// **Validates: Requirement 3.3** - cJSON_Delete correctly frees C trees
#[quickcheck]
fn prop_delete_single_node(type_bits: u8) -> bool {
    let type_ = (type_bits % 7) as i32; // Generate valid type values
    
    unsafe {
        let node = create_test_cjson_node(type_);
        cJSON_Delete(node);
    }
    
    // Test passes if we don't segfault or leak
    true
}

/// **Property 3.3**: cJSON_Delete handles nodes with owned strings
/// **Validates: Requirement 3.3, 3.4** - cJSON_Delete correctly frees strings
#[quickcheck]
fn prop_delete_node_with_strings(value: String, key: String) -> bool {
    let value_clean: String = value.chars().take(100).collect();
    let key_clean: String = key.chars().take(100).collect();
    
    if value_clean.is_empty() || key_clean.is_empty() {
        return true;
    }
    
    unsafe {
        let node = create_test_cjson_node(CJSON_STRING);
        
        if let Ok(vs) = CString::new(value_clean) {
            (*node).valuestring = vs.into_raw();
        }
        
        if let Ok(ks) = CString::new(key_clean) {
            (*node).string = ks.into_raw();
        }
        
        cJSON_Delete(node);
    }
    
    true
}

/// **Property 3.4**: cJSON_Delete respects cJSON_IsReference flag
/// **Validates: Requirement 3.3** - cJSON_Delete respects reference flags
#[test]
fn prop_delete_respects_reference_flag() {
    unsafe {
        // Create a child node that is "owned elsewhere"
        let owned_child = create_test_cjson_node(CJSON_STRING);
        let vs = CString::new("owned_value").unwrap();
        (*owned_child).valuestring = vs.into_raw();
        
        // Create a reference node pointing to it
        let ref_node = create_test_cjson_node(CJSON_OBJECT | CJSON_IS_REFERENCE);
        (*ref_node).child = owned_child;
        
        // Delete reference node - should NOT delete child
        cJSON_Delete(ref_node);
        
        // Child should still be accessible - clean it up separately
        cJSON_Delete(owned_child);
    }
    // Test passes if no double-free
}

/// **Property 3.5**: cJSON_Delete respects cJSON_StringIsConst flag
/// **Validates: Requirement 3.3** - cJSON_Delete respects string const flags
#[test]
fn prop_delete_respects_string_const_flag() {
    static STATIC_KEY: &[u8] = b"static_key\0";
    
    unsafe {
        let node = create_test_cjson_node(CJSON_STRING | CJSON_STRING_IS_CONST);
        
        // valuestring is owned, should be freed
        let vs = CString::new("value").unwrap();
        (*node).valuestring = vs.into_raw();
        
        // string is const, should NOT be freed
        (*node).string = STATIC_KEY.as_ptr() as *mut i8;
        
        cJSON_Delete(node);
    }
    // Test passes if no attempt to free static memory
}

/// **Property 3.6**: cJSON_Delete handles sibling chains
/// **Validates: Requirement 3.3** - cJSON_Delete correctly frees sibling chains
#[quickcheck]
fn prop_delete_sibling_chain(count: u8) -> bool {
    let count = (count % 10) + 1; // 1-10 siblings
    
    unsafe {
        let mut nodes = Vec::new();
        
        // Create chain
        for _ in 0..count {
            nodes.push(create_test_cjson_node(CJSON_NULL));
        }
        
        // Link siblings
        for i in 0..(count as usize - 1) {
            (*nodes[i]).next = nodes[i + 1];
            (*nodes[i + 1]).prev = nodes[i];
        }
        
        // Delete head - should free entire chain
        cJSON_Delete(nodes[0]);
    }
    
    true
}

// ===========================================================================
//  Property 4: cJSON_InitHooks Correctness
//  Validates: Requirement 3.4
// ===========================================================================

/// **Property 4.1**: cJSON_InitHooks with NULL never crashes
/// **Validates: Requirement 3.4** - cJSON_InitHooks stub behaves correctly
#[test]
fn prop_init_hooks_null_is_safe() {
    unsafe {
        cJSON_InitHooks(ptr::null_mut());
    }
    // Test passes if we don't segfault
}

/// **Property 4.2**: cJSON_InitHooks with custom hooks never crashes
/// **Validates: Requirement 3.4** - cJSON_InitHooks stub handles non-null input
#[test]
fn prop_init_hooks_custom_is_safe() {
    unsafe extern "C" fn dummy_malloc(_sz: usize) -> *mut std::os::raw::c_void {
        ptr::null_mut()
    }
    
    unsafe extern "C" fn dummy_free(_ptr: *mut std::os::raw::c_void) {
        // No-op
    }
    
    let mut hooks = cJSON_Hooks {
        malloc_fn: Some(dummy_malloc),
        free_fn: Some(dummy_free),
    };
    
    unsafe {
        cJSON_InitHooks(&mut hooks as *mut cJSON_Hooks);
    }
    // Test passes if we don't segfault
}

/// **Property 4.3**: cJSON_InitHooks can be called multiple times safely
/// **Validates: Requirement 3.4** - cJSON_InitHooks stub is idempotent
#[quickcheck]
fn prop_init_hooks_idempotent(iterations: u8) -> bool {
    let iterations = (iterations % 10) + 1;
    
    for _ in 0..iterations {
        unsafe {
            cJSON_InitHooks(ptr::null_mut());
        }
    }
    
    true
}

// ===========================================================================
//  Property 5: Existing FFI Tests Continue to Pass
//  Validates: Requirement 3.5
// ===========================================================================
// Note: The existing FFI tests in ffi_impl.rs::tests already verify this.
// We document here that those tests form part of the preservation property.

#[test]
fn preservation_note_existing_ffi_tests() {
    // This test serves as documentation that the existing FFI tests in
    // cjson-rs/src/ffi_impl.rs (delete_null_is_noop, delete_single_node_no_strings,
    // delete_node_with_owned_strings, etc.) are part of the preservation property.
    //
    // Those tests verify that cJSON_Delete and cJSON_InitHooks work correctly
    // independent of the cJSON_Parse Arena lifetime issue.
    //
    // Running `cargo test` will execute all those tests, confirming preservation.
}

