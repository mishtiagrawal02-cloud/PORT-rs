# Borrow-Checker Integration Fix Design

## Overview

The `cJSON_Parse` function in `ffi_impl.rs` creates an Arena allocator on the stack, parses JSON into arena-backed nodes, then materializes those nodes into C-compatible structs. However, the Arena is dropped at the end of the function scope while the materialization function still holds an immutable borrow of the Arena, causing a borrow checker error. The fix relocates the Arena to the heap via `Box`, extends its lifetime beyond the materialization phase, and ensures the Arena is properly cleaned up after all data has been copied into independent C allocations.

The fix strategy is **Arena Heap Allocation**: move the Arena from stack to heap using `Box::new`, pass ownership through materialization, and drop the Arena only after materialization completes and all data is copied.

## Glossary

- **Bug_Condition (C)**: The condition that triggers the bug - Arena created on stack with lifetime ending before materialization completes
- **Property (P)**: The desired behavior when Arena is heap-allocated - Arena outlives materialization, borrow checker accepts the code
- **Preservation**: Existing parser behavior, deletion logic, and FFI safety guarantees that must remain unchanged by the fix
- **Arena**: The safe, index-based JSON AST allocator defined in `arena.rs` with `#![forbid(unsafe_code)]`
- **materialization**: The process of converting Arena index-based nodes into C-compatible `*mut cJSON` pointer-based linked lists
- **lifetime 'arena**: The Rust compiler's lifetime annotation tracking how long Arena references remain valid
- **borrow**: An immutable reference (`&Arena`) that prevents mutation while allowing shared read access
- **stack allocation**: Memory allocated in the function's stack frame, automatically dropped when the function returns
- **heap allocation**: Memory allocated via `Box::new` that persists beyond function scope until explicitly dropped

## Bug Details

### Bug Condition

The bug manifests when the Arena allocator is created on the stack within `cJSON_Parse`, borrowed immutably by `materialize_arena_node`, but the function attempts to return a pointer while the Arena is still borrowed. The Rust compiler detects that the Arena's lifetime ends when the function returns, but the borrow must remain valid throughout materialization, creating an impossible lifetime constraint.

**Formal Specification:**
```
FUNCTION isBugCondition(code_structure)
  INPUT: code_structure of type FunctionImplementation
  OUTPUT: boolean
  
  arena_on_stack := code_structure.arena IS Stack_Allocated
  
  arena_borrowed := EXISTS call TO materialize_arena_node(&arena, node_id)
                    WHERE call BORROWS arena immutably
  
  arena_dropped_early := arena.lifetime ENDS BEFORE materialize_returns
  
  borrow_checker_error := compiler DETECTS (arena_borrowed AND arena_dropped_early)
  
  RETURN arena_on_stack AND arena_borrowed AND arena_dropped_early AND borrow_checker_error
END FUNCTION
```

### Examples

**Example 1: Current buggy code (compilation fails)**
```rust
pub unsafe extern "C" fn cJSON_Parse(value: *const c_char) -> *mut cJSON {
    let mut arena = Arena::new();  // Stack allocation
    let root_index = parse_json(input, &mut arena)?;
    let root_id = NodeId::from_raw(root_index);
    
    // Bug: arena is borrowed here but will be dropped when function returns
    materialize_arena_node(&arena, root_id)  // Borrow extends to here
    // Arena dropped here - compiler error!
}
```

**Compiler error:**
```
error[E0597]: `arena` does not live long enough
  --> ffi_impl.rs:XX:YY
   |
XX |     let mut arena = Arena::new();
   |         --------- binding `arena` declared here
...
XX |     materialize_arena_node(&arena, root_id)
   |                            ^^^^^^ borrowed value does not live long enough
XX | }
   | - `arena` dropped here while still borrowed
```

**Example 2: Attempting to extend lifetime with explicit annotation (still fails)**
```rust
// This doesn't work because the lifetime parameter can't change the fact
// that arena is stack-allocated and will be dropped
fn materialize_arena_node<'a>(arena: &'a Arena, node_id: NodeId) -> *mut cJSON {
    // ... materialization code ...
}
```

**Example 3: Valid case - Arena used without FFI (no bug)**
```rust
// This works fine - no lifetime conflict
let mut arena = Arena::new();
let root = parse_json(input, &mut arena)?;
let json_string = arena.to_json_string(NodeId::from_raw(root));
// Arena dropped here, but no references escape
```

**Example 4: Edge case - Empty JSON document**
```rust
// Input: ""
// Current behavior: parse_json returns Err, function returns null
// Expected after fix: Same behavior (no Arena materialization occurs)
```

## Expected Behavior

### Preservation Requirements

