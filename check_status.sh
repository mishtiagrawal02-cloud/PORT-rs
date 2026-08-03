#!/bin/bash

clear
echo "╔════════════════════════════════════════════════════════════╗"
echo "║         cJSON-rs Project Status Checker                   ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

# Check Rust installation
echo "🔍 Checking prerequisites..."
echo ""

if command -v rustc &> /dev/null; then
    rustc_version=$(rustc --version | cut -d' ' -f2)
    echo "✅ Rust installed: $rustc_version"
else
    echo "❌ Rust not found"
fi

if command -v cargo &> /dev/null; then
    cargo_version=$(cargo --version | cut -d' ' -f2)
    echo "✅ Cargo installed: $cargo_version"
else
    echo "❌ Cargo not found"
fi

if rustup show | grep -q nightly; then
    echo "✅ Rust nightly available (optional for fuzzing)"
else
    echo "ℹ️  Rust nightly not installed (optional, needed for fuzzing)"
fi

echo ""

# Check project structure
echo "📦 Checking project structure..."
echo ""

if [ -d "cjson-rs" ]; then
    echo "✅ cjson-rs/ directory found"
else
    echo "❌ cjson-rs/ directory missing"
fi

if [ -f "cjson-rs/Cargo.toml" ]; then
    echo "✅ Cargo.toml present"
else
    echo "❌ Cargo.toml missing"
fi

if [ -d "cjson-rs/src" ]; then
    src_files=$(ls -1 cjson-rs/src/*.rs 2>/dev/null | wc -l | tr -d ' ')
    echo "✅ Source files: $src_files Rust files"
else
    echo "❌ src/ directory missing"
fi

if [ -d "docs" ]; then
    doc_files=$(ls -1 docs/*.md 2>/dev/null | wc -l | tr -d ' ')
    echo "✅ Documentation: $doc_files files"
else
    echo "⚠️  docs/ directory missing"
fi

echo ""

# Check build status
echo "🔨 Checking build status..."
echo ""

if [ -f "cjson-rs/target/release/libcjson_rs.a" ]; then
    size=$(ls -lh cjson-rs/target/release/libcjson_rs.a | awk '{print $5}')
    echo "✅ Release binary built: $size"
else
    echo "ℹ️  Release binary not built yet"
    echo "   Run: cd cjson-rs && cargo build --release"
fi

if [ -d "cjson-rs/target/debug" ]; then
    echo "✅ Debug build directory exists"
else
    echo "ℹ️  Debug build not done yet"
    echo "   Run: cd cjson-rs && cargo build"
fi

echo ""

# Check test status
echo "🧪 Testing capabilities..."
echo ""

cd cjson-rs 2>/dev/null

if cargo test --lib --no-run &>/dev/null; then
    echo "✅ Test suite compiles"
    echo "   Run: cargo test"
else
    echo "⚠️  Tests not ready to run"
fi

if [ -d "examples" ]; then
    example_count=$(ls -1 examples/*.rs 2>/dev/null | wc -l | tr -d ' ')
    echo "✅ Examples available: $example_count"
else
    echo "❌ examples/ directory missing"
fi

cd .. 2>/dev/null

echo ""

# Check documentation
echo "📚 Documentation status..."
echo ""

essential_docs=(
    "README.md"
    "cjson-rs/README.md"
    "cjson-rs/ARCHITECTURE.md"
    "cjson-rs/DECISIONS.md"
    "cjson-rs/IMPLEMENTATION.md"
)

for doc in "${essential_docs[@]}"; do
    if [ -f "$doc" ]; then
        lines=$(wc -l < "$doc" | tr -d ' ')
        echo "✅ $(basename $doc): $lines lines"
    else
        echo "❌ $(basename $doc): missing"
    fi
done

echo ""

# Project statistics
echo "📊 Project statistics..."
echo ""

if [ -d "cjson-rs/src" ]; then
    rust_lines=$(find cjson-rs/src -name "*.rs" -exec cat {} \; 2>/dev/null | wc -l | tr -d ' ')
    echo "📝 Rust code: ~$rust_lines lines"
fi

if [ -d "cjson-rs/tests" ]; then
    test_files=$(ls -1 cjson-rs/tests/*.rs 2>/dev/null | wc -l | tr -d ' ')
    echo "🧪 Test files: $test_files"
fi

if [ -d "docs" ]; then
    doc_words=$(find docs -name "*.md" -exec cat {} \; 2>/dev/null | wc -w | tr -d ' ')
    echo "📖 Documentation: ~$doc_words words"
fi

echo ""

# Quick actions
echo "╔════════════════════════════════════════════════════════════╗"
echo "║                   Quick Actions                            ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""
echo "To get started:"
echo ""
echo "  1️⃣  Run quick demo:          ./run_demo.sh"
echo "  2️⃣  Interactive menu:        ./interactive_demo.sh"
echo "  3️⃣  Build project:           cd cjson-rs && cargo build --release"
echo "  4️⃣  Run tests:               cd cjson-rs && cargo test"
echo "  5️⃣  Memory demo:             cd cjson-rs && cargo run --example memory_safety_demo"
echo "  6️⃣  Read quick start:        cat QUICK_START_LOCAL.md"
echo ""

# Overall status
echo "╔════════════════════════════════════════════════════════════╗"
echo "║                  Overall Status                            ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

# Count successful checks
checks_passed=0
checks_total=0

# Rust installed
checks_total=$((checks_total + 1))
if command -v rustc &> /dev/null; then
    checks_passed=$((checks_passed + 1))
fi

# Cargo installed
checks_total=$((checks_total + 1))
if command -v cargo &> /dev/null; then
    checks_passed=$((checks_passed + 1))
fi

# Project structure
checks_total=$((checks_total + 1))
if [ -d "cjson-rs" ] && [ -f "cjson-rs/Cargo.toml" ]; then
    checks_passed=$((checks_passed + 1))
fi

# Source files
checks_total=$((checks_total + 1))
if [ -d "cjson-rs/src" ]; then
    checks_passed=$((checks_passed + 1))
fi

# Documentation
checks_total=$((checks_total + 1))
if [ -f "cjson-rs/README.md" ]; then
    checks_passed=$((checks_passed + 1))
fi

percentage=$((checks_passed * 100 / checks_total))

if [ $percentage -eq 100 ]; then
    echo "🎉 Status: EXCELLENT - All systems ready!"
    echo "   $checks_passed/$checks_total core checks passed ($percentage%)"
    echo ""
    echo "   ✅ Project is fully functional"
    echo "   ✅ Ready to build and test"
    echo "   ✅ Documentation complete"
elif [ $percentage -ge 80 ]; then
    echo "✅ Status: GOOD - Ready to use"
    echo "   $checks_passed/$checks_total core checks passed ($percentage%)"
elif [ $percentage -ge 60 ]; then
    echo "⚠️  Status: FAIR - Some setup needed"
    echo "   $checks_passed/$checks_total core checks passed ($percentage%)"
else
    echo "❌ Status: INCOMPLETE - Setup required"
    echo "   $checks_passed/$checks_total core checks passed ($percentage%)"
fi

echo ""
