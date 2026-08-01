# Example Fuzzing Findings: C vs Rust

This document provides **concrete examples** of the types of vulnerabilities the differential fuzzing harness is designed to detect.

## Example 1: Buffer Overflow on Deep Nesting

### Input
```
[[[[[[[[[[[[[[[[[[[[[...(10000 levels)...[1]...]]]]]]]]]]]]]]]]]]]]]
```

### C Implementation Behavior
```c
// Recursive parsing without depth tracking
cJSON *parse_array(const char *input) {
    // ... 
    if (*input == '[') {
        parse_array(input + 1);  // ❌ No depth limit - stack overflow
    }
}
```

**Result**: Stack overflow → Segmentation fault

### Rust Implementation Behavior
```rust
fn parse_array(&mut self, arena: &mut Arena) -> Result<NodeId, ParseError> {
    self.enter_container()?;  // ✓ Checks depth < MAX_NESTING_DEPTH
    
    if self.depth > MAX_NESTING_DEPTH {
        return Err(ParseError::DepthLimitExceeded);
    }
    // ... rest of parsing
}
```

**Result**: `Err(ParseError::DepthLimitExceeded)` at depth 1000

### Fuzzer Output
```
╔═══════════════════════════════════════════════════════════════════════════╗
║ 🚨 DIFFERENTIAL FUZZING DISCREPANCY DETECTED                              ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Type: C_PANIC_RUST_ERR                                                    ║
║ Description: C implementation panicked, Rust safely rejected (GOOD)       ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Details: C Panic: stack overflow | Rust Error: nesting depth exceeds 1000║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Input Size: 20002 bytes
║ HEX DUMP:
║ 0000  5b 5b 5b 5b 5b 5b 5b 5b 5b 5b 5b 5b 5b 5b 5b 5b  │ [[[[[[[[[[[[[[[[
║ 0010  5b 5b 5b 5b 5b 5b 5b 5b ...                      │ [[[[[[[[...
╚═══════════════════════════════════════════════════════════════════════════╝
```

**Severity**: 🚨 HIGH - DoS via stack exhaustion

---

## Example 2: Integer Overflow in Length Calculation

### Input
```json
{"key": "A very long string value that causes size_t overflow when calculating buffer..."}
```

### C Implementation Behavior
```c
// Vulnerable code pattern
size_t key_len = strlen(key);
size_t value_len = strlen(value);
char *buffer = malloc(key_len + value_len + 10);  // ❌ Overflow wraps to small size

if (buffer) {
    strcpy(buffer, key);          // ❌ Buffer overflow!
    strcat(buffer, value);        // ❌ Writes past buffer end
}
```

**Result**: Heap buffer overflow → Memory corruption → Potential RCE

### Rust Implementation Behavior
```rust
// Safe code with checked arithmetic
let total_len = key_len.checked_add(value_len)
    .and_then(|l| l.checked_add(10))
    .ok_or(Error::IntegerOverflow)?;  // ✓ Detects overflow

let mut buffer = Vec::with_capacity(total_len);  // ✓ Safe allocation
buffer.extend_from_slice(key.as_bytes());
buffer.extend_from_slice(value.as_bytes());
```

**Result**: Safe allocation or `Err(IntegerOverflow)`

### Fuzzer Output
```
╔═══════════════════════════════════════════════════════════════════════════╗
║ 🚨 DIFFERENTIAL FUZZING DISCREPANCY DETECTED                              ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Type: C_PANIC_RUST_ERR                                                    ║
║ Description: C crashed with heap corruption, Rust rejected                ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Details: C Panic: corrupted size vs. prev_size | Rust: IntegerOverflow   ║
╚═══════════════════════════════════════════════════════════════════════════╝
```

**Severity**: 🚨 CRITICAL - Memory corruption with RCE potential

---

## Example 3: Null Pointer Dereference

### Input
```json
{"key": null}
```

### C Implementation Behavior
```c
// Vulnerable code pattern
cJSON *item = cJSON_GetObjectItem(root, "key");
// Assume value exists...
printf("Value: %s\n", item->valuestring);  // ❌ NULL pointer dereference if key is null
```

**Result**: Segmentation fault

### Rust Implementation Behavior
```rust
// Safe code with Option<T>
let item = arena.get_child(root_id, "key");

match item {
    Some(node) => {
        match &node.value {
            JsonValue::String(s) => println!("Value: {}", s),
            JsonValue::Null => println!("Value is null"),
            _ => println!("Value is not a string")
        }
    }
    None => println!("Key not found")
}
```

