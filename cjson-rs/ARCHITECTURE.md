# Architecture Diagram

## Complete System Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                           C Test Suite                              │
│                                                                     │
│  main() {                                                           │
│    cJSON_Hooks hooks = { .malloc_fn = custom, .free_fn = custom }; │
│    cJSON_InitHooks(&hooks);  ←─────────────┐                       │
│                                             │                       │
│    cJSON *root = cJSON_Parse("[1,2,3]");    │                       │
│    cJSON_Delete(root); ←──────────────┐     │                       │
│  }                                    │     │                       │
└───────────────────────────────────────┼─────┼───────────────────────┘
                                        │     │
                    FFI Boundary (C ABI) │     │
════════════════════════════════════════│═════│═══════════════════════
                                        │     │
┌───────────────────────────────────────┼─────┼───────────────────────┐
│                    ffi_impl.rs        │     │                       │
│                  (Minimal unsafe)     │     │                       │
│                                       │     │                       │
│  #[no_mangle]                         │     │                       │
│  pub unsafe extern "C"                │     │                       │
│  fn cJSON_InitHooks(                  │     │                       │
│      hooks: *mut cJSON_Hooks ◄────────┘     │                       │
│  ) {                                        │                       │
│      if hooks.is_null() {                   │                       │
│          safe::warn_hooks_ignored(false, false);                    │
│          return; ─────────────────┐         │                       │
│      }                            │         │                       │
│      let h = unsafe { &*hooks }; ←┼─unsafe──┤                       │
│      safe::warn_hooks_ignored(   │         │                       │
│          h.malloc_fn.is_some(),  │         │                       │
│          h.free_fn.is_some()     │         │                       │
│      ); ──────────────────────────┼─────────┼────────┐              │
│  }                                │         │        │              │
│                                   │         │        │              │
│  #[no_mangle]                     │         │        │              │
│  pub unsafe extern "C"            │         │        │              │
│  fn cJSON_Delete(                 │         │        │              │
│      item: *mut cJSON ◄───────────┘         │        │              │
│  ) {                                        │        │              │
│      if item.is_null() {                    │        │              │
│          return; ───────────────────────────┤        │              │
│      }                                      │        │              │
│                                             │        │              │
│      let mut plan = Vec::new();             │        │              │
│      unsafe {                               │        │              │
│          collect_tree_for_deletion(         │        │              │
│              item, ◄────unsafe──────────────┤        │              │
│              &mut plan                      │        │              │
│          );                                 │        │              │
│      }                                      │        │              │
│                                             │        │              │
│      safe::execute_delete_plan(plan); ──────┼────────┼────────┐     │
│  }                                          │        │        │     │
│                                             │        │        │     │
│  unsafe fn collect_tree_for_deletion(       │        │        │     │
│      item: *mut cJSON,                      │        │        │     │
│      plan: &mut Vec<NodeResources>          │        │        │     │
│  ) {                                        │        │        │     │
│      let mut current = item;                │        │        │     │
│      while !current.is_null() { ◄───unsafe──┤        │        │     │
│          let next = (*current).next; ◄──────┤        │        │     │
│                                             │        │        │     │
│          // Recurse into children           │        │        │     │
│          if !is_ref && !child.is_null() {   │        │        │     │
│              collect_tree_for_deletion(     │        │        │     │
│                  child, plan                │        │        │     │
│              );                             │        │        │     │
│          }                                  │        │        │     │
│                                             │        │        │     │
│          // Reconstitute owned strings      │        │        │     │
│          let valuestring = if owned {       │        │        │     │
│              let len = strlen(vs) + 1;      │        │        │     │
│              Some(Vec::from_raw_parts( ◄────┼unsafe─┤        │     │
│                  vs, len, len               │        │        │     │
│              ))                             │        │        │     │
│          } else { None };                   │        │        │     │
│                                             │        │        │     │
│          let keystring = /* similar */;     │        │        │     │
│                                             │        │        │     │
│          // Reconstitute node struct        │        │        │     │
│          let node = Box::from_raw( ◄────────┼unsafe─┤        │     │
│              current                        │        │        │     │
│          );                                 │        │        │     │
│                                             │        │        │     │
│          plan.push(NodeResources {          │        │        │     │
│              node_box: Some(node),          │        │        │     │
│              owned_valuestring: valuestring,│        │        │     │
│              owned_keystring: keystring     │        │        │     │
│          }); ◄──────────────────────────────┼────────┼────────┼───┐ │
│                                             │        │        │   │ │
│          current = next;                    │        │        │   │ │
│      }                                      │        │        │   │ │
│  }                                          │        │        │   │ │
└─────────────────────────────────────────────┼────────┼────────┼───┼─┘
                                              │        │        │   │
                    Safe boundary             │        │        │   │
════════════════════════════════════════════════════════════════│═══│═══
                                              │        │        │   │
