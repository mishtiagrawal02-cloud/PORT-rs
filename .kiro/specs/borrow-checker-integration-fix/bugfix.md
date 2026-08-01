# Bugfix Requirements Document

## Introduction

The C-FFI wrapper (`ffi_impl.rs`) contains a critical lifetime bug in the `cJSON_Parse` function. The function creates an Arena allocator on the stack, parses JSON into arena-backed nodes, then attempts to materialize those nodes into C-compatible structs. However, the Arena is dropped before materialization completes, causing the Rust borrow checker to detect dangling references.

This bug prevents compilation of the FFI integration code despite both the Arena module and parser module being individually correct and forbidding unsafe code. The issue manifests as borrow checker errors when the Arena's lifetime ends while its data is still being accessed during the tree materialization phase.

**Impact**: Complete compilation failure of the FFI layer, preventing C code from linking against the Rust implementation.

## Bug Analysis

### Current Behavior (Defect)

1.1 WHEN `cJSON_Parse` is called with valid JSON input THEN the Arena allocator is created on the stack within the function scope

1.2 WHEN `parse_json(input, &mut arena)` executes successfully and returns a root node index THEN the arena contains parsed nodes but is still owned by the stack frame

1.3 WHEN `materialize_arena_node(&arena, root_id)` is called to convert Arena nodes to C structs THEN the function borrows the Arena immutably while still in the same scope as Arena's owner

1.4 WHEN the `cJSON_Parse` function returns a `*mut cJSON` pointer THEN the Arena is dropped at the end of the function scope, causing the borrow checker to fail because the Arena reference was used during materialization but no longer exists

1.5 WHEN the Rust compiler analyzes the lifetime relationships THEN it detects that Arena-backed data (accessed via `&Arena` during materialization) could be used after the Arena is dropped, violating memory safety guarantees

1.6 WHEN the compiler enforces the lifetime constraint `'a` on `materialize_arena_node(&'a Arena, NodeId)` THEN the returned `*mut cJSON` appears to the compiler to have a dependency on the Arena's lifetime, which ends before the function returns

### Expected Behavior (Correct)

2.1 WHEN `cJSON_Parse` creates an Arena and parses JSON THEN the Arena SHALL remain valid for the entire duration of tree materialization

2.2 WHEN `materialize_arena_node` reads from the Arena to build C-compatible structs THEN the function SHALL complete all allocations and copies before the Arena is dropped

2.3 WHEN the materialized C tree is returned via `*mut cJSON` THEN the pointer SHALL reference heap-allocated memory independent of the Arena's lifetime

2.4 WHEN the Rust compiler verifies lifetime safety THEN it SHALL confirm that no references to Arena data escape beyond the Arena's scope

2.5 WHEN the Arena is dropped at the end of `cJSON_Parse` THEN all data needed by the returned C tree SHALL already be copied into separate heap allocations (via `Box`, `CString::into_raw`)

2.6 WHEN the borrow checker analyzes `materialize_arena_node(&arena, root_id)` THEN it SHALL verify that the immutable borrow of the Arena ends before the Arena itself is dropped

### Unchanged Behavior (Regression Prevention)

3.1 WHEN the Arena module is used independently (without FFI) THEN the system SHALL CONTINUE TO forbid unsafe code via `#![forbid(unsafe_code)]`

3.2 WHEN the parser module parses JSON into an Arena THEN the system SHALL CONTINUE TO produce correct, safe index-based AST structures

3.3 WHEN `cJSON_Delete` frees a C tree returned by `cJSON_Parse` THEN the system SHALL CONTINUE TO correctly deallocate all Box and CString allocations without memory leaks

3.4 WHEN `materialize_arena_node` allocates C-compatible structs THEN the system SHALL CONTINUE TO use `Box::into_raw` and `CString::into_raw` for ownership transfer to C

3.5 WHEN the FFI test suite calls `cJSON_Parse` followed by `cJSON_Delete` THEN the system SHALL CONTINUE TO pass all memory safety checks (no use-after-free, no double-free)

3.6 WHEN the Arena contains deeply nested structures (1000+ levels) THEN the parser SHALL CONTINUE TO enforce depth limits and reject excessively nested input

---

## Bug Condition and Property Specification

### Bug Condition Function

The bug condition is met when the Arena's lifetime constraint conflicts with the materialization function's return value:

```pascal
FUNCTION isBugCondition(code_structure)
  INPUT: code_structure containing function scope, Arena lifetime, and borrow usage
  OUTPUT: boolean
  
  // Arena created on stack
  arena_on_stack ← arena IS Stack_Allocated
  
  // Arena borrowed during materialization
  arena_borrowed ← EXISTS call TO materialize_arena_node(&arena, node_id)
  
  // Arena dropped before borrow completes
  arena_dropped_early ← arena.lifetime ENDS BEFORE materialize_returns
  
  RETURN arena_on_stack AND arena_borrowed AND arena_dropped_early
END FUNCTION
```

**Concrete Example:**
```rust
pub unsafe extern "C" fn cJSON_Parse(value: *const c_char) -> *mut cJSON {
    let mut arena = Arena::new();  // ← Stack allocation
    let root_index = parse_json(input, &mut arena)?;
    let root_id = NodeId::from_raw(root_index);
    
    // Bug condition met here:
    materialize_arena_node(&arena, root_id)  // ← Borrows arena
    // Arena dropped here when function returns
    // But compiler thinks arena reference might escape via return value
}
```

### Fix Checking Property

The fix must ensure that all Arena data is copied before the Arena is dropped:

```pascal
// Property: Fix Checking - Arena Lifetime Independence
FOR ALL json_input WHERE isBugCondition(cJSON_Parse_implementation) DO
  result ← cJSON_Parse'(json_input)
  
  ASSERT result IS heap_allocated
  ASSERT result DOES_NOT_REFERENCE arena_memory
  ASSERT materialize_completes BEFORE arena_drops
  ASSERT no_borrow_checker_errors()
END FOR
```

**Key Constraints:**
- **F** (buggy code): Arena borrowed during materialization with lifetime `'arena`, return value incorrectly appears to depend on `'arena`
- **F'** (fixed code): Arena borrow scope clearly ends before Arena is dropped, return value has no lifetime dependency on Arena

### Preservation Checking Property

For all correct usage patterns, behavior must remain unchanged:

```pascal
// Property: Preservation Checking
FOR ALL usage WHERE NOT isBugCondition(usage) DO
  ASSERT F(usage) = F'(usage)
END FOR
```

**Examples of preserved behavior:**
- Arena used without FFI (pure Rust): No changes
- Parser producing correct AST: No changes  
- cJSON_Delete correctly freeing C trees: No changes
- Test suite passing: All tests continue to pass