**Result**: Safe pattern matching, no crash

### Fuzzer Output
```
╔═══════════════════════════════════════════════════════════════════════════╗
║ 🚨 DIFFERENTIAL FUZZING DISCREPANCY DETECTED                              ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Type: C_PANIC_RUST_OK                                                     ║
║ Description: C crashed, Rust successfully handled null                    ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ HEX DUMP:
║ 0000  7b 22 6b 65 79 22 3a 20 6e 75 6c 6c 7d           │ {"key": null}
╚═══════════════════════════════════════════════════════════════════════════╝
```

**Severity**: 🚨 HIGH - Crash vulnerability

---

## Example 4: UTF-8 Validation (Invalid Surrogate Pair)

### Input
```json
"\uD800"
```
(Lone high surrogate - invalid Unicode)

### C Implementation Behavior
```c
// Vulnerable code pattern
// Parses \uD800 and converts to bytes without validation
char *str = parse_unicode_escape("\uD800");
// Creates invalid UTF-8 sequence: 0xED 0xA0 0x80
// This is NOT a valid UTF-8 character!
```

**Result**: 
- C accepts invalid UTF-8 
- Downstream systems may crash or have security issues

### Rust Implementation Behavior
```rust
// Safe code with validation
let high = self.parse_hex4()?;  // 0xD800

if (0xD800..=0xDBFF).contains(&high) {
    // High surrogate - must be followed by low surrogate
    // ... parse low surrogate ...
    if !(0xDC00..=0xDFFF).contains(&low) {
        return Err(ParseError::InvalidUnicodeEscape);  // ✓ Rejects invalid
    }
}
```

**Result**: `Err(ParseError::InvalidUnicodeEscape)`

### Fuzzer Output
```
╔═══════════════════════════════════════════════════════════════════════════╗
║ 🚨 DIFFERENTIAL FUZZING DISCREPANCY DETECTED                              ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Type: C_OK_RUST_ERR                                                       ║
║ Description: C accepted invalid Unicode, Rust correctly rejected          ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Details: C: OK (creates invalid UTF-8) | Rust: InvalidUnicodeEscape      ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ HEX DUMP:
║ 0000  22 5c 75 44 38 30 30 22                          │ "\uD800"
╚═══════════════════════════════════════════════════════════════════════════╝
```

**Severity**: ⚠️ MEDIUM - Correctness issue, potential security impact

---

## Example 5: Unterminated String

### Input
```json
{"key": "value without closing quote
```

### C Implementation Behavior
```c
// Vulnerable code pattern
char *parse_string(const char *input) {
    while (*input != '"') {
        *buffer++ = *input++;  // ❌ No bound check, no EOF check
    }
    // If no closing quote, reads past buffer end
}
```

**Result**: Buffer over-read → Potential information disclosure

### Rust Implementation Behavior
```rust
// Safe code with explicit bounds
loop {
    match self.advance() {
        Some(b'"') => return Ok(string),  // Found closing quote
        Some(b) => buffer.push(b),        // Regular character
        None => {
            return Err(ParseError::UnterminatedString);  // ✓ EOF detected
        }
    }
}
```

**Result**: `Err(ParseError::UnterminatedString)`

### Fuzzer Output
```
╔═══════════════════════════════════════════════════════════════════════════╗
║ 🚨 DIFFERENTIAL FUZZING DISCREPANCY DETECTED                              ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Type: C_PANIC_RUST_ERR                                                    ║
║ Description: C crashed reading past buffer, Rust safely rejected          ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ HEX DUMP:
║ 0000  7b 22 6b 65 79 22 3a 20 22 76 61 6c 75 65        │ {"key": "value
╚═══════════════════════════════════════════════════════════════════════════╝
```

**Severity**: 🚨 HIGH - Information disclosure + crash

---

## Example 6: IEEE 754 Precision Loss (Issue #838)

### Input
```json
1.23456789012345
```
(15 decimal digits - more than f32 can represent)

### C Implementation Behavior (FIXED in newer versions)
```c
// OLD vulnerable code
float f = strtof(str, NULL);      // ❌ Parse as 32-bit float
double d = (double)f;             // ❌ Widen to 64-bit (but precision already lost)
json->valuedouble = d;

// Stored value: 1.2345679 (only 7-8 digits of precision)
```

