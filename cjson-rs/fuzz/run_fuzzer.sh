#!/usr/bin/env bash

# Differential Fuzzing Runner Script
# Automates the setup and execution of the differential fuzzer

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_header() {
    echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║${NC}  ${GREEN}cJSON-rs Differential Fuzzing Harness${NC}                      ${BLUE}║${NC}"
    echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"
    echo
}

print_step() {
    echo -e "${YELLOW}▶${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

check_prerequisites() {
    print_step "Checking prerequisites..."
    
    # Check for Rust
    if ! command -v cargo &> /dev/null; then
        print_error "cargo not found. Please install Rust: https://rustup.rs/"
        exit 1
    fi
    
    # Check for nightly toolchain
    if ! rustup toolchain list | grep -q nightly; then
        print_error "Nightly toolchain not found. Installing..."
        rustup install nightly
    fi
    print_success "Rust nightly found"
    
    # Check for cargo-fuzz
    if ! cargo fuzz --version &> /dev/null; then
        print_error "cargo-fuzz not found. Installing..."
        cargo install cargo-fuzz
    fi
    print_success "cargo-fuzz found"
}

setup_seed_corpus() {
    print_step "Setting up seed corpus..."
    
    CORPUS_DIR="$SCRIPT_DIR/corpus/fuzz_differential"
    mkdir -p "$CORPUS_DIR"
    
    # Add various seed inputs
    echo '{}' > "$CORPUS_DIR/empty_object.json"
    echo '[]' > "$CORPUS_DIR/empty_array.json"
    echo 'null' > "$CORPUS_DIR/null.json"
    echo 'true' > "$CORPUS_DIR/true.json"
    echo 'false' > "$CORPUS_DIR/false.json"
    echo '0' > "$CORPUS_DIR/zero.json"
    echo '-123' > "$CORPUS_DIR/negative.json"
    echo '3.14159' > "$CORPUS_DIR/float.json"
    echo '"hello"' > "$CORPUS_DIR/string.json"
    echo '{"key":"value"}' > "$CORPUS_DIR/simple_object.json"
    echo '[1,2,3]' > "$CORPUS_DIR/simple_array.json"
    echo '{"nested":{"key":"value"}}' > "$CORPUS_DIR/nested.json"
    echo '[[[[[1]]]]]' > "$CORPUS_DIR/deep_nesting.json"
    echo '"\u0000"' > "$CORPUS_DIR/null_char.json"
    echo '"\uD834\uDD1E"' > "$CORPUS_DIR/surrogate_pair.json"
    echo '{"a":1,"b":2,"c":3,"d":4,"e":5}' > "$CORPUS_DIR/many_keys.json"
    
    # Malformed inputs (should be rejected by both)
    echo '{invalid' > "$CORPUS_DIR/malformed_unclosed.json"
    echo '{"key":}' > "$CORPUS_DIR/malformed_no_value.json"
    echo '{,}' > "$CORPUS_DIR/malformed_comma.json"
    
    # Edge cases
    echo '1e308' > "$CORPUS_DIR/huge_exponent.json"
    echo '0.0000000000000001' > "$CORPUS_DIR/tiny_float.json"
    printf '"\x00"' > "$CORPUS_DIR/embedded_null.bin"
    
    print_success "Seed corpus created with $(ls -1 "$CORPUS_DIR" | wc -l) files"
}

run_fuzzer() {
    local duration=${1:-300}  # Default 5 minutes
    local timeout=${2:-10}    # Default 10s per input
    
    print_step "Running fuzzer for $duration seconds (timeout: ${timeout}s per input)..."
    
    cd "$PROJECT_ROOT"
    
    # Run the fuzzer with recommended settings
    cargo +nightly fuzz run fuzz_differential -- \
        -max_total_time="$duration" \
        -timeout="$timeout" \
        -print_final_stats=1 \
        -print_pcs=0 \
        -print_corpus_stats=1
    
    local exit_code=$?
    
    if [ $exit_code -eq 0 ]; then
        print_success "Fuzzing completed successfully"
    else
        print_error "Fuzzing found issues! Check artifacts directory."
        return $exit_code
    fi
}

show_artifacts() {
    print_step "Checking for discovered artifacts..."
    
    ARTIFACTS_DIR="$SCRIPT_DIR/artifacts/fuzz_differential"
    
    if [ -d "$ARTIFACTS_DIR" ] && [ "$(ls -A "$ARTIFACTS_DIR" 2>/dev/null)" ]; then
        print_error "Found crash artifacts:"
        ls -lh "$ARTIFACTS_DIR"
        echo
        echo "To reproduce a crash:"
        echo "  cargo +nightly fuzz run fuzz_differential $ARTIFACTS_DIR/<artifact-file>"
    else
        print_success "No crashes found"
    fi
}

show_corpus_stats() {
    print_step "Corpus statistics..."
    
    CORPUS_DIR="$SCRIPT_DIR/corpus/fuzz_differential"
    
    if [ -d "$CORPUS_DIR" ]; then
        local count=$(ls -1 "$CORPUS_DIR" | wc -l)
        local size=$(du -sh "$CORPUS_DIR" | cut -f1)
        echo "  Files: $count"
        echo "  Total size: $size"
    fi
}

minimize_corpus() {
    print_step "Minimizing corpus (removing redundant inputs)..."
    
    cd "$PROJECT_ROOT"
    cargo +nightly fuzz cmin fuzz_differential
    
    print_success "Corpus minimized"
}

usage() {
    cat << EOF
Usage: $0 [COMMAND] [OPTIONS]

Commands:
    run [DURATION] [TIMEOUT]  Run fuzzer (default: 300s duration, 10s timeout)
    setup                     Setup seed corpus only
    minimize                  Minimize corpus
    artifacts                 Show discovered artifacts
    stats                     Show corpus statistics
    help                      Show this help

Examples:
    $0 run                    # Run for 5 minutes (default)
    $0 run 3600 30            # Run for 1 hour, 30s timeout per input
    $0 setup                  # Setup seed corpus
    $0 minimize               # Minimize corpus
    $0 artifacts              # Check for crashes

Environment Variables:
    FUZZ_DURATION    Default fuzzing duration in seconds (default: 300)
    FUZZ_TIMEOUT     Default timeout per input in seconds (default: 10)
    
EOF
}

main() {
    print_header
    
    local command="${1:-run}"
    shift || true
    
    case "$command" in
        run)
            check_prerequisites
            setup_seed_corpus
            local duration="${1:-${FUZZ_DURATION:-300}}"
            local timeout="${2:-${FUZZ_TIMEOUT:-10}}"
            run_fuzzer "$duration" "$timeout"
            show_artifacts
            show_corpus_stats
            ;;
        setup)
            setup_seed_corpus
            ;;
        minimize)
            check_prerequisites
            minimize_corpus
            ;;
        artifacts)
            show_artifacts
            ;;
        stats)
            show_corpus_stats
            ;;
        help|--help|-h)
            usage
            ;;
        *)
            print_error "Unknown command: $command"
            echo
            usage
            exit 1
            ;;
    esac
}

main "$@"
