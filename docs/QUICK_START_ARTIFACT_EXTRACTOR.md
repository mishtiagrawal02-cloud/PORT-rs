# 🚀 Quick Start: Artifact Extractor

## What This Is

An automated tool that runs differential fuzzing to catch C vulnerabilities (segfaults, buffer overflows, undefined behavior) and extracts proof-of-bug artifacts.

## One Command to Rule Them All

```bash
cd cjson-rs/fuzz && ./extract_crash_artifact.sh
```

## What Happens Next

1. Script validates prerequisites (Rust nightly, cargo-fuzz)
2. Generates 30+ malformed JSON crash triggers
3. Runs differential fuzzer (C vs Rust comparison)
4. Detects when C crashes but Rust safely handles the input
5. Extracts the crashing input to `crash_proof.json`
6. Generates detailed analysis report
7. Displays victory banner: **"CRASH SECURED: BUG CATCHER ARTIFACT GENERATED"**

## Expected Output

```
╔═══════════════════════════════════════════════════════════════════╗
║  🎯 CRASH SECURED: BUG CATCHER ARTIFACT GENERATED 🎯              ║
╚═══════════════════════════════════════════════════════════════════╝

Artifact Location:
  cjson-rs/crash_proof.json

Report Location:
  cjson-rs/crash_proof_REPORT.txt
```

## More Options

```bash
# Extended fuzzing (5 minutes)
./extract_crash_artifact.sh --duration 300

# Aggressive parallel fuzzing (10 minutes, 8 cores)
./extract_crash_artifact.sh --duration 600 --workers 8

# Show help
./extract_crash_artifact.sh --help
```

## Output Files

| File | Description |
|------|-------------|
| `cjson-rs/crash_proof.json` | The exact bytes that crashed C |
| `cjson-rs/crash_proof_REPORT.txt` | Detailed analysis and reproduction steps |
| `cjson-rs/fuzz/fuzzer_output.log` | Complete fuzzer execution log |

## Reproduce a Crash

```bash
cd cjson-rs
cargo +nightly fuzz run fuzz_differential crash_proof.json
```

## Documentation

| Document | Location | Purpose |
|----------|----------|---------|
| **Quick Reference** | `cjson-rs/fuzz/QUICK_REFERENCE_EXTRACTOR.md` | One-page cheat sheet |
| **Complete Guide** | `cjson-rs/fuzz/ARTIFACT_EXTRACTOR_GUIDE.md` | Full user manual |
| **Implementation** | `cjson-rs/fuzz/DELIVERABLE_COMPLETE.md` | Technical details |
| **Summary** | `ARTIFACT_EXTRACTOR_SUMMARY.md` | Executive overview |

## Prerequisites

The script will auto-install missing dependencies, but you can manually prepare:

```bash
# Install Rust nightly
rustup install nightly

# Install cargo-fuzz
cargo install cargo-fuzz

# Build C library
cd /Users/awantikamaheshwari/Desktop/PORT-rs
make
```

## Troubleshooting

**No crashes found?**
```bash
# Try longer duration
./extract_crash_artifact.sh --duration 600
```

**Need help?**
```bash
./extract_crash_artifact.sh --help
```

Or consult: `cjson-rs/fuzz/ARTIFACT_EXTRACTOR_GUIDE.md`

---

**Ready? Run:** `cd cjson-rs/fuzz && ./extract_crash_artifact.sh`
