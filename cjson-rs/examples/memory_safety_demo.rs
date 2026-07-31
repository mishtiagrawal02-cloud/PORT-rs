//! # Memory Safety Demonstration for cJSON_InitHooks and cJSON_Delete
//!
//! This example demonstrates the safe Rust implementation of cJSON's memory
//! management functions, showing:
//!
//! 1. How custom C allocator hooks are safely ignored
//! 2. How cJSON_Delete properly cleans up complex tree structures
//! 3. How reference flags are honored to prevent double-frees
//! 4. How the implementation maintains C API compatibility
//!
//! Run with:
//! ```bash
//! cargo run --example memory_safety_demo
//! ```

#![allow(non_snake_case, dead_code)]

use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::ptr;

// Import the FFI types and functions from our crate
use cjson_rs::{
    cJSON, cJSON_Hooks, cJSON_InitHooks, cJSON_Delete,
    CJSON_OBJECT, CJSON_STRING, CJSON_ARRAY, CJSON_NUMBER,
    CJSON_IS_REFERENCE, CJSON_STRING_IS_CONST,
};

fn main() {
    println!("=== cJSON Rust Memory Safety Demo ===\n");

    demo_init_hooks();
    demo_delete_simple();
    demo_delete_with_strings();
    demo_delete_tree();
    demo_reference_nodes();
    
    println!("\n=== All demos completed successfully ===");
}

/// Demonstrate cJSON_InitHooks behavior
fn demo_init_hooks() {
    println!("--- Demo 1: cJSON_InitHooks ---");
    
    // Case 1: NULL hooks (reset to default)
    println!("Calling cJSON_InitHooks(NULL)...");
    unsafe {
        cJSON_InitHooks(ptr::null_mut());
    }
    println!("✓ No crash, no warning\n");
    
    // Case 2: Custom hooks (will be ignored with warning)
    println!("Calling cJSON_InitHooks with custom allocators...");
    let mut hooks = cJSON_Hooks {
        malloc_fn: Some(demo_custom_malloc),
        free_fn: Some(demo_custom_free),
    };
    
    unsafe {
        cJSON_InitHooks(&mut hooks as *mut cJSON_Hooks);
    }
    println!("✓ Custom hooks safely ignored (warning should appear above)\n");
}

/// Demonstrate deleting a simple node without strings
fn demo_delete_simple() {
    println!("--- Demo 2: Delete Simple Node ---");
    
    // Manually allocate a simple cJSON node (simulating what cJSON_CreateNumber does)
    let node = Box::new(cJSON {
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        child: ptr::null_mut(),
        type_: CJSON_NUMBER,
        valuestring: ptr::null_mut(),
        valueint: 42,
        valuedouble: 42.0,
        string: ptr::null_mut(),
    });
    
    let node_ptr = Box::into_raw(node);
    println!("Created node at {:?}", node_ptr);
    
    // Delete it
    unsafe {
        cJSON_Delete(node_ptr);
    }
    println!("✓ Node deleted successfully\n");
}

/// Demonstrate deleting a node with owned strings
fn demo_delete_with_strings() {
    println!("--- Demo 3: Delete Node With Strings ---");
    
    let node = Box::new(cJSON {
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        child: ptr::null_mut(),
        type_: CJSON_STRING,
        valuestring: ptr::null_mut(),
        valueint: 0,
        valuedouble: 0.0,
        string: ptr::null_mut(),
    });
    
    let node_ptr = Box::into_raw(node);
    
    // Attach owned strings
    let value = CString::new("Hello, Rust!").unwrap();
    let key = CString::new("greeting").unwrap();
    
    unsafe {
        (*node_ptr).valuestring = value.into_raw();
        (*node_ptr).string = key.into_raw();
        
        println!("Created string node with key='greeting', value='Hello, Rust!'");
        
        // Delete — should free both strings and the node
        cJSON_Delete(node_ptr);
    }
    println!("✓ Node and both strings deleted successfully\n");
}

