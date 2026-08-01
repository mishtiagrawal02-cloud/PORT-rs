# Differential Fuzzing Harness: Technical Summary

## Executive Summary

This directory contains a **production-ready differential fuzzing harness** using cargo-fuzz (libFuzzer) to systematically compare the legacy C implementation of cJSON against the new memory-safe Rust implementation.

**Goal:** Detect security vulnerabilities where the C implementation exhibits undefined behavior (crashes, buffer overflows, use-after-free) while the Rust implementation safely rejects the same input.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    libFuzzer Engine                         │
│                  (Generates test inputs)                    │
└────────────────────┬────────────────────────────────────────┘
                     │ Arbitrary byte sequence
                     ▼
┌─────────────────────────────────────────────────────────────┐
│              Differential Fuzzing Harness                   │
│                (fuzz_differential.rs)                       │
└────┬─────────────────────────────────────────────┬──────────┘
     │                                             │
     ▼                                             ▼
┌──────────────────────┐              ┌────────────────────────┐
│  C Implementation    │              │  Rust Implementation   │
│   cJSON_Parse()      │              │   parse_json()         │
│                      │              │                        │
│ • Raw pointers       │              │ • Ownership system     │
│ • Manual malloc/free │              │ • Bounds checking      │
│ • No bounds checks   │              │ • Type safety          │
└──────────────────────┘              └────────────────────────┘
     │                                             │
     └─────────────┬───────────────────────────────┘
                   ▼
     ┌─────────────────────────────────┐
     │    Discrepancy Detection        │
     │                                 │
     │ If C crashes && Rust rejects    │
     │    → VULNERABILITY DETECTED     │
     │    → LOG WITH FULL INPUT        │
     └─────────────────────────────────┘
