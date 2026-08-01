# Quick Start Guide: Differential Fuzzing

## 🚀 5-Minute Setup

### Step 1: Install Prerequisites
```bash
# Install Rust nightly
rustup install nightly

# Install cargo-fuzz
cargo install cargo-fuzz
```

### Step 2: Run the Fuzzer (Easy Mode)
```bash
cd /Users/awantikamaheshwari/Desktop/PORT-rs/cjson-rs/fuzz

# Use the convenience script
./run_fuzzer.sh run 300
# This runs for 5 minutes (300 seconds)
```

### Step 3: Check for Crashes
```bash
# If the fuzzer found anything, it's saved here:
ls -la artifacts/fuzz_differential/
```

That's it! 🎉

---

## 📊 Understanding the Output

### Good Run (No Crashes)
```
#12345: cov: 234 ft: 567 corp: 89 exec/s: 2345
✓ Fuzzing completed successfully
✓ No crashes found
```

### Vulnerability Found! 🚨
```
╔═══════════════════════════════════════════════════════════════╗
║ 🚨 DIFFERENTIAL FUZZING DISCREPANCY DETECTED                  ║
╠═══════════════════════════════════════════════════════════════╣
║ Type: C_PANIC_RUST_ERR                                        ║
║ Description: C crashed, Rust safely rejected (GOOD)           ║
╠═══════════════════════════════════════════════════════════════╣
║ HEX DUMP (for reproduction):
║ 0000  7b 7b 7b 7b ...
╚═══════════════════════════════════════════════════════════════╝

✗ Fuzzing found issues! Check artifacts directory.
```

**What this means:**
- The C implementation CRASHED on this input
- The Rust implementation safely REJECTED it
- This is a **memory safety vulnerability** in C that Rust prevents
- The exact input is saved for reproduction

---

## 🎯 Common Use Cases

### Quick Smoke Test (30 seconds)
```bash
./run_fuzzer.sh run 30
```

### Standard Fuzzing Session (1 hour)
```bash
./run_fuzzer.sh run 3600
```

### Overnight Fuzzing (8 hours)
```bash
nohup ./run_fuzzer.sh run 28800 &
```

### Reproduce a Specific Crash
```bash
cd /Users/awantikamaheshwari/Desktop/PORT-rs/cjson-rs
cargo +nightly fuzz run fuzz_differential \
  fuzz/artifacts/fuzz_differential/crash-abc123
```

---

## 🔧 Manual Commands (Alternative to Script)

### Setup Corpus
```bash
cd /Users/awantikamaheshwari/Desktop/PORT-rs/cjson-rs
mkdir -p fuzz/corpus/fuzz_differential

# Add seed files
echo '{"test":123}' > fuzz/corpus/fuzz_differential/seed1.json
echo '[1,2,3]' > fuzz/corpus/fuzz_differential/seed2.json
```

### Run Fuzzer Directly
```bash
cargo +nightly fuzz run fuzz_differential -- \
  -max_total_time=300 \
  -timeout=10
```

### Check Coverage
```bash
cargo +nightly fuzz coverage fuzz_differential
```

### Minimize Corpus
```bash
cargo +nightly fuzz cmin fuzz_differential
```

---

## 🐛 Troubleshooting

### "cargo: command not found"
```bash
# Install Rust first
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

### "nightly toolchain not found"
```bash
rustup install nightly
```

### "cargo-fuzz not found"
```bash
cargo install cargo-fuzz
```

### "undefined reference to cJSON_Parse"
The fuzzer needs the C library to be built. This is automatically linked, but if you see this error:
```bash
cd /Users/awantikamaheshwari/Desktop/PORT-rs
make
```

### Fuzzer runs but finds nothing
- ✅ This is actually good! It means no crashes detected
- To test the fuzzer works, try a longer run: `./run_fuzzer.sh run 3600`
- Or check if seed corpus is diverse: `ls -la fuzz/corpus/fuzz_differential/`

### Out of memory
```bash
# Limit memory usage
cargo +nightly fuzz run fuzz_differential -- -rss_limit_mb=2048
```

---

## 📈 Metrics to Watch

### Execution Speed
```
exec/s: 2345   ← Higher is better (2000+ is good)
```

### Coverage
```
cov: 234       ← Unique code paths discovered
ft: 567        ← Features discovered (instrumentation points)
```

### Corpus Size
```
corp: 89       ← Unique interesting inputs retained
```

### Crashes
```
crash: 3       ← Number of crashes found (investigate each!)
```

---

## 🎓 Next Steps

### Deep Dive
- Read [README.md](README.md) for full documentation
- Read [VULNERABILITY_CLASSES.md](VULNERABILITY_CLASSES.md) for details on what we're detecting

### Customize
- Adjust timeout: `./run_fuzzer.sh run 600 30` (10 min, 30s timeout)
- Add custom seeds: Create files in `fuzz/corpus/fuzz_differential/`

### Contribute
- Found a crash? Report it to cJSON maintainers
- Improved the fuzzer? Submit a PR!

---

## 💡 Tips

1. **Start small**: Run for 5 minutes first to ensure everything works
2. **Gradually increase**: If no issues, try 1 hour, then overnight
3. **Check artifacts**: Even if the fuzzer completes, check `artifacts/` for any findings
4. **Use nohup**: For long runs, use `nohup ./run_fuzzer.sh run 86400 &` to run in background
5. **Monitor progress**: Check stats with `tail -f nohup.out` if running in background

---

## 🆘 Getting Help

- **Documentation**: See [README.md](README.md)
- **Issues**: File an issue on GitHub
- **Questions**: Check the fuzzing section in the main project README

---

## ✅ Checklist

- [ ] Rust nightly installed
- [ ] cargo-fuzz installed
- [ ] Ran first fuzzing session
- [ ] Checked for artifacts
- [ ] Understand how to reproduce findings

Once you've checked all boxes, you're ready for production fuzzing! 🎉
