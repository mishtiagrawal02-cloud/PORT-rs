# CVE-2023-50471 Vulnerability Corpus Generator

## ⚠️ SECURITY WARNING

**This tool generates INTENTIONALLY MALICIOUS payloads designed to exploit known vulnerabilities in the DaveGamble/cJSON C library.**

- **CVE-2023-50471**: Heap corruption via malformed arrays
- **Stack Exhaustion**: Deep recursion without proper depth limits
- **Buffer Overflows**: Extremely large structures
- **Parsing Errors**: Missing commas, unclosed structures

**DO NOT use these payloads against production systems or any system you do not have explicit authorization to test.**

---

## Purpose

This corpus generator creates malicious JSON payloads specifically designed to:

1. **Trigger CVE-2023-50471** - Heap corruption in cJSON's array parsing
2. **Exhaust the call stack** - Deep recursion causing stack overflow
3. **Cause buffer overflows** - Extremely large arrays and strings
4. **Exploit parsing logic** - Missing commas, unclosed brackets
5. **Test edge cases** - Null bytes, control characters, invalid Unicode

The generated corpus is used with cargo-fuzz to validate that the Rust implementation safely rejects all these attacks while the C implementation crashes.

---

## Vulnerability Details

### CVE-2023-50471: Heap Corruption

**Description**: The cJSON library has a vulnerability in its array parsing logic where deeply nested or malformed arrays can cause heap metadata corruption.

**Root Cause**: 
- Improper tracking of heap allocations during recursive parsing
- Missing bounds checks on array depth
- Incorrect handling of unclosed array structures

**Exploitation**:
```json
[[[[[[[[[[...deep nesting...[1
```
(Note: Missing closing brackets)

**Impact**: 
- Heap corruption → Memory safety violation
- Potential for arbitrary code execution
- Denial of service (crash)

### Stack Exhaustion

**Description**: Deep recursion without proper depth limiting causes stack overflow.

**Exploitation**:
```json
[[[[[[[[[[[...10,000 levels deep...[1]...]]]]]]]]]
```

**Impact**:
- Stack overflow → Segmentation fault
- Denial of service

---

## Generated Payload Categories

### 1. Heap Corruption Payloads (`cve_2023_50471_heap_*.json`)

- **Deeply nested arrays** (100-10,000 levels)
- **Unclosed arrays** (deliberate memory leak)
- **Alternating open/close** (complex heap layout)
- **Mixed arrays/objects** (fragmented allocations)

**Example**:
```json
[[[[[[[[[[[...1000 levels...
```
(Deliberately missing closing brackets)

### 2. Missing Comma Payloads (`cve_2023_50471_comma_*.json`)

- **Simple arrays** without commas: `[1 2 3 4 5]`
- **Nested arrays** missing separators: `[[1 2] [3 4]]`
- **Large arrays** with randomly missing commas
- **Mixed types** without proper separation

**Example**:
```json
[1 2 3 4 5]
```
(Missing commas between elements)

### 3. Buffer Overflow Payloads (`buffer_overflow_*.json`)

- **Extremely long arrays** (1M+ elements)
- **Very long strings** (100KB+ in single string)
- **Huge key names** (50KB+ object keys)

**Example**:
```json
["AAAAA...100,000 A's...AAAAA"]
```

### 4. Stack Exhaustion Payloads (`stack_exhaustion_*.json`)

- **Pure array nesting** (50,000 levels deep)
- **Pure object nesting** (5,000 levels deep)
- **Alternating structures** (arrays↔objects)
- **Values at each level** (maximum memory pressure)

**Example**:
```json
[[[[[[[...50,000 levels...[1]...50,000 closes...]]]]]]]
```

### 5. Edge Case Payloads (`edge_cases_*.json`)

- **Empty/minimal**: `[`, `]`, `[[`, etc.
- **Mismatched brackets**: `[[[]]`, `[{]}`, `{[}]`
- **Null bytes**: `["\x00"]`
- **Control characters**: `[\x00-\x1F]`
- **Invalid Unicode**: `["\uD800"]` (lone surrogate)
- **Extreme numbers**: `[1e308]`, `[1e-324]`

### 6. Fuzzing-Optimized Seeds (`fuzz_seeds_*.json`)

- **Random deep nesting** with varying depths
- **Random comma omissions**
- **Mixed structural errors**
- Optimized for fuzzer mutation efficiency

---

## Usage

### Building

```bash
cd corpus_generator
cargo build --release
```

### Running

```bash
# Generate corpus in default location
cargo run --release

# View generated payloads
ls -lh ../corpus/fuzz_differential/
```

### Output

The generator creates files in `../corpus/fuzz_differential/`:

```
cve_2023_50471_heap_0000.json
cve_2023_50471_heap_0001.json
...
cve_2023_50471_comma_0000.json
...
buffer_overflow_0000.json
...
stack_exhaustion_0000.json
...
edge_cases_0000.json
...
fuzz_seeds_0000.json
...
```

**Total**: ~200+ malicious payloads

---

## Integration with Fuzzing

### Step 1: Generate Corpus

```bash
cd fuzz/corpus_generator
cargo run --release
```

### Step 2: Run Differential Fuzzing

```bash
cd ../
cargo +nightly fuzz run fuzz_differential
```

### Step 3: Monitor for Crashes

The fuzzer will:
1. Load the malicious corpus
2. Feed each payload to both C and Rust parsers
3. Detect when C crashes but Rust safely rejects
4. Log discrepancies with full payload details

