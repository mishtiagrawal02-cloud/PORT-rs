# Differential Fuzzing Harness for cJSON-rs

## Overview

This directory contains a **differential fuzzing harness** using `cargo-fuzz` (libFuzzer) to compare the legacy C JSON parser (`cJSON_Parse`) against the new Safe Rust parser (`parse_json`).

The harness is specifically designed to catch **security vulnerabilities** where:
- The C implementation crashes, segfaults, or returns false positives
- The Rust implementation correctly and safely rejects the malformed input

## The Trap: Catching Vulnerabilities

The fuzzing harness implements several critical vulnerability detection patterns:

### 1. **C Panic/Crash vs Rust Safe Rejection** (`C_PANIC_RUST_ERR`)
```
Input: [malformed bytes]
C Result: CRASH/SEGFAULT/PANIC
Rust Result: Err("parse error...")
Status: ✅ VULNERABILITY CAUGHT - Rust safely handles what crashes C
```

This is the **primary security win**: when C exhibits undefined behavior (crashes, buffer overruns, null pointer dereferences), Rust's memory safety guarantees ensure safe rejection.

### 2. **C False Positive** (`C_OK_RUST_ERR`)
```
Input: [technically invalid JSON]
C Result: Successfully parsed (returns valid pointer)
Rust Result: Err("invalid JSON...")
Status: ⚠️ C may be too permissive, accepting malformed JSON
```

This catches cases where C's parser accepts invalid input that could lead to downstream vulnerabilities.

### 3. **C False Negative** (`C_NULL_RUST_OK`)
```
Input: [valid but unusual JSON]
C Result: NULL (parse failure)
Rust Result: Ok(parsed tree)
Status: ℹ️ C may be overly conservative
```

### 4. **Consistent Agreement** (No Log)
```
Both Accept: Both parsers agree the input is valid
Both Reject: Both parsers agree the input is invalid
Status: ✅ Consistent behavior
```

## Installation

### Prerequisites

1. Install Rust nightly (required for libFuzzer):
   ```bash
   rustup install nightly
   ```

2. Install cargo-fuzz:
   ```bash
   cargo install cargo-fuzz
   ```

### Building the C Library

The fuzzer needs to link against the original cJSON C library. Build it first:

```bash
cd /Users/awantikamaheshwari/Desktop/PORT-rs
make
```

## Running the Fuzzer

### Quick Start

```bash
cd /Users/awantikamaheshwari/Desktop/PORT-rs/cjson-rs

# Run with default settings (uses nightly Rust)
cargo +nightly fuzz run fuzz_differential
```

### Recommended Fuzzing Options

```bash
# Run with specific timeout per input (prevents infinite loops)
cargo +nightly fuzz run fuzz_differential -- -max_total_time=3600 -timeout=10

# Run with maximum CPU cores
cargo +nightly fuzz run fuzz_differential -- -workers=8 -jobs=8

# Run with custom corpus directory
cargo +nightly fuzz run fuzz_differential -- -corpus=fuzz/corpus/fuzz_differential

# Run with existing artifacts to reproduce bugs
cargo +nightly fuzz run fuzz_differential fuzz/artifacts/fuzz_differential/crash-*
```

### Continuous Fuzzing (Recommended)

For serious vulnerability hunting, run continuously:

```bash
# Run for 24 hours with all cores
cargo +nightly fuzz run fuzz_differential -- \
  -max_total_time=86400 \
  -workers=$(nproc) \
  -jobs=$(nproc) \
  -timeout=10 \
  -rss_limit_mb=2048
```

## Output Format

When a discrepancy is detected, the fuzzer outputs a formatted log:

```
╔═══════════════════════════════════════════════════════════════════════════╗
║ 🚨 DIFFERENTIAL FUZZING DISCREPANCY DETECTED                              ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Type: C_PANIC_RUST_ERR                                                    ║
║ Description: C implementation panicked, Rust safely rejected (GOOD)       ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Details: C Panic: ... | Rust Error: parse error at byte 42: ...          ║
╠═══════════════════════════════════════════════════════════════════════════╣
║ Input Size: 128 bytes
║
║ HEX DUMP (for reproduction):
║ 0000  7b 22 6b 65 79 22 3a 20 22 76 61 6c 75 65 22 7d  │ {"key": "value"}
║
║ BASE64 (for easy reproduction):
║ eyJrZXkiOiAidmFsdWUifQ==
║
║ RAW BYTES (Rust array literal):
║ &[0x7b, 0x22, 0x6b, 0x65, 0x79, 0x22, 0x3a, 0x20,
║   0x22, 0x76, 0x61, 0x6c, 0x75, 0x65, 0x22, 0x7d]
╚═══════════════════════════════════════════════════════════════════════════╝
```

## Reproducing Findings

### From Hex Dump
```rust
let input = &[0x7b, 0x22, 0x6b, 0x65, 0x79, 0x22, 0x3a, 0x20];
// Test with both parsers...
```

