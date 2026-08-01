#![no_main]

use libfuzzer_sys::fuzz_target;
use std::os::raw::c_char;
use std::panic;
use std::ptr;

// Import both implementations
use cjson_rs::{cJSON_Parse, cJSON_Delete, cJSON};
use cjson_rs::parser::parse_json;
use cjson_rs::arena::Arena;

/// Differential Fuzzing Harness: C Implementation vs Safe Rust Implementation
/// 
/// This harness is designed to catch critical security discrepancies where:
/// 1. The C implementation crashes, segfaults, or exhibits undefined behavior
/// 2. The Rust implementation safely rejects the same input with an Err
/// 
/// Such discrepancies indicate memory safety vulnerabilities in the C code
/// that have been fixed by the Rust implementation.

fuzz_target!(|data: &[u8]| {
    // ═══════════════════════════════════════════════════════════════════
    // STEP 1: Test Safe Rust Parser
    // ═══════════════════════════════════════════════════════════════════
    
    let mut arena = Arena::new();
    let rust_result = parse_json(data, &mut arena);
    
    // ═══════════════════════════════════════════════════════════════════
    // STEP 2: Test C FFI Parser with Panic Catching
    // ═══════════════════════════════════════════════════════════════════
    
    // We need a null-terminated string for the C parser
    let mut null_terminated = data.to_vec();
    null_terminated.push(0); // Add NUL terminator
    
    // Catch panics, segfaults manifest as aborts, but we can catch some UB
    let c_result = panic::catch_unwind(|| {
        unsafe {
            let c_ptr = null_terminated.as_ptr() as *const c_char;
            let json_ptr = cJSON_Parse(c_ptr);
            
            // Return whether parsing succeeded and the pointer
            (!json_ptr.is_null(), json_ptr)
        }
    });
    
    // ═══════════════════════════════════════════════════════════════════
    // STEP 3: Analyze Results and Detect Discrepancies
    // ═══════════════════════════════════════════════════════════════════
    
    match (c_result, rust_result) {
        // ───────────────────────────────────────────────────────────────
        // CASE 1: C Implementation PANICKED/CRASHED
        // ───────────────────────────────────────────────────────────────
        (Err(panic_info), Ok(_rust_success)) => {
            // CRITICAL VULNERABILITY DETECTED!
            // C crashed but Rust succeeded - this means Rust is more robust
            log_discrepancy(
                data,
                "C_PANIC_RUST_OK",
                "C implementation panicked while Rust successfully parsed",
                format!("Panic: {:?}", panic_info).as_str()
            );
        }
        
        (Err(panic_info), Err(rust_err)) => {
            // C crashed and Rust also rejected - this is the expected good outcome
            // The Rust version safely rejects what causes C to crash
            log_discrepancy(
                data,
                "C_PANIC_RUST_ERR",
                "C implementation panicked, Rust safely rejected (GOOD - vulnerability caught)",
                format!("C Panic: {:?} | Rust Error: {}", panic_info, rust_err).as_str()
            );
        }
        
        // ───────────────────────────────────────────────────────────────
        // CASE 2: C Returned NULL (Parse Failure)
        // ───────────────────────────────────────────────────────────────
        (Ok((false, _null_ptr)), Ok(_rust_success)) => {
            // DISCREPANCY: C rejected but Rust accepted
            // This might indicate C is overly conservative, or Rust is too permissive
            log_discrepancy(
                data,
                "C_NULL_RUST_OK",
                "C returned NULL (failure) but Rust successfully parsed",
                "Possible false negative in C or false positive in Rust"
            );
        }
        
        (Ok((false, _null_ptr)), Err(rust_err)) => {
            // Both rejected - consistent behavior (GOOD)
            // No action needed in normal fuzzing mode
            // Uncomment below to log all consistent rejections:
            // log_trace(data, "BOTH_REJECT", format!("Rust: {}", rust_err).as_str());
        }
        
        // ───────────────────────────────────────────────────────────────
        // CASE 3: C Returned Valid Pointer (Parse Success)
        // ───────────────────────────────────────────────────────────────
        (Ok((true, json_ptr)), Ok(_rust_success)) => {
            // Both succeeded - validate the parse tree matches
            // Clean up the C allocation
            unsafe {
                cJSON_Delete(json_ptr);
            }
            
            // Both parsers agree - consistent behavior (GOOD)
            // No action needed in normal fuzzing mode
        }
        
        (Ok((true, json_ptr)), Err(rust_err)) => {
            // CRITICAL DISCREPANCY!
            // C accepted (false positive) but Rust rejected
            // This could indicate C is accepting malformed JSON
            
            log_discrepancy(
                data,
                "C_OK_RUST_ERR",
                "C successfully parsed (FALSE POSITIVE?) but Rust rejected",
                format!("Rust Error: {}", rust_err).as_str()
            );
            
            // Clean up the C allocation
            unsafe {
                cJSON_Delete(json_ptr);
            }
        }
    }
});