**Expected Results**:
- C parser: **CRASHES** on heap corruption payloads
- Rust parser: **Safe rejection** with `Err(ParseError)`
- Fuzzer: **Logs discrepancy** with payload details

---

## Payload Statistics

| Category | Count | Max Depth | Max Size | Target Vulnerability |
|----------|-------|-----------|----------|---------------------|
| Heap Corruption | ~15 | 10,000 | ~20 KB | CVE-2023-50471 |
| Missing Comma | ~25 | N/A | ~10 KB | Parsing logic |
| Buffer Overflow | ~10 | N/A | ~1 MB | Buffer bounds |
| Stack Exhaustion | ~15 | 50,000 | ~100 KB | Stack limits |
| Edge Cases | ~50 | Varies | <1 KB | Various |
| Fuzz Seeds | ~150 | Varies | Varies | General fuzzing |

**Total**: ~265 payloads

---

## Technical Details

### Heap Corruption Mechanism (CVE-2023-50471)

When cJSON parses deeply nested arrays without proper tracking:

1. **Allocation Phase**:
   ```c
   for (depth = 0; depth < 1000; depth++) {
       item = cJSON_New_Item();  // malloc()
       item->child = parse_value();  // Recursive
   }
   ```

2. **Missing Cleanup on Error**:
   ```c
   // If parsing fails mid-way, allocated nodes are leaked
   // Heap metadata becomes corrupted
   ```

3. **Unclosed Structures**:
   - Parser expects closing `]`
   - Never receives it
   - Loses track of allocations
   - Heap corruption ensues

### Stack Exhaustion Mechanism

```c
cJSON *parse_array(const char *input) {
    if (*input == '[') {
        return parse_array(input + 1);  // ❌ No depth check
    }
}

// With 10,000 nesting levels:
// Stack frame size × 10,000 = STACK OVERFLOW
```

### Why Rust is Safe

```rust
fn parse_array(&mut self, arena: &mut Arena) -> Result<NodeId, ParseError> {
    self.enter_container()?;  // ✓ Checks depth < MAX_NESTING_DEPTH
    
    if self.depth > MAX_NESTING_DEPTH {
        return Err(ParseError::DepthLimitExceeded);
    }
    // ... parsing logic
}
```

---

## Customization

### Adding New Payload Patterns

Edit `src/main.rs` and add to the appropriate generator:

```rust
impl CVE_2023_50471_Generator {
    fn generate_my_pattern() -> Vec<Vec<u8>> {
        let mut payloads = Vec::new();
        
        // Your malicious pattern here
        let payload = b"[[[...".to_vec();
        payloads.push(payload);
        
        payloads
    }
}
```

Then call it in `main()`:

```rust
let my_payloads = CVE_2023_50471_Generator::generate_my_pattern();
save_payloads(&my_payloads, output_dir, "my_pattern", &mut total_payloads);
```

### Adjusting Payload Sizes

Modify the loop ranges in each generator:

```rust
// Increase max depth for stack exhaustion
for depth in [500, 1000, 2000, 5000, 10000, 100000] {  // Added 100000
    // ...
}

// Increase buffer sizes
for size in [1000, 10000, 100000, 10000000] {  // Added 10M
    // ...
}
```

---

## Testing the Generator

```bash
# Run unit tests
cargo test

# Verify payload generation
cargo run --release
ls -lh ../corpus/fuzz_differential/

# Count generated payloads
ls ../corpus/fuzz_differential/ | wc -l

# Inspect a payload
cat ../corpus/fuzz_differential/cve_2023_50471_heap_0000.json
hexdump -C ../corpus/fuzz_differential/cve_2023_50471_heap_0000.json | head
```

---

## Security Research

### Verifying CVE-2023-50471

1. Generate corpus: `cargo run --release`
2. Test against C library:
   ```bash
   # Assuming cJSON is built
   for payload in ../corpus/fuzz_differential/cve_2023_50471_*.json; do
       echo "Testing: $payload"
       ./test_cjson < "$payload" || echo "CRASH!"
   done
   ```
3. Many payloads should cause crashes

### Verifying Rust Safety

1. Run fuzzer: `cargo +nightly fuzz run fuzz_differential`
2. Check for discrepancies where:
   - C crashes (`C_PANIC_RUST_ERR`)
   - Rust safely rejects
3. Review logged payloads

---

## References

- **CVE-2023-50471**: https://cve.mitre.org/cgi-bin/cvename.cgi?name=CVE-2023-50471
- **cJSON Repository**: https://github.com/DaveGamble/cJSON
- **libFuzzer**: https://llvm.org/docs/LibFuzzer.html
- **cargo-fuzz**: https://rust-fuzz.github.io/book/cargo-fuzz.html

---

## Ethical Considerations

This tool is provided for:
- ✅ Security research
- ✅ Vulnerability validation
- ✅ Testing defensive implementations
- ✅ Educational purposes

**NOT for**:
- ❌ Attacking production systems
- ❌ Unauthorized penetration testing
- ❌ Malicious exploitation

**Always obtain proper authorization before security testing.**

---

## License

MIT - Same as parent project

---

## Contributing

To add new vulnerability patterns:
1. Research the vulnerability
2. Implement a generator function
3. Add unit tests
4. Document the payload pattern
5. Submit a PR

---

**Remember**: These payloads are weapons. Use them responsibly. 🛡️