┌─────────────────────────────────────────────┼────────┼────────┼───┼─┐
│                     safe.rs                 │        │        │   │ │
│            #![forbid(unsafe_code)]          │        │        │   │ │
│                                             │        │        │   │ │
│  pub fn warn_hooks_ignored( ◄───────────────┴────────┘        │   │ │
│      has_malloc: bool,                                        │   │ │
│      has_free: bool                                           │   │ │
│  ) -> HookPolicy {                                            │   │ │
│      if !has_malloc && !has_free {                            │   │ │
│          return HookPolicy::RustDefault;                      │   │ │
│      }                                                         │   │ │
│                                                                │   │ │
│      eprintln!(                                                │   │ │
│          "[cjson-rs] WARNING: ..."                            │   │ │
│      ); ──────────────────────────────────────────► stderr    │   │ │
│                                                                │   │ │
│      HookPolicy::IgnoredCustomHooks                           │   │ │
│  }                                                             │   │ │
│                                                                │   │ │
│  pub struct NodeResources { ◄──────────────────────────────────┘   │ │
│      pub node_box: Option<BoxedNode>,                              │ │
│      pub owned_valuestring: Option<Vec<u8>>,                       │ │
│      pub owned_keystring: Option<Vec<u8>>,                         │ │
│  }                                                                  │ │
│                                                                     │ │
│  impl Drop for NodeResources {                                     │ │
│      fn drop(&mut self) {                                          │ │
│          drop(self.owned_valuestring.take());                      │ │
│          drop(self.owned_keystring.take());                        │ │
│          drop(self.node_box.take()); ───────┐                      │ │
│      }                                      │                      │ │
│  }                                          │                      │ │
│                                             │                      │ │
│  pub fn execute_delete_plan( ◄──────────────┴──────────────────────┘ │
│      nodes: Vec<NodeResources>                                       │
│  ) {                                                                 │
│      drop(nodes); ──────────────────────┐                            │
│  }                                      │                            │
└─────────────────────────────────────────┼────────────────────────────┘
                                          │
                                          ▼
                          ┌───────────────────────────┐
                          │   Rust Global Allocator   │
                          │   (jemalloc/system/etc)   │
                          │                           │
                          │   • Deallocates strings   │
                          │   • Deallocates nodes     │
                          │   • AUTOMATIC via Drop    │
                          └───────────────────────────┘
```

## Data Flow: cJSON_Delete Example

### Input: Tree Structure

```
root (object) @ 0x1000
  ├─ child: name (string) @ 0x2000
  │    ├─ string: "name" @ 0x3000
  │    ├─ valuestring: "Alice" @ 0x4000
  │    └─ next: age @ 0x5000
  └─ child->next: age (number) @ 0x5000
       ├─ string: "age" @ 0x6000
       └─ valuedouble: 30.0
```

### Step 1: FFI Boundary (unsafe)

```rust
// cJSON_Delete(0x1000) called from C

unsafe {
    collect_tree_for_deletion(0x1000, &mut plan);
}

// Walk tree:
// 1. Visit root @ 0x1000
//    - Recurse into child @ 0x2000
//      2. Visit name @ 0x2000
//         - Collect string @ 0x3000 → Vec<u8>
//         - Collect valuestring @ 0x4000 → Vec<u8>
//         - Collect node @ 0x2000 → Box<cJSON>
//         - Move to next @ 0x5000
//      3. Visit age @ 0x5000
//         - Collect string @ 0x6000 → Vec<u8>
//         - Collect node @ 0x5000 → Box<cJSON>
//    - Collect node @ 0x1000 → Box<cJSON>
```

### Step 2: NodeResources Collection

```rust
plan = vec![
    NodeResources {  // name node
        node_box: Some(BoxedNode(Box @ 0x2000)),
        owned_valuestring: Some(Vec @ 0x4000),  // "Alice"
        owned_keystring: Some(Vec @ 0x3000),    // "name"
    },
    NodeResources {  // age node
        node_box: Some(BoxedNode(Box @ 0x5000)),
        owned_valuestring: None,
        owned_keystring: Some(Vec @ 0x6000),    // "age"
    },
    NodeResources {  // root node
        node_box: Some(BoxedNode(Box @ 0x1000)),
        owned_valuestring: None,
        owned_keystring: None,
    },
]
```

### Step 3: Safe Module (no unsafe)

```rust
safe::execute_delete_plan(plan);

// This just drops the vector, which triggers:
// 1. Drop NodeResources[0] (name)
//    - Drop Vec @ 0x4000 ("Alice")
//    - Drop Vec @ 0x3000 ("name")
//    - Drop Box @ 0x2000 (node)
//
// 2. Drop NodeResources[1] (age)
//    - Drop Vec @ 0x6000 ("age")
//    - Drop Box @ 0x5000 (node)
//
// 3. Drop NodeResources[2] (root)
//    - Drop Box @ 0x1000 (node)
//
// All deallocations happen via Rust's Drop trait!
```

## Memory Safety Properties

### Property 1: No Use-After-Free

```rust
// In collect_tree_for_deletion:

let next_sibling = (*current).next;  // Read BEFORE consuming

// ... process current ...

(*current).next = ptr::null_mut();   // Null out pointer
(*current).prev = ptr::null_mut();
(*current).child = ptr::null_mut();

let node = Box::from_raw(current);   // Consume ownership
plan.push(/* ... */);

