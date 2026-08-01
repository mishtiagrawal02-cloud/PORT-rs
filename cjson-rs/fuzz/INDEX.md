# Differential Fuzzing Harness - File Index

## 📁 Directory Structure

```
fuzz/
├── README.md                          ← Start here! Complete guide
├── QUICK_START.md                     ← 5-minute quick start
├── DIFFERENTIAL_FUZZING_SUMMARY.md    ← Technical architecture
├── VULNERABILITY_CLASSES.md           ← Catalog of detectable bugs
├── INDEX.md                           ← This file
│
├── Cargo.toml                         ← Fuzzing workspace config
├── .gitignore                         ← Ignore build artifacts
├── run_fuzzer.sh                      ← Automation script (executable)
│
├── fuzz_targets/
│   ├── fuzz_differential.rs           ← Main fuzzing harness (THE CORE)
│   └── test_harness.rs                ← Standalone test/demo
│
├── corpus/                            ← Generated: Interesting inputs
│   └── fuzz_differential/
│       ├── seed1.json                 ← (Created by run_fuzzer.sh)
│       ├── seed2.json
│       └── ...
│
└── artifacts/                         ← Generated: Crashes found
    └── fuzz_differential/
        ├── crash-abc123               ← (Created when bugs found)
        └── ...
```

## 📚 Documentation Guide

### For First-Time Users
1. **[QUICK_START.md](QUICK_START.md)** - Get fuzzing in 5 minutes
2. **[README.md](README.md)** - Complete user guide and reference

### For Security Researchers
1. **[VULNERABILITY_CLASSES.md](VULNERABILITY_CLASSES.md)** - What we detect and why
2. **[DIFFERENTIAL_FUZZING_SUMMARY.md](DIFFERENTIAL_FUZZING_SUMMARY.md)** - Technical deep dive

### For Contributors
1. **[fuzz_targets/fuzz_differential.rs](fuzz_targets/fuzz_differential.rs)** - Source code with detailed comments
2. **[DIFFERENTIAL_FUZZING_SUMMARY.md](DIFFERENTIAL_FUZZING_SUMMARY.md)** - Architecture section

## 🎯 Quick Navigation

### I want to...

#### Run the fuzzer
```bash
./run_fuzzer.sh run 300
```
→ See [QUICK_START.md](QUICK_START.md)

#### Understand what it detects
→ Read [VULNERABILITY_CLASSES.md](VULNERABILITY_CLASSES.md)

#### Reproduce a crash
```bash
cargo +nightly fuzz run fuzz_differential artifacts/fuzz_differential/crash-<id>
```
→ See [README.md#reproducing-findings](README.md)

#### Customize the harness
→ Edit [fuzz_targets/fuzz_differential.rs](fuzz_targets/fuzz_differential.rs)
→ Read [DIFFERENTIAL_FUZZING_SUMMARY.md#architecture](DIFFERENTIAL_FUZZING_SUMMARY.md)

#### Integrate with CI/CD
→ See [README.md#integration-with-cicd](README.md)

#### Report a vulnerability
→ See [DIFFERENTIAL_FUZZING_SUMMARY.md#security-disclosure-process](DIFFERENTIAL_FUZZING_SUMMARY.md)

## 🔑 Key Concepts

### Differential Fuzzing
**What**: Compare two implementations (C vs Rust) with the same input
**Why**: Detect bugs where C crashes but Rust safely rejects
**How**: libFuzzer generates inputs → both parsers → compare results

### Fuzzing Harness
**What**: The code that accepts fuzz input and tests the target
**Where**: [fuzz_targets/fuzz_differential.rs](fuzz_targets/fuzz_differential.rs)
**Does**: 
- Runs C parser (potentially unsafe)
- Runs Rust parser (memory-safe)
- Catches crashes/panics
- Logs discrepancies

### Corpus
**What**: Collection of interesting test inputs
**Where**: `corpus/fuzz_differential/`
**Purpose**: Seed the fuzzer and track discovered inputs
**Management**: Auto-managed by libFuzzer, can be minimized

### Artifacts
**What**: Crashes and failures found by the fuzzer
**Where**: `artifacts/fuzz_differential/`
**Purpose**: Reproduce and analyze bugs
**Action**: Investigate each artifact → file security reports

## 🚨 Critical Files

### Must Read
- **[fuzz_differential.rs](fuzz_targets/fuzz_differential.rs)** - The actual fuzzing logic
- **[README.md](README.md)** - How to use the fuzzer

### Must Run
- **[run_fuzzer.sh](run_fuzzer.sh)** - Convenient automation script

### Must Understand
- **[VULNERABILITY_CLASSES.md](VULNERABILITY_CLASSES.md)** - What we're looking for

## 📊 Metrics & Outputs

### During Fuzzing
```
#12345: cov: 234 ft: 567 corp: 89 exec/s: 2345
        │     │     │     │        └─ Executions per second
        │     │     │     └────────── Corpus size (unique inputs)
        │     │     └──────────────── Features (instrumentation points)
        │     └────────────────────── Coverage (unique code paths)
        └──────────────────────────── Iteration number
```

### After Finding Bug
```
╔══════════════════════════════════════════════════╗
║ 🚨 DIFFERENTIAL FUZZING DISCREPANCY DETECTED     ║
╠══════════════════════════════════════════════════╣
║ Type: C_PANIC_RUST_ERR                           ║
║ HEX DUMP: ...                                    ║
║ BASE64: ...                                      ║
╚══════════════════════════════════════════════════╝
```
→ Full details in [fuzz_differential.rs:log_discrepancy()](fuzz_targets/fuzz_differential.rs)

## 🛠️ Build Artifacts

### Generated During Build
```
target/
└── x86_64-unknown-linux-gnu/
    └── release/
        └── fuzz_differential         ← Compiled fuzzer binary
```

### Generated During Fuzzing
```
corpus/fuzz_differential/             ← Discovered interesting inputs
artifacts/fuzz_differential/          ← Crashes and failures
coverage/fuzz_differential/           ← (if running with --coverage)
```

## 🔗 External Links

- [cargo-fuzz Documentation](https://rust-fuzz.github.io/book/cargo-fuzz.html)
- [libFuzzer Options](https://llvm.org/docs/LibFuzzer.html#options)
- [cJSON Repository](https://github.com/DaveGamble/cJSON)
- [Rust Fuzzing Authority](https://github.com/rust-fuzz)

## 📝 Version History

- **v1.0** - Initial implementation
  - Differential C vs Rust fuzzing
  - Comprehensive logging
  - Automation scripts
  - Full documentation

## 🤝 Contributing

To improve the fuzzing harness:
1. Edit [fuzz_targets/fuzz_differential.rs](fuzz_targets/fuzz_differential.rs)
2. Test: `cargo +nightly fuzz run fuzz_differential -- -runs=1000`
3. Update relevant documentation
4. Submit PR

## 📞 Support

- **Questions**: See [QUICK_START.md](QUICK_START.md) troubleshooting section
- **Bugs**: File an issue on GitHub
- **Security**: See [DIFFERENTIAL_FUZZING_SUMMARY.md#security-disclosure-process](DIFFERENTIAL_FUZZING_SUMMARY.md)

---

**Remember**: The goal is to find vulnerabilities in the C implementation that the Rust implementation prevents. Every discrepancy is an opportunity to improve security! 🛡️
