# 🎯 Artifact Extractor - Complete Documentation Index

## 📚 Documentation Files

This directory contains the **Artifact Extractor** system - an automated tool for detecting C vulnerabilities through differential fuzzing and extracting proof-of-bug artifacts.

### Quick Navigation

| Document | Purpose | Read When... |
|----------|---------|--------------|
| **[QUICK_REFERENCE_EXTRACTOR.md](QUICK_REFERENCE_EXTRACTOR.md)** | One-page cheat sheet | You need a quick command reference |
| **[ARTIFACT_EXTRACTOR_GUIDE.md](ARTIFACT_EXTRACTOR_GUIDE.md)** | Complete user manual | You're using the tool for the first time |
| **[DELIVERABLE_COMPLETE.md](DELIVERABLE_COMPLETE.md)** | Implementation summary | You want to understand what was built |
| **[README.md](README.md)** | Differential fuzzing overview | You want to understand the fuzzing harness |

### Core Files

| File | Description |
|------|-------------|
| **[extract_crash_artifact.sh](extract_crash_artifact.sh)** | Main executable script |
| [fuzz_targets/fuzz_differential.rs](fuzz_targets/fuzz_differential.rs) | Differential fuzzing harness |
| [Cargo.toml](Cargo.toml) | Fuzzing configuration |

## 🚀 Quick Start (30 seconds)

```bash
# Navigate to this directory
cd /Users/awantikamaheshwari/Desktop/PORT-rs/cjson-rs/fuzz

# Run the artifact extractor
./extract_crash_artifact.sh

# Wait for completion, then check results
ls -lh ../crash_proof*
```

## 📖 Reading Guide

### For First-Time Users
1. Start with **[QUICK_REFERENCE_EXTRACTOR.md](QUICK_REFERENCE_EXTRACTOR.md)** (2 min read)
2. Run `./extract_crash_artifact.sh --help`
3. Execute `./extract_crash_artifact.sh` with default settings
4. If you encounter issues, consult **[ARTIFACT_EXTRACTOR_GUIDE.md](ARTIFACT_EXTRACTOR_GUIDE.md)**

### For Understanding the System
1. Read **[DELIVERABLE_COMPLETE.md](DELIVERABLE_COMPLETE.md)** for implementation details
2. Review **[README.md](README.md)** for fuzzing harness documentation
3. Examine the script source: `extract_crash_artifact.sh`

### For Debugging Issues
1. Check **[ARTIFACT_EXTRACTOR_GUIDE.md](ARTIFACT_EXTRACTOR_GUIDE.md)** → "Troubleshooting" section
2. Review `fuzzer_output.log` (generated after running)
3. Verify prerequisites: `rustup toolchain list | grep nightly` and `cargo fuzz --version`

## 🎓 What Each Document Covers

### QUICK_REFERENCE_EXTRACTOR.md
- One-line commands for common scenarios
- Output file locations
- Environment variable reference
- Quick troubleshooting tips
- CI/CD snippet

**Best for:** Experienced users who need a quick reminder

### ARTIFACT_EXTRACTOR_GUIDE.md
- Detailed usage instructions
- Complete command-line options
- Crash type explanations
- Reproduction methods
- Advanced usage patterns
- Performance metrics
- Security disclosure guidelines

**Best for:** Comprehensive understanding and advanced usage

### DELIVERABLE_COMPLETE.md
- What was delivered and why
- Technical implementation details
- Malformed corpus specifications
- File structure overview
- Success criteria validation
- Next steps and long-term usage

**Best for:** Understanding the complete implementation

### README.md (Differential Fuzzing)
- Overview of the fuzzing harness
- How differential fuzzing works
- Vulnerability detection patterns
- libFuzzer integration
- Corpus management
- Coverage analysis

**Best for:** Understanding the underlying fuzzing technology

## 🔧 System Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                 extract_crash_artifact.sh                   │
│                  (Automation Orchestrator)                  │
└─────────────────────────────────────────────────────────────┘
                              │
                              ├─> Check prerequisites
                              ├─> Generate malformed corpus
                              ├─> Execute cargo-fuzz ──┐
                              ├─> Monitor exit codes   │
                              ├─> Extract artifacts    │
                              └─> Generate reports     │
                                                       │
                              ┌────────────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│              cargo +nightly fuzz run                        │
│                  (libFuzzer Engine)                         │
└─────────────────────────────────────────────────────────────┘
                              │
                              ├─> Load corpus
                              ├─> Generate test cases
                              └─> Execute harness ──┐
                                                    │
                              ┌─────────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────┐