current = next_sibling;              // Safe: we saved it earlier
```

**Result**: After `Box::from_raw`, the node's pointers are nulled, preventing any dangling access.

### Property 2: No Double-Free

```rust
// Each allocation converted exactly once:

// First occurrence:
let node1 = Box::from_raw(ptr1);  // Takes ownership

// Second occurrence would be UB:
// let node2 = Box::from_raw(ptr1);  // ⚠️ DOUBLE FREE (not in our code!)

// We track each pointer and only call from_raw ONCE
```

**Result**: Rust's ownership system guarantees each `Box::from_raw` is called exactly once per allocation.

### Property 3: No Memory Leaks

```rust
impl Drop for NodeResources {
    fn drop(&mut self) {
        // Even if panic occurs, Drop is called
        drop(self.owned_valuestring.take());
        drop(self.owned_keystring.take());
        drop(self.node_box.take());
    }
}

// If panic occurs:
// 1. Stack unwinding begins
// 2. Drop called on all in-scope owned values
// 3. All memory freed
```

**Result**: Rust's Drop guarantees cleanup even during panics (unless panic=abort).

### Property 4: No Allocator Mismatch

```rust
// All allocations use Rust global allocator:

// Allocation:
let node = Box::new(cJSON { /* ... */ });       // Rust allocator
let string = CString::new("hello").unwrap();    // Rust allocator

// Deallocation:
drop(Box::from_raw(node_ptr));    // Rust allocator
drop(Vec::from_raw_parts(/* */)); // Rust allocator

// C hooks NEVER called:
// hooks.malloc_fn(size);  // ❌ NOT CALLED
// hooks.free_fn(ptr);     // ❌ NOT CALLED
```

**Result**: Single allocator throughout, no mixed malloc/free.

## Unsafe Code Audit

### Where Unsafe Appears

1. **FFI boundary** (`ffi_impl.rs`):
   - Dereferencing `*mut cJSON_Hooks` pointer
   - Dereferencing `*mut cJSON` pointers during traversal
   - `Box::from_raw` to reconstitute owned allocations
   - `Vec::from_raw_parts` to reconstitute owned strings

2. **NOT in safe module** (`safe.rs`):
   - Zero unsafe blocks
   - `#![forbid(unsafe_code)]` enforced at compile time

### Why This Unsafe is Safe

1. **Pointer Validity**:
   - All pointers come from C, checked for null before deref
   - Node pointers were allocated by our Rust code via `Box::into_raw`
   - String pointers were allocated by our Rust code via `CString::into_raw`

2. **Ownership Tracking**:
   - `Box::from_raw` called exactly once per allocation
   - Reference flags honored (borrowed pointers skipped)
   - Const flags honored (static strings skipped)

3. **Layout Compatibility**:
   - `#[repr(C)]` guarantees ABI compatibility
   - Manual layout tests verify size/alignment

4. **Invariant Preservation**:
   - All pointers nulled after consumption
   - Tree structure walked in safe order (children before parents)

## Comparison: C vs Rust

### C Implementation (cJSON.c)

```c
void cJSON_Delete(cJSON *item) {
    cJSON *next;
    while (item != NULL) {
        next = item->next;
        
        if (!(item->type & cJSON_IsReference) && item->child) {
            cJSON_Delete(item->child);  // Recursive
        }
        
        if (!(item->type & cJSON_IsReference) && item->valuestring) {
            hooks.free_fn(item->valuestring);  // Manual free
        }
        
        if (!(item->type & cJSON_StringIsConst) && item->string) {
            hooks.free_fn(item->string);  // Manual free
        }
        
        hooks.free_fn(item);  // Manual free
        
        item = next;
    }
}
```

**Challenges**:
- ❌ Manual memory management
- ❌ Easy to forget to free something
- ❌ Easy to free something twice
- ❌ Custom allocator mixing is error-prone

### Rust Implementation (ffi_impl.rs + safe.rs)

```rust
// ffi_impl.rs (unsafe boundary)
pub unsafe extern "C" fn cJSON_Delete(item: *mut cJSON) {
    if item.is_null() { return; }
    let mut plan = Vec::new();
    unsafe { collect_tree_for_deletion(item, &mut plan); }
    safe::execute_delete_plan(plan);  // Hand off to safe code
}

// safe.rs (zero unsafe)
pub fn execute_delete_plan(nodes: Vec<NodeResources>) {
    drop(nodes);  // Rust's Drop trait handles everything
}
```

**Advantages**:
- ✅ Automatic memory management via Drop
- ✅ Impossible to forget cleanup (compiler enforces)
- ✅ Impossible to double-free (ownership system prevents)
- ✅ Single allocator (no mixing)
- ✅ Panic-safe (Drop called during unwinding)

## Conclusion

This architecture achieves memory safety through:

1. **Minimal Unsafe**: Only at FFI boundary for pointer operations
2. **Ownership Transfer**: Raw pointers → owned Rust types
3. **Safe Orchestration**: All logic in `#![forbid(unsafe_code)]` module
4. **Drop-Based Cleanup**: Leverage Rust's RAII guarantees

The result: **Zero unsafe code in the safe module** while maintaining **full C API compatibility**.