/// Demonstrate deleting a tree with children and siblings
fn demo_delete_tree() {
    println!("--- Demo 4: Delete Complex Tree ---");
    println!("Building tree structure:");
    println!("  root (object)");
    println!("    ├─ name: \"John Doe\" (string)");
    println!("    └─ age: 30 (number)");
    
    // Create root object
    let root = Box::new(cJSON {
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        child: ptr::null_mut(),
        type_: CJSON_OBJECT,
        valuestring: ptr::null_mut(),
        valueint: 0,
        valuedouble: 0.0,
        string: ptr::null_mut(),
    });
    let root_ptr = Box::into_raw(root);
    
    // Create "name" child
    let name_node = Box::new(cJSON {
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        child: ptr::null_mut(),
        type_: CJSON_STRING,
        valuestring: CString::new("John Doe").unwrap().into_raw(),
        valueint: 0,
        valuedouble: 0.0,
        string: CString::new("name").unwrap().into_raw(),
    });
    let name_ptr = Box::into_raw(name_node);
    
    // Create "age" child
    let age_node = Box::new(cJSON {
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        child: ptr::null_mut(),
        type_: CJSON_NUMBER,
        valuestring: ptr::null_mut(),
        valueint: 30,
        valuedouble: 30.0,
        string: CString::new("age").unwrap().into_raw(),
    });
    let age_ptr = Box::into_raw(age_node);
    
    // Link the tree structure
    unsafe {
        // name is first child
        (*root_ptr).child = name_ptr;
        
        // age is sibling of name
        (*name_ptr).next = age_ptr;
        (*age_ptr).prev = name_ptr;
        
        println!("Tree structure built, deleting entire tree...");
        
        // Delete root — should cascade to all children
        cJSON_Delete(root_ptr);
    }
    
    println!("✓ Entire tree deleted (root + 2 children + 3 strings)\n");
}

/// Demonstrate reference node handling (borrowed pointers not freed)
fn demo_reference_nodes() {
    println!("--- Demo 5: Reference Nodes ---");
    
    // Create a real node that we own
    let real_child = Box::new(cJSON {
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        child: ptr::null_mut(),
        type_: CJSON_STRING,
        valuestring: CString::new("shared value").unwrap().into_raw(),
        valueint: 0,
        valuedouble: 0.0,
        string: CString::new("shared_key").unwrap().into_raw(),
    });
    let real_child_ptr = Box::into_raw(real_child);
    
    // Create a reference node that borrows the child
    let ref_node = Box::new(cJSON {
        next: ptr::null_mut(),
        prev: ptr::null_mut(),
        child: real_child_ptr,  // Borrowed, not owned
        type_: CJSON_OBJECT | CJSON_IS_REFERENCE,  // Reference flag set
        valuestring: ptr::null_mut(),
        valueint: 0,
        valuedouble: 0.0,
        string: ptr::null_mut(),
    });
    let ref_node_ptr = Box::into_raw(ref_node);
    
    println!("Created reference node pointing to shared child");
    
    unsafe {
        // Delete reference node — should NOT delete the child
        println!("Deleting reference node (child should remain alive)...");
        cJSON_Delete(ref_node_ptr);
        println!("✓ Reference node deleted, child still alive");
        
        // Now delete the real child
        println!("Deleting real child...");
        cJSON_Delete(real_child_ptr);
        println!("✓ Real child deleted");
    }
    
    println!();
}

/// Demo custom malloc (should never be called by Rust implementation)
unsafe extern "C" fn demo_custom_malloc(size: usize) -> *mut c_void {
    panic!("demo_custom_malloc called with size {size} — this should never happen!");
}

/// Demo custom free (should never be called by Rust implementation)
unsafe extern "C" fn demo_custom_free(ptr: *mut c_void) {
    panic!("demo_custom_free called with {ptr:?} — this should never happen!");
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn all_demos_run_without_panic() {
        // If main() completes without panicking, all demos passed
        main();
    }
}