│           fuzz_targets/fuzz_differential.rs                 │
│              (Differential Comparator)                      │
└─────────────────────────────────────────────────────────────┘
                              │
                    ┌─────────┴─────────┐
                    ▼                   ▼
        ┌───────────────────┐   ┌──────────────────┐
        │  C Implementation │   │ Rust             │
        │  cJSON_Parse()    │   │ Implementation   │
        │  (Legacy, Unsafe) │   │ parse_json()     │
        │                   │   │ (Safe, Modern)   │
        └───────────────────┘   └──────────────────┘
                    │                   │
                    └─────────┬─────────┘
                              │
                    Compare Results
                              │
                              ▼
                  C Crash? Rust OK? ──> ARTIFACT! 🎯
```

## 📊 Workflow Diagram

```
START
  │
  ├─> [1] Pre-flight Checks
  │   ├─> Rust nightly installed?
  │   ├─> cargo-fuzz installed?
  │   └─> C library built?
  │
  ├─> [2] Generate Malformed Corpus
  │   ├─> Buffer overflows
  │   ├─> Null injection
  │   ├─> Deep nesting
  │   ├─> Integer overflows
  │   └─> 30+ patterns total
  │
  ├─> [3] Execute Fuzzer
  │   ├─> Run for N seconds
  │   ├─> Feed corpus to differential harness
  │   ├─> Compare C vs Rust results
  │   └─> Detect discrepancies
  │
  ├─> [4] Monitor for Crashes
  │   ├─> Check exit code
  │   ├─> Parse sanitizer output
  │   └─> Locate artifact files
  │
  ├─> [5] Extract Artifact
  │   ├─> Find crash-* files
  │   ├─> Copy to crash_proof.json
  │   └─> Generate report
  │
  └─> [6] Display Victory Banner
      └─> CRASH SECURED! 🎯
  
END
```

## 🎯 Expected Outcomes

### Success Case
```bash
$ ./extract_crash_artifact.sh

[... colorized output ...]

╔═══════════════════════════════════════════════════════════════════╗
║  🎯 CRASH SECURED: BUG CATCHER ARTIFACT GENERATED 🎯              ║
╚═══════════════════════════════════════════════════════════════════╝

Artifact Location:
  /Users/awantikamaheshwari/Desktop/PORT-rs/cjson-rs/crash_proof.json

Report Location:
  /Users/awantikamaheshwari/Desktop/PORT-rs/cjson-rs/crash_proof_REPORT.txt
```

### No Crash Case
```bash
$ ./extract_crash_artifact.sh

[... output ...]

⚠  No crashes detected during this fuzzing session
ℹ  Recommendations:
  - Increase FUZZ_DURATION (current: 60s)
  - Run: FUZZ_DURATION=300 ./extract_crash_artifact.sh
```

## 🛠️ Common Tasks

### Task: Run a Quick Test
```bash
./extract_crash_artifact.sh
```

### Task: Extended Fuzzing Session
```bash
./extract_crash_artifact.sh --duration 600 --workers 8
```

### Task: Reproduce a Found Crash
```bash
cargo +nightly fuzz run fuzz_differential ../crash_proof.json
```

### Task: Add Crash to Regression Tests
```bash
cp ../crash_proof.json ../tests/crash_regression_001.json
```

### Task: View Detailed Report
```bash
cat ../crash_proof_REPORT.txt
```

### Task: Examine Hex Dump
```bash
hexdump -C ../crash_proof.json
```

## 📞 Getting Help

### Documentation
- **Quick answers**: See [QUICK_REFERENCE_EXTRACTOR.md](QUICK_REFERENCE_EXTRACTOR.md)
- **Detailed guide**: See [ARTIFACT_EXTRACTOR_GUIDE.md](ARTIFACT_EXTRACTOR_GUIDE.md)
- **Troubleshooting**: See [ARTIFACT_EXTRACTOR_GUIDE.md](ARTIFACT_EXTRACTOR_GUIDE.md) → "Troubleshooting"

### Logs
- Check `fuzzer_output.log` for execution details
- Review `../crash_proof_REPORT.txt` for crash analysis

### Prerequisites
```bash
# Verify installation
rustup toolchain list | grep nightly
cargo fuzz --version

# Install if missing
rustup install nightly
cargo install cargo-fuzz
```

## 🎉 Success Indicators

You'll know the system is working when:

1. ✅ Script executes without errors
2. ✅ Corpus directory contains 30+ files
3. ✅ Fuzzer runs for specified duration
4. ✅ Crash is detected (exit code 77 or non-zero)
5. ✅ `crash_proof.json` is created
6. ✅ `crash_proof_REPORT.txt` is generated
7. ✅ Victory banner is displayed
8. ✅ Artifact can be reproduced

## 📝 License

MIT - Same as parent project

---

**Ready to hunt for bugs? Start with [QUICK_REFERENCE_EXTRACTOR.md](QUICK_REFERENCE_EXTRACTOR.md)!**