**Unchanged Behaviors:**
- The `Arena` module must continue to forbid unsafe code via `#![forbid(unsafe_code)]`
- The `parser` module must continue to forbid unsafe code via `#![forbid(unsafe_code)]`
- The `parse_json` function must continue to return `Result<u32, ParseError>` with correct node indices
- The `materialize_arena_node` function must continue to create C-compatible structs via `Box::into_raw` and `CString::into_raw`
- The `cJSON_Delete` function must continue to correctly deallocate all materialized C structs without leaks or double-frees
- All existing FFI tests must continue to pass without modification
- Parse errors must continue to return `ptr::null_mut()` to C callers
- The depth limit enforcement (1000 levels) must remain active in the parser

**Scope:**
All parsing logic, tree materialization logic, and deletion logic that does NOT involve Arena lifetime management should be completely unaffected by this fix. This includes:
- Parser correctness (IEEE 754 number parsing, string escaping, depth checks)
- Materialization correctness (type flag assignment, sibling chain linking)
- Deletion correctness (reference flag handling, string ownership tracking)

## Hypothesized Root Cause

Based on the bug description and code analysis, the most likely issues are:

1. **Stack Allocation of Arena**: The Arena is created as a local variable on the stack with `let mut arena = Arena::new()`, giving it a lifetime tied to the function scope. When the function returns, the Arena is automatically dropped, but the borrow checker requires the Arena to outlive any borrows taken during materialization.

2. **Immutable Borrow Conflict**: The `materialize_arena_node(&arena, root_id)` call creates an immutable borrow of the Arena. The borrow checker conservatively assumes this borrow must remain valid for the lifetime of the returned pointer, creating a conflict with the Arena's stack lifetime.

3. **Lack of Explicit Cleanup**: There is no mechanism to keep the Arena alive beyond the function scope, and no explicit signal to the compiler that the Arena can be safely dropped after materialization completes.

4. **Lifetime Inference Limitation**: The compiler cannot infer that the returned `*mut cJSON` pointer does not actually reference Arena memory (all data is copied), so it conservatively rejects the code to prevent potential use-after-free.

## Correctness Properties

Property 1: Bug Condition - Arena Heap Allocation Enables Compilation

_For any_ JSON input where `cJSON_Parse` is called, the fixed function SHALL allocate the Arena on the heap via `Box::new`, allowing the Arena to outlive the immutable borrow taken by `materialize_arena_node`, enabling the Rust compiler to verify that all Arena data is copied before the Arena is dropped, and the function SHALL compile without borrow checker errors.

**Validates: Requirements 2.1, 2.2, 2.3, 2.4, 2.5, 2.6**

Property 2: Preservation - Non-Arena Code Behavior

_For any_ function call to the parser module, Arena module methods, or deletion functions that does NOT involve the Arena lifetime bug in `cJSON_Parse`, the fixed code SHALL produce exactly the same behavior as the original code, preserving parsing correctness, materialization correctness, deletion correctness, and all existing safety guarantees.

**Validates: Requirements 3.1, 3.2, 3.3, 3.4, 3.5, 3.6**

## Fix Implementation

### Changes Required

Assuming our root cause analysis is correct:

**File**: `cjson-rs/src/ffi_impl.rs`

**Function**: `cJSON_Parse`

**Specific Changes**:

1. **Arena Heap Allocation**: Replace stack allocation with heap allocation
   - Change `let mut arena = Arena::new()` to `let mut arena = Box::new(Arena::new())`
   - This moves the Arena to the heap, giving it an independent lifetime not tied to the stack frame

2. **Ownership Through Materialization**: Pass the heap-allocated Arena through materialization
   - Store the `Box<Arena>` before materialization: `let arena_box = arena;`
   - Borrow from the Box during materialization: `materialize_arena_node(&arena_box, root_id)`
   - The Box ensures the Arena outlives the borrow

3. **Explicit Cleanup**: Drop the Arena after materialization completes
   - After `materialize_arena_node` returns, the immutable borrow ends
   - Let the Box go out of scope naturally: `drop(arena_box);` (or implicit drop at end of scope)
   - This signals to the compiler that the Arena is no longer needed

4. **Error Handling Preservation**: Maintain null-pointer return on parse failure
   - Keep the early return pattern: `Err(_) => return ptr::null_mut();`
   - Ensure Arena is dropped on error path (Box drop is automatic)

5. **Documentation Update**: Add comment explaining the heap allocation strategy
   - Document why `Box::new(Arena::new())` is necessary (lifetime extension)
   - Note that the Arena is safely dropped after all data is copied