```

## Core Components

### 1. Fuzzing Target (`fuzz_targets/fuzz_differential.rs`)

The main harness that:
- Accepts arbitrary byte sequences from libFuzzer
- Feeds them to both C and Rust parsers
- Catches panics/crashes in the C implementation
- Compares results and logs discrepancies
- Provides structured output with hex dumps for reproduction

**Key Logic:**
```rust
fuzz_target!(|data: &[u8]| {
    // Test Rust parser (safe)
    let rust_result = parse_json(data, &mut arena);
    
    // Test C parser (potentially unsafe) with panic catching
    let c_result = panic::catch_unwind(|| unsafe {
        cJSON_Parse(data.as_ptr() as *const c_char)
    });
    
    // Detect and log discrepancies
    match (c_result, rust_result) {
        (Err(_panic), Err(_rust_err)) => {
            // 🚨 CRITICAL: C crashed, Rust safely rejected
            log_discrepancy(...);
        }
        // ... other cases
    }
});
```

### 2. Build Configuration (`Cargo.toml`)

Configures the fuzzing workspace:
- Links against libfuzzer-sys (LLVM's libFuzzer)
- Links against the parent cjson-rs library
- Isolates fuzzing dependencies from main crate

### 3. Automation Script (`run_fuzzer.sh`)

Convenience script that:
- Checks prerequisites (Rust nightly, cargo-fuzz)
- Sets up seed corpus with diverse inputs
- Runs the fuzzer with recommended settings
- Reports findings and statistics

### 4. Documentation

- **README.md**: Complete user guide
- **QUICK_START.md**: 5-minute setup guide
- **VULNERABILITY_CLASSES.md**: Detailed catalog of detectable vulnerabilities
- **This file**: Technical architecture overview

## Vulnerability Detection Matrix

| Vulnerability Class | C Behavior | Rust Behavior | Detection Method |
|---------------------|------------|---------------|------------------|
| Buffer Overflow | Segfault/Corruption | `Err(ParseError)` | `C_PANIC_RUST_ERR` |
| Null Pointer Deref | Segfault | Safe `Option<T>` | `C_PANIC_RUST_ERR` |
| Use-After-Free | Undefined Behavior | Compile-time prevention | N/A (compiler) |
| Integer Overflow | Wrap-around → Overflow | Checked arithmetic | `C_PANIC_RUST_ERR` |
| Stack Overflow | Segfault | Depth limit error | `C_PANIC_RUST_ERR` |
| Double Free | Heap corruption | Compile-time prevention | N/A (compiler) |
| Unicode Errors | Invalid UTF-8 | Validation error | `C_OK_RUST_ERR` |
| Type Confusion | Wrong value | Type-safe enum | `C_OK_RUST_ERR` |
| Precision Loss | Truncated f64→f32 | Direct f64 parse | Value comparison |

## Logging Format

When a discrepancy is detected, the harness outputs:

```
╔═══════════════════════════════════════════════════════════════════════════╗
║ 🚨 DIFFERENTIAL FUZZING DISCREPANCY DETECTED                              ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Type: C_PANIC_RUST_ERR
║ Description: C implementation panicked, Rust safely rejected (GOOD)
╠═══════════════════════════════════════════════════════════════════════════╣
║ Details: C Panic: ... | Rust Error: parse error at byte 42: ...
╠═══════════════════════════════════════════════════════════════════════════╣
║ Input Size: N bytes
║
║ HEX DUMP (for reproduction):
║ 0000  xx xx xx xx ...  │ ASCII representation
║ 0010  xx xx xx xx ...  │ ...
║
║ BASE64 (for easy reproduction):
║ base64encodedstring...
║
║ RAW BYTES (Rust array literal):
║ &[0xXX, 0xXX, ...]
╚═══════════════════════════════════════════════════════════════════════════╝
```

**Three reproduction formats:**
1. **Hex dump**: For visual inspection
2. **Base64**: For pasting into test cases
3. **Rust array**: For unit tests

## Discrepancy Types

### Critical: `C_PANIC_RUST_ERR`
- **What**: C crashed/panicked, Rust safely rejected
- **Severity**: 🚨 HIGH - Memory safety vulnerability
- **Action**: File security advisory, investigate exploit potential

### Important: `C_PANIC_RUST_OK`
- **What**: C crashed/panicked, Rust successfully parsed
- **Severity**: ⚠️ MEDIUM-HIGH - C is too fragile
- **Action**: Report to cJSON maintainers

### Notable: `C_OK_RUST_ERR`
- **What**: C accepted (false positive), Rust rejected
- **Severity**: ⚠️ MEDIUM - C is too permissive
- **Action**: Verify if input is actually invalid per RFC 8259

### Info: `C_NULL_RUST_OK`
- **What**: C returned NULL (failure), Rust succeeded
- **Severity**: ℹ️ LOW - C is overly conservative
- **Action**: Document compatibility difference

## Fuzzing Strategy

### Phase 1: Quick Validation (5 minutes)
```bash
./run_fuzzer.sh run 300
```
- Sanity check that fuzzer works
- Catch obvious crashes

### Phase 2: Standard Session (1 hour)
```bash
./run_fuzzer.sh run 3600
```
- Discover common edge cases
- Build initial corpus

### Phase 3: Deep Fuzzing (8-24 hours)
```bash
nohup ./run_fuzzer.sh run 86400 &
```
- Explore deep code paths
- Find rare edge cases
- Maximize coverage

### Phase 4: Continuous (CI/CD)
```yaml
# Run daily in CI pipeline
schedule:
  - cron: '0 0 * * *'
