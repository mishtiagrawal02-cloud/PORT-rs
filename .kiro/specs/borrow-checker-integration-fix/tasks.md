# Implementation Plan

## Overview

This implementation plan follows the bugfix workflow using the bug condition methodology. The fix resolves a borrow checker lifetime error in `cJSON_Parse` where the Arena allocator is borrowed during materialization but the compiler incorrectly infers that the return value depends on the Arena's lifetime. The solution introduces an explicit scope block to ensure the Arena's lifetime ends before the function returns, making it clear that the materialized C tree has no lifetime dependency on the Arena.

## Tasks

- [x] 1. Write bug condition exploration test
  - **Property 1: Bug Condition** - Arena Lifetime Conflict in cJSON_Parse
  - **CRITICAL**: This test MUST FAIL on unfixed code - failure confirms the bug exists
  - **DO NOT attempt to fix the test or the code when it fails**
  - **NOTE**: This test encodes the expected behavior - it will validate the fix when it passes after implementation
  - **GOAL**: Surface counterexamples that demonstrate the borrow checker error exists
  - **Scoped PBT Approach**: Scope the property to concrete failing cases where `cJSON_Parse` creates Arena on stack, borrows it during materialization, and attempts to return while Arena is in scope
  - Test that `cJSON_Parse` with valid JSON input causes borrow checker to fail with lifetime error
  - The test assertions should verify the borrow checker detects Arena lifetime dependency on return value
  - Run test on UNFIXED code
  - **EXPECTED OUTCOME**: Test FAILS at compile time (borrow checker error - this is correct and proves the bug exists)
  - Document counterexamples found: specific compilation error messages indicating "borrowed value does not live long enough" or "cannot return value referencing local variable `arena`"
  - Mark task complete when test is written, compilation attempted, and failure (borrow checker error) is documented
  - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6_

- [x] 2. Write preservation property tests (BEFORE implementing fix)
  - **Property 2: Preservation** - Module Independence and Existing Behavior
  - **IMPORTANT**: Follow observation-first methodology
  - Observe behavior on UNFIXED code for modules and functions not affected by the Arena lifetime issue
  - Write property-based tests capturing observed behavior patterns:
    - Arena module tests pass independently (no unsafe code, correct allocation)
    - Parser module tests pass independently (correct JSON parsing into Arena)
    - `cJSON_Delete` correctly frees C trees (no memory leaks)
    - `cJSON_InitHooks` stub behaves correctly (no crashes on null/non-null input)
    - All existing FFI tests pass (except for `cJSON_Parse` compilation)
  - Property-based testing generates many test cases for stronger guarantees
  - Run tests on UNFIXED code
  - **EXPECTED OUTCOME**: Tests PASS on unfixed code (confirms baseline behavior to preserve)
  - Mark task complete when tests are written, run, and passing on unfixed code
  - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6_

- [ ] 3. Fix for Arena lifetime conflict in cJSON_Parse

  - [~] 3.1 Implement the fix using explicit scope block approach
    - Add explicit scope block to wrap Arena allocation, parsing, and materialization
    - Structure: `let result = { let mut arena = Arena::new(); ... materialize_arena_node(&arena, root_id) };`
    - Ensure Arena lifetime ends within the scope block before `cJSON_Parse` returns
    - Alternatively, use explicit `drop(arena);` after materialization completes
    - Add documentation comment explaining lifetime guarantee: materialization copies all data to heap, Arena can be safely dropped
    - _Bug_Condition: isBugCondition(code_structure) where arena_on_stack AND arena_borrowed AND arena_drops_in_scope AND compiler_infers_dependency (from design)_
    - _Expected_Behavior: Arena borrow completes before Arena drops, return value has no lifetime dependency on Arena, code compiles without borrow checker errors (from design Expected Behavior Properties)_
    - _Preservation: No changes to Arena module, parser module, cJSON_Delete, or other FFI functions (from design Preservation Requirements)_
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

  - [~] 3.2 Verify bug condition exploration test now passes
    - **Property 1: Expected Behavior** - Code Compiles Without Borrow Checker Errors
    - **IMPORTANT**: Re-run the SAME test from task 1 - do NOT write a new test
    - The test from task 1 (attempting to compile `cJSON_Parse`) now encodes the expected behavior
    - When this test passes (code compiles), it confirms the expected behavior is satisfied
    - Run compilation test from step 1
    - **EXPECTED OUTCOME**: Test PASSES (code compiles successfully - confirms bug is fixed)
    - Verify no borrow checker errors related to Arena lifetime
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

  - [~] 3.3 Verify preservation tests still pass
    - **Property 2: Preservation** - No Behavioral Regressions
    - **IMPORTANT**: Re-run the SAME tests from task 2 - do NOT write new tests
    - Run preservation property tests from step 2
    - **EXPECTED OUTCOME**: Tests PASS (confirms no regressions)
    - Confirm Arena module tests pass (independent behavior unchanged)
    - Confirm parser module tests pass (independent behavior unchanged)
    - Confirm `cJSON_Delete` tests pass (deallocation logic unchanged)
    - Confirm `cJSON_InitHooks` tests pass (stub behavior unchanged)
    - Confirm all existing FFI tests pass (including new `cJSON_Parse` tests)
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6_

- [~] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.
  - Run full test suite: `cargo test`
  - Verify no borrow checker errors in entire codebase
  - Verify no memory leaks with AddressSanitizer (ASAN) if available: `RUSTFLAGS="-Z sanitizer=address" cargo test`
  - Confirm `cJSON_Parse` can parse and materialize all JSON types (null, bool, number, string, array, object)
  - Confirm `cJSON_Parse` → `cJSON_Delete` round-trip works without leaks

## Task Dependency Graph

```
1 (Bug Condition Test) ─┐
                         ├──> 3 (Implementation) ──> 4 (Checkpoint)
2 (Preservation Tests) ──┘
```

```json
{
  "waves": [
    {
      "name": "Pre-Implementation Testing",
      "tasks": ["1", "2"]
    },
    {
      "name": "Implementation",
      "tasks": ["3.1", "3.2", "3.3"]
    },
    {
      "name": "Verification",
      "tasks": ["4"]
    }
  ]
}
```

**Dependencies:**
- Task 1 and Task 2 are independent and can run in parallel (both are pre-implementation tests)
- Task 3 depends on Tasks 1 and 2 (implementation requires understanding bug and baseline behavior)
- Task 4 depends on Task 3 (final verification after fix is applied)

## Notes

- **Critical Note**: Task 1 is expected to FAIL at compile time on unfixed code. This is the correct behavior and confirms the bug exists. Do not attempt to fix the compilation error until Task 3.
- **Compile-Time Bug**: This is a borrow checker (compile-time) bug, not a runtime bug. The "test" in Task 1 is attempting to compile the code and documenting the compiler error.
- **Scope Block Technique**: The fix uses Rust's explicit scope block `{ ... }` to control lifetime boundaries. This makes it clear to the borrow checker that the Arena's lifetime ends before the function returns.
- **Alternative Approach**: If the scope block approach causes issues with early returns, use explicit `drop(arena)` after materialization completes.
- **Memory Safety**: The fix preserves all memory safety guarantees. The materialization process already copies all Arena data into independent heap allocations (`Box`, `CString`), so dropping the Arena is safe once materialization completes.
- **No Unsafe Changes**: The fix involves zero changes to unsafe code. It only restructures the control flow to make lifetime boundaries explicit for the borrow checker.