**Pseudo-code for the fix:**
```rust
#[no_mangle]
pub unsafe extern "C" fn cJSON_Parse(value: *const c_char) -> *mut cJSON {
    // Step 1: Null-pointer guard
    if value.is_null() {
        return ptr::null_mut();
    }

    // Step 2: Convert to byte slice
    let c_str = unsafe { CStr::from_ptr(value) };
    let input: &[u8] = c_str.to_bytes();

    // Step 3: Parse into heap-allocated Arena
    // CRITICAL: Arena must be on heap to outlive the borrow in Step 4
    let mut arena = Box::new(Arena::new());
    let root_index = match parse_json(input, &mut arena) {
        Ok(idx) => idx,
        Err(_) => {
            // Arena Box dropped here automatically on error
            return ptr::null_mut();
        }
    };

    // Step 4: Materialize Arena tree → cJSON linked list
    let root_id = NodeId::from_raw(root_index);
    let c_tree = materialize_arena_node(&arena, root_id);
    
    // Step 5: Arena can now be safely dropped (all data copied to c_tree)
    drop(arena);
    
    c_tree
}
```

**Alternative considered: Extending materialize_arena_node signature**
```rust
// This alternative is REJECTED because it complicates the API
fn materialize_arena_node_with_arena_ownership(arena: Box<Arena>, node_id: NodeId) -> *mut cJSON {
    let result = materialize_arena_node(&arena, node_id);
    drop(arena);
    result
}
```
**Rejection rationale:** This couples Arena lifetime management to the materialization function, reducing separation of concerns. The heap allocation approach is cleaner because it keeps materialization as a pure transformation function.

## Testing Strategy

### Validation Approach

The testing strategy follows a two-phase approach: first, verify the fix compiles and passes all existing tests (exploratory bug condition checking), then verify materialization correctness and memory safety with new tests (fix checking and preservation checking).

### Exploratory Bug Condition Checking

**Goal**: Verify that the fixed code compiles without borrow checker errors and that the existing FFI test suite passes on the FIXED code. This confirms that the Arena lifetime bug has been resolved.

**Test Plan**: Run `cargo build` and `cargo test` on the FIXED code. The compiler should accept the borrow checker constraints, and all existing tests should pass without modification.

**Test Cases**:
1. **Compilation Test**: Run `cargo build --lib` (will fail on unfixed code due to borrow checker error)
2. **FFI Test Suite**: Run `cargo test --test ffi_impl_tests` (existing tests like `parse_null_literal`, `parse_array_of_numbers`, etc.)
3. **Round-Trip Test**: Run the existing `parse_then_delete_round_trip` test (verifies Arena → C tree → delete path)
4. **Memory Sanitizer**: Run tests with ASAN/MSAN to detect use-after-free or leaks (should be clean)

**Expected Counterexamples (on unfixed code)**:
- Borrow checker error: `arena` does not live long enough
- Compilation failure before tests can run

**Expected Success (on fixed code)**:
- Compilation succeeds
- All existing FFI tests pass
- No memory safety violations detected by sanitizers

### Fix Checking

**Goal**: Verify that for all inputs where the bug condition holds, the fixed function produces the expected behavior (successful parse and materialization without lifetime errors).

**Pseudocode:**
```
FOR ALL json_input WHERE isBugCondition(cJSON_Parse_unfixed) DO
  arena := Box::new(Arena::new())
  result := cJSON_Parse'(json_input)
  
  ASSERT result IS heap_allocated
  ASSERT result DOES_NOT_REFERENCE arena_memory
  ASSERT materialize_completes BEFORE arena_drops
  ASSERT no_borrow_checker_errors()
  
  cJSON_Delete(result)  // Verify cleanup works
END FOR
```

**Test Plan**: Write targeted tests that exercise different JSON structures to ensure the Arena lifetime fix works across all parse scenarios.

**Test Cases**:
1. **Simple Literal Test**: Parse `"null"`, verify result is non-null, call `cJSON_Delete`
2. **Nested Structure Test**: Parse `{"a": [1, 2], "b": {"c": true}}`, verify structure integrity, call `cJSON_Delete`
3. **Large Document Test**: Parse a JSON document with 100+ nodes, verify no memory leaks
4. **Unicode String Test**: Parse `"\\u00e9\\uD83C\\uDF89"`, verify correct UTF-8 encoding in C strings

### Preservation Checking

**Goal**: Verify that for all inputs where the bug condition does NOT hold, the fixed function produces the same result as the original function.

**Pseudocode:**
```
FOR ALL usage WHERE NOT isBugCondition(usage) DO
  ASSERT F(usage) = F'(usage)
END FOR
```

**Testing Approach**: Property-based testing is recommended for preservation checking because:
- It generates many test cases automatically across the input domain
- It catches edge cases that manual unit tests might miss
- It provides strong guarantees that behavior is unchanged for all non-buggy inputs