```
- Regression testing
- Catch new vulnerabilities

## Performance Characteristics

### Expected Throughput
- **Fast path**: 5,000-10,000 exec/s (small valid JSON)
- **Slow path**: 500-1,000 exec/s (complex nested structures)
- **Optimal**: 2,000-3,000 exec/s (mixed corpus)

### Coverage Goals
- **Minimum**: 80% code coverage (basic paths)
- **Good**: 90% code coverage (most branches)
- **Excellent**: 95%+ code coverage (edge cases)

### Corpus Growth
- **Initial**: 20-30 seed inputs
- **After 1 hour**: 100-200 interesting inputs
- **After 24 hours**: 500-1000 inputs (diminishing returns)

## Integration Points

### Build System
The fuzzer integrates with:
- **Cargo workspace**: Separate fuzz crate
- **C library**: Links via FFI to original cJSON
- **CI/CD**: GitHub Actions, GitLab CI compatible

### Artifact Storage
- **corpus/**: Interesting inputs discovered (keep in version control)
- **artifacts/**: Crashes and failures (investigate and report)
- **coverage/**: Code coverage reports (track progress)

### Linking Requirements
Must link against:
1. `libfuzzer-sys` (LLVM libFuzzer)
2. `cjson-rs` (Rust implementation)
3. `libcjson.so/.a` (C implementation for comparison)

## Limitations

### 1. Signal Handling
`panic::catch_unwind` catches Rust panics but not all signals:
- ✅ Catches: Out-of-bounds, assertion failures
- ❌ Misses: SIGSEGV (caught by libFuzzer itself)
- **Workaround**: libFuzzer saves crash inputs automatically

### 2. Non-Determinism
Some crashes may be non-deterministic:
- Timing-dependent bugs
- Use-after-free (depends on allocator state)
- **Workaround**: Re-run crashes multiple times

### 3. False Positives
Rare cases where discrepancy is benign:
- Different error messages for same invalid input
- Different handling of undefined JSON edge cases
- **Mitigation**: Manual triage of findings

### 4. Performance
Differential fuzzing is ~2x slower than single-parser fuzzing:
- Must parse with both implementations
- Extra comparison logic
- **Acceptable**: Security benefits outweigh performance cost

## Security Disclosure Process

If critical vulnerabilities are found:

1. **Document**:
   - Save exact input from artifacts/
   - Verify reproducibility
   - Analyze root cause

2. **Verify Exploitability**:
   - Can attacker control input?
   - Can attacker trigger code path?
   - What's the impact? (crash, RCE, info leak?)

3. **Report Upstream**:
   - File private security advisory to cJSON maintainers
   - Include reproduction steps
   - Propose fix if possible

4. **Credit**:
   - Mention this fuzzing harness
   - Link to Port Mortem 2026 Hackathon project
   - Acknowledge Rust memory safety benefits

## Future Enhancements

### Short-term
- [ ] Add structure-aware fuzzing (generate valid JSON, then mutate)
- [ ] Add performance regression detection
- [ ] Integrate with OSS-Fuzz for continuous fuzzing

### Medium-term
- [ ] Compare serialization (cJSON_Print vs Rust serializer)
- [ ] Add JSON Patch/Pointer operations fuzzing
- [ ] Add fuzzing for cJSON_Utils functions

### Long-term
- [ ] Property-based testing integration
- [ ] Formal verification comparison
- [ ] Cross-language fuzzing (Python, JavaScript parsers)

## References

- [libFuzzer Documentation](https://llvm.org/docs/LibFuzzer.html)
- [cargo-fuzz Book](https://rust-fuzz.github.io/book/)
- [RFC 8259: JSON Specification](https://datatracker.ietf.org/doc/html/rfc8259)
- [cJSON Repository](https://github.com/DaveGamble/cJSON)
- [Rust Memory Safety](https://doc.rust-lang.org/nomicon/)

## License

MIT - Same as parent project

## Authors

Port Mortem 2026 Hackathon Team
- Differential fuzzing harness implementation
- Security-focused testing infrastructure
- Comprehensive vulnerability documentation

## Acknowledgments

- LLVM Project (libFuzzer)
- Rust fuzzing community (cargo-fuzz)
- cJSON maintainers (original C implementation)
- Security researchers who discovered prior cJSON vulnerabilities
