# 🚀 Artifact Extractor - Quick Reference Card

## One-Line Commands

```bash
# Quick test (1 minute)
./extract_crash_artifact.sh

# Thorough test (5 minutes)
./extract_crash_artifact.sh --duration 300

# Aggressive (10 min, 8 workers)
./extract_crash_artifact.sh --duration 600 --workers 8

# Help
./extract_crash_artifact.sh --help
```

## Output Files

| File | Description |
|------|-------------|
| `../crash_proof.json` | The crashing input (your proof!) |
| `../crash_proof_REPORT.txt` | Full analysis report |
| `fuzzer_output.log` | Complete fuzzer log |

## Reproduce a Crash

```bash
cargo +nightly fuzz run fuzz_differential ../crash_proof.json
```

## Environment Variables

```bash
export FUZZ_DURATION=300    # 5 minutes
export FUZZ_TIMEOUT=10      # 10s per input
export FUZZ_WORKERS=4       # 4 workers

./extract_crash_artifact.sh
```

## What Gets Detected

| Type | Meaning |
|------|---------|
| **C_PANIC_RUST_ERR** | 🎯 C crashes, Rust rejects (GOAL!) |
| **C_OK_RUST_ERR** | ⚠️ C accepts invalid JSON |
| **C_NULL_RUST_OK** | ℹ️ C too conservative |

## Troubleshooting

**No crashes found?**
```bash
# Try longer duration
./extract_crash_artifact.sh --duration 600

# Or more workers
./extract_crash_artifact.sh --workers 8
```

**cargo-fuzz not found?**
```bash
cargo install cargo-fuzz
```

**Nightly not installed?**
```bash
rustup install nightly
```

## CI/CD Snippet

```yaml
- name: Fuzz
  run: |
    cd cjson-rs/fuzz
    ./extract_crash_artifact.sh --duration 300
    
- name: Upload
  if: success()
  uses: actions/upload-artifact@v3
  with:
    name: crash-artifacts
    path: cjson-rs/crash_proof*
```

## Victory Output

When successful, you'll see:

```
╔═══════════════════════════════════════════════════════════════════╗
║  🎯 CRASH SECURED: BUG CATCHER ARTIFACT GENERATED 🎯              ║
╚═══════════════════════════════════════════════════════════════════╝
```

## Next Steps After Finding a Crash

1. Review `crash_proof.json` and `crash_proof_REPORT.txt`
2. Reproduce: `cargo +nightly fuzz run fuzz_differential ../crash_proof.json`
3. Add to tests: `cp ../crash_proof.json ../tests/regression_001.json`
4. Document and report the vulnerability

---

**Full docs:** See `ARTIFACT_EXTRACTOR_GUIDE.md`