**Test Plan**: Run the existing test suite on FIXED code without any test modifications. All tests should pass identically to the unfixed code (except the unfixed code doesn't compile).

**Test Cases**:
1. **Parser Preservation**: Run `cargo test --lib parser::tests` - all parser tests must pass unchanged
2. **Arena Preservation**: Run `cargo test --lib arena::tests` - all arena tests must pass unchanged
3. **Deletion Preservation**: Run existing `cJSON_Delete` tests - `delete_single_node_no_strings`, `delete_node_with_owned_strings`, etc.
4. **Error Handling Preservation**: Verify `parse_null_input_returns_null`, `parse_invalid_json_returns_null`, `parse_empty_string_returns_null` still work

### Unit Tests

**Existing tests to verify (no modification required):**
- `parse_null_literal`, `parse_true_literal`, `parse_false_literal` - verify literal parsing works
- `parse_number` - verify IEEE 754 number parsing works
- `parse_string` - verify string with escapes works
- `parse_empty_array` - verify empty container handling
- `parse_array_of_numbers` - verify sibling chain linking
- `parse_object_with_members` - verify key-value materialization
- `parse_nested_document` - verify recursive materialization
- All `delete_*` tests - verify cleanup correctness

**New tests to add (if needed):**
- Explicit test that verifies Arena is heap-allocated (compile-time only, no runtime test needed)
- Test that measures Arena memory usage before/after materialization (should be independent)

### Property-Based Tests

**Property 1: Arena Independence**
- Generate random JSON documents (varying depth, node count, types)
- For each document, parse and materialize
- Verify: `materialize_arena_node` returns a pointer that does not reference Arena memory
- Verify: `cJSON_Delete` correctly frees all memory without double-free

**Property 2: Lifetime Safety**
- Generate random JSON documents
- For each document, call `cJSON_Parse` then immediately `cJSON_Delete`
- Verify: No ASAN/MSAN violations (use-after-free, heap-use-after-free)
- Verify: No memory leaks (Valgrind or LeakSanitizer)

**Property 3: Materialization Correctness**
- Generate random JSON documents with known structure
- Parse via `cJSON_Parse`, traverse the C tree
- Verify: Tree structure matches expected (children, siblings, types, values)
- Verify: String values are correctly NUL-terminated and match expected content

### Integration Tests

**Test 1: Full parse-inspect-delete cycle**
```rust
#[test]
fn integration_parse_materialize_delete() {
    let json = r#"{"library":"cJSON","version":1.7,"active":true}"#;
    let input = CString::new(json).unwrap();
    
    // Parse (Arena created and dropped internally)
    let root = unsafe { cJSON_Parse(input.as_ptr()) };
    assert!(!root.is_null());
    
    // Inspect structure
    unsafe {
        assert_eq!((*root).type_ & 0xFF, CJSON_OBJECT);
        let first_child = (*root).child;
        assert!(!first_child.is_null());
        let key = CStr::from_ptr((*first_child).string);
        assert_eq!(key.to_str().unwrap(), "library");
    }
    
    // Delete (must not crash or leak)
    unsafe { cJSON_Delete(root) };
}
```

**Test 2: Error path - invalid JSON**
```rust
#[test]
fn integration_parse_error_cleanup() {
    let input = CString::new("{invalid}").unwrap();
    let result = unsafe { cJSON_Parse(input.as_ptr()) };
    assert!(result.is_null());
    // Arena should be dropped on error path (Box drop is automatic)
}
```

**Test 3: Deeply nested document (1000 levels)**
```rust
#[test]
fn integration_deep_nesting() {
    // Generate: {"a":{"a":{"a": ... }}} (1000 levels)
    let mut json = String::new();
    for _ in 0..999 {
        json.push_str(r#"{"a":"#);
    }
    json.push_str("null");
    for _ in 0..999 {
        json.push('}');
    }
    
    let input = CString::new(json).unwrap();
    let root = unsafe { cJSON_Parse(input.as_ptr()) };
    assert!(!root.is_null());
    
    unsafe { cJSON_Delete(root) };
}
```

**Test 4: Memory leak detection with repeated parsing**
```rust
#[test]
fn integration_no_memory_leaks() {
    let json = r#"[1,2,3,4,5]"#;
    let input = CString::new(json).unwrap();
    
    // Parse and delete 1000 times
    for _ in 0..1000 {
        let root = unsafe { cJSON_Parse(input.as_ptr()) };
        assert!(!root.is_null());
        unsafe { cJSON_Delete(root) };
    }
    
    // If there are leaks, memory usage will grow significantly
    // Run with Valgrind or LeakSanitizer to verify
}
```