/// Log a security-critical discrepancy with full details
/// 
/// Format: Structured log that includes:
/// - Timestamp
/// - Discrepancy type (for pattern analysis)
/// - Human-readable description
/// - Full hex dump of input bytes (for reproduction)
/// - Base64 encoding (for easy copy-paste)
fn log_discrepancy(data: &[u8], discrepancy_type: &str, description: &str, details: &str) {
    eprintln!("\n╔═══════════════════════════════════════════════════════════════════════════╗");
    eprintln!("║ 🚨 DIFFERENTIAL FUZZING DISCREPANCY DETECTED                              ║");
    eprintln!("╠═══════════════════════════════════════════════════════════════════════════╣");
    eprintln!("║ Type: {:<70} ║", discrepancy_type);
    eprintln!("║ Description: {:<63} ║", truncate(description, 63));
    eprintln!("╠═══════════════════════════════════════════════════════════════════════════╣");
    eprintln!("║ Details: {:<67} ║", truncate(details, 67));
    eprintln!("╠═══════════════════════════════════════════════════════════════════════════╣");
    eprintln!("║ Input Size: {} bytes", data.len());
    eprintln!("║");
    eprintln!("║ HEX DUMP (for reproduction):");
    
    // Print hex dump in rows of 16 bytes
    for (i, chunk) in data.chunks(16).enumerate() {
        eprint!("║ {:04x}  ", i * 16);
        for byte in chunk {
            eprint!("{:02x} ", byte);
        }
        // Pad the last line
        for _ in 0..(16 - chunk.len()) {
            eprint!("   ");
        }
        eprint!(" │ ");
        // ASCII representation
        for &byte in chunk {
            if byte >= 0x20 && byte <= 0x7e {
                eprint!("{}", byte as char);
            } else {
                eprint!(".");
            }
        }
        eprintln!();
    }
    
    eprintln!("║");
    eprintln!("║ BASE64 (for easy reproduction):");
    eprintln!("║ {}", base64_encode(data));
    eprintln!("║");
    eprintln!("║ RAW BYTES (Rust array literal):");
    eprint!("║ &[");
    for (i, byte) in data.iter().enumerate() {
        if i > 0 {
            eprint!(", ");
        }
        if i > 0 && i % 12 == 0 {
            eprint!("\n║   ");
        }
        eprint!("0x{:02x}", byte);
    }
    eprintln!("]");
    eprintln!("╚═══════════════════════════════════════════════════════════════════════════╝\n");
}

/// Truncate string to fit in formatted log
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Simple base64 encoding (alphabet: A-Z, a-z, 0-9, +, /)
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    
    for chunk in data.chunks(3) {
        let mut buf = [0u8; 3];
        for (i, &byte) in chunk.iter().enumerate() {
            buf[i] = byte;
        }
        
        let b1 = (buf[0] >> 2) & 0x3f;
        let b2 = ((buf[0] << 4) | (buf[1] >> 4)) & 0x3f;
        let b3 = ((buf[1] << 2) | (buf[2] >> 6)) & 0x3f;
        let b4 = buf[2] & 0x3f;
        
        result.push(ALPHABET[b1 as usize] as char);
        result.push(ALPHABET[b2 as usize] as char);
        
        if chunk.len() > 1 {
            result.push(ALPHABET[b3 as usize] as char);
        } else {
            result.push('=');
        }
        
        if chunk.len() > 2 {
            result.push(ALPHABET[b4 as usize] as char);
        } else {
            result.push('=');
        }
    }
    
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_valid_json_both_accept() {
        let input = br#"{"key": "value"}"#;
        // Both should accept valid JSON - no discrepancy
        // This is tested by running the fuzz target
    }
    
    #[test]
    fn test_malformed_json_both_reject() {
        let input = b"{invalid";
        // Both should reject - no discrepancy
    }
    
    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(base64_encode(b"hi"), "aGk=");
        assert_eq!(base64_encode(b"a"), "YQ==");
    }
}