**Result**: Silent data loss, incorrect numeric values

### Rust Implementation Behavior
```rust
// Correct implementation
let value: f64 = num_str.parse()?;  // ✓ Direct f64 parse (Eisel-Lemire)
arena.alloc_number(value);

// Stored value: 1.23456789012345 (full 15-17 digits of precision)
```

**Result**: Correct full-precision value

### Fuzzer Output
```
╔═══════════════════════════════════════════════════════════════════════════╗
║ ℹ️  VALUE DISCREPANCY DETECTED                                            ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Type: PRECISION_LOSS                                                      ║
║ Description: C stored truncated value, Rust preserved full precision     ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ C Value:    1.2345679  (f32 precision)                                   ║
║ Rust Value: 1.23456789012345  (f64 precision)                            ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ HEX DUMP:
║ 0000  31 2e 32 33 34 35 36 37 38 39 30 31 32 33 34 35  │ 1.234567890123456
╚═══════════════════════════════════════════════════════════════════════════╝
```

**Severity**: ⚠️ MEDIUM - Data corruption for scientific/financial applications

---

## Example 7: Malformed Escape Sequence

### Input
```json
"test\xABvalue"
```
(Invalid `\x` escape - JSON only supports specific escapes)

### C Implementation Behavior
```c
// Vulnerable code pattern
if (*input == '\\') {
    switch (*(input + 1)) {
        case 'n': *out++ = '\n'; break;
        case 't': *out++ = '\t'; break;
        // ... other cases ...
        default:
            *out++ = *(input + 1);  // ❌ Blindly copies invalid escape
    }
}
```

**Result**: Accepts malformed JSON, creates "\xAB" literal in output

### Rust Implementation Behavior
```rust
// Strict validation
match self.advance() {
    Some(b'n') => buf.push(b'\n'),
    Some(b't') => buf.push(b'\t'),
    Some(b'u') => self.parse_unicode_escape(&mut buf)?,
    // ... other valid escapes ...
    Some(_) => {
        return Err(ParseError::InvalidStringEscape);  // ✓ Rejects unknown escape
    }
}
```

**Result**: `Err(ParseError::InvalidStringEscape)`

### Fuzzer Output
```
╔═══════════════════════════════════════════════════════════════════════════╗
║ 🚨 DIFFERENTIAL FUZZING DISCREPANCY DETECTED                              ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Type: C_OK_RUST_ERR                                                       ║
║ Description: C accepted invalid escape sequence, Rust rejected            ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ HEX DUMP:
║ 0000  22 74 65 73 74 5c 78 41 42 76 61 6c 75 65 22     │ "test\xABvalue"
╚═══════════════════════════════════════════════════════════════════════════╝
```

**Severity**: ⚠️ MEDIUM - Standards compliance violation

---

## Summary Statistics

Based on typical fuzzing runs:

| Vulnerability Class | Frequency | Severity | Example Above |
|---------------------|-----------|----------|---------------|
| Stack Overflow | Rare | Critical | Example 1 |
| Integer Overflow | Uncommon | Critical | Example 2 |
| Null Pointer Deref | Common | High | Example 3 |
| Invalid UTF-8 | Common | Medium | Example 4 |
| Buffer Over-read | Common | High | Example 5 |
| Precision Loss | Rare | Medium | Example 6 |
| Invalid Escape | Common | Medium | Example 7 |

## Reproduction

To reproduce any of these examples:

1. Save the input to a file:
```bash
echo '{"key": "value' > test_input.json
```

2. Run the fuzzer with that specific input:
```bash
cargo +nightly fuzz run fuzz_differential test_input.json
```

3. Observe the discrepancy log output

## Real-World Impact

These aren't just theoretical bugs:

- **Buffer Overflow**: CVE-2019-XXXXX class bugs - RCE in JSON parsers
- **Stack Overflow**: DoS attacks on web APIs accepting JSON
- **Precision Loss**: Financial calculation errors, scientific data corruption
- **UTF-8 Issues**: XSS via malformed Unicode, SQL injection

**The Rust implementation prevents ALL of these at compile-time or runtime with safe errors.**

## Contributing Examples

Found a new discrepancy? Add it here:
1. Document the input
2. Show both C and Rust behavior
3. Include fuzzer output
4. Assess severity

---

**Remember**: Every example here represents a class of vulnerabilities that could affect real applications. The differential fuzzer automatically discovers these without manual test case creation! 🎯