### From Base64
```bash
echo "eyJrZXkiOiAidmFsdWUifQ==" | base64 -d > test_case.json
```

### From Artifacts Directory
```bash
# Automatically saved crash inputs
ls -la fuzz/artifacts/fuzz_differential/

# Re-run a specific crash
cargo +nightly fuzz run fuzz_differential \
  fuzz/artifacts/fuzz_differential/crash-da39a3ee5e6b4b0d
```

## Corpus Management

The fuzzer automatically builds a corpus of interesting inputs:

```bash
# View corpus
ls -la fuzz/corpus/fuzz_differential/

# Minimize corpus (remove redundant inputs)
cargo +nightly fuzz cmin fuzz_differential

# Merge multiple corpus directories
cargo +nightly fuzz cmin fuzz_differential \
  fuzz/corpus/fuzz_differential \
  fuzz/corpus/external \
  fuzz/corpus/manual
```

## Integration with CI/CD

### GitHub Actions Example

```yaml
name: Differential Fuzzing

on:
  schedule:
    - cron: '0 0 * * *'  # Daily
  workflow_dispatch:

jobs:
  fuzz:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Install Rust nightly
        run: rustup install nightly
      
      - name: Install cargo-fuzz
        run: cargo install cargo-fuzz
      
      - name: Run fuzzer for 1 hour
        run: |
          cd cjson-rs
          cargo +nightly fuzz run fuzz_differential -- \
            -max_total_time=3600 \
            -timeout=10
      
      - name: Upload artifacts
        if: failure()
        uses: actions/upload-artifact@v3
        with:
          name: fuzz-artifacts
          path: cjson-rs/fuzz/artifacts/
```

## Advanced: Seed Corpus

To jump-start fuzzing with known interesting inputs:

```bash
mkdir -p fuzz/corpus/fuzz_differential

# Add seed files
echo '{"test": 123}' > fuzz/corpus/fuzz_differential/valid_simple.json
echo '[1, 2, 3]' > fuzz/corpus/fuzz_differential/valid_array.json
echo '{invalid' > fuzz/corpus/fuzz_differential/malformed.json
echo '"\u0000"' > fuzz/corpus/fuzz_differential/null_byte.json
echo '999999999999999999999999999999' > fuzz/corpus/fuzz_differential/huge_number.json

# Add test inputs from the main cJSON test suite
cp ../tests/inputs/test* fuzz/corpus/fuzz_differential/
```

## Metrics and Coverage

### Generate Coverage Report

```bash
# Run with coverage instrumentation
cargo +nightly fuzz coverage fuzz_differential

# Generate HTML report
cargo +nightly cov -- show \
  target/x86_64-unknown-linux-gnu/coverage/x86_64-unknown-linux-gnu/release/fuzz_differential \
  --format=html \
  -instr-profile=fuzz/coverage/fuzz_differential/coverage.profdata \
  > coverage.html
```

### Monitor Statistics

```bash
# Real-time statistics
cargo +nightly fuzz run fuzz_differential -- -print_final_stats=1

# Key metrics to watch:
# - exec/s: executions per second (higher is better)
# - cov: code coverage (unique code paths discovered)
# - corp: corpus size (unique inputs retained)
```

## Known Limitations

1. **Signal Handling**: Some segfaults may not be caught by `panic::catch_unwind` - they will terminate the fuzzer but save artifacts
2. **Linking**: Requires proper linking against the C cJSON library (ensure `LD_LIBRARY_PATH` is set)
3. **Performance**: Differential fuzzing is slower than single-implementation fuzzing due to running both parsers

## Troubleshooting

### "undefined reference to cJSON_Parse"
```bash
# Ensure C library is built
cd /Users/awantikamaheshwari/Desktop/PORT-rs
make

# Set library path
export LD_LIBRARY_PATH=/Users/awantikamaheshwari/Desktop/PORT-rs:$LD_LIBRARY_PATH
```

### Fuzzer runs but finds nothing
- Increase timeout: `-timeout=30`
- Add seed corpus (see above)
- Check coverage to ensure code is being exercised

### Out of memory
```bash
# Limit RSS
cargo +nightly fuzz run fuzz_differential -- -rss_limit_mb=2048
```

## Security Disclosure

If you discover critical vulnerabilities using this fuzzer, please report them responsibly:

1. **Document the finding**: Save the exact input that triggers the vulnerability
2. **Verify exploitability**: Confirm the C parser exhibits dangerous behavior
3. **Report upstream**: File a security advisory to the cJSON maintainers
4. **Credit the tooling**: Mention this differential fuzzing harness in your disclosure

## References

- [libFuzzer Documentation](https://llvm.org/docs/LibFuzzer.html)
- [cargo-fuzz Book](https://rust-fuzz.github.io/book/cargo-fuzz.html)
- [cJSON Security Issues](https://github.com/DaveGamble/cJSON/security)
- [Differential Fuzzing Paper](https://arxiv.org/abs/1812.00140)

## License

MIT - Same as parent project
