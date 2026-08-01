#!/usr/bin/env bash

###############################################################################
# ARTIFACT EXTRACTOR: Automated Fuzzer Crash Trap & Evidence Collection
###############################################################################
#
# Purpose: Execute differential fuzzer, trap crashes from C segfaults/UB,
#          and automatically extract the winning crash artifact.
#
# Strategy:
#   1. Execute cargo-fuzz with malformed JSON corpus
#   2. Monitor for crashes (exit codes, ASan/UBSan traps, segfaults)
#   3. Parse fuzzer output to locate crash artifacts
#   4. Extract and preserve the crashing input
#   5. Generate high-visibility proof-of-bug report
#
# Expected Outcome:
#   When the legacy C parser hits a segfault/UB on malformed input while
#   the Rust implementation safely rejects it, we capture that evidence.
#
###############################################################################

set -euo pipefail

# ═══════════════════════════════════════════════════════════════════════════
# CONFIGURATION
# ═══════════════════════════════════════════════════════════════════════════

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FUZZ_TARGET="fuzz_differential"
ARTIFACT_OUTPUT="$PROJECT_ROOT/crash_proof.json"
LOG_FILE="$SCRIPT_DIR/fuzzer_output.log"

# Fuzzing parameters
FUZZ_DURATION="${FUZZ_DURATION:-60}"       # Run for 60 seconds by default
FUZZ_TIMEOUT="${FUZZ_TIMEOUT:-5}"          # 5 second timeout per input
FUZZ_WORKERS="${FUZZ_WORKERS:-1}"          # Single worker to simplify monitoring

# ANSI Color Codes
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
MAGENTA='\033[0;35m'
CYAN='\033[0;36m'
WHITE='\033[1;37m'
NC='\033[0m' # No Color
BOLD='\033[1m'
DIM='\033[2m'

# Box Drawing Characters
BOX_TL="╔"
BOX_TR="╗"
BOX_BL="╚"
BOX_BR="╝"
BOX_H="═"
BOX_V="║"
BOX_VL="╣"
BOX_VR="╠"

# ═══════════════════════════════════════════════════════════════════════════
# UTILITY FUNCTIONS
# ═══════════════════════════════════════════════════════════════════════════

print_banner() {
    local text="$1"
    local color="${2:-$CYAN}"
    echo
    echo -e "${color}${BOX_TL}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_TR}${NC}"
    echo -e "${color}${BOX_V}${NC}  ${BOLD}${WHITE}${text}${NC}$(printf '%*s' $((75 - ${#text})) '')${color}${BOX_V}${NC}"
    echo -e "${color}${BOX_BL}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_BR}${NC}"
    echo
}

print_step() {
    echo -e "${BLUE}▶${NC} ${BOLD}$1${NC}"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}⚠${NC}  $1"
}

print_info() {
    echo -e "${CYAN}ℹ${NC}  $1"
}

print_victory() {
    echo
    echo -e "${GREEN}${BOX_TL}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_TR}${NC}"
    echo -e "${GREEN}${BOX_V}${NC}  ${BOLD}${WHITE}🎯 CRASH SECURED: BUG CATCHER ARTIFACT GENERATED 🎯${NC}           ${GREEN}${BOX_V}${NC}"
    echo -e "${GREEN}${BOX_BL}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_H}${BOX_BR}${NC}"
    echo
}

# ═══════════════════════════════════════════════════════════════════════════
# PRE-FLIGHT CHECKS
# ═══════════════════════════════════════════════════════════════════════════

check_prerequisites() {
    print_step "Running pre-flight checks..."
    
    # Check for cargo
    if ! command -v cargo &> /dev/null; then
        print_error "cargo not found. Please install Rust: https://rustup.rs/"
        exit 1
    fi
    print_success "Cargo found: $(cargo --version | head -n1)"
    
    # Check for nightly toolchain
    if ! rustup toolchain list | grep -q nightly; then
        print_warning "Nightly toolchain not found. Installing..."
        rustup install nightly
    fi
    print_success "Nightly toolchain: $(rustup run nightly rustc --version | cut -d' ' -f2)"
    
    # Check for cargo-fuzz
    if ! cargo fuzz --version &> /dev/null 2>&1; then
        print_warning "cargo-fuzz not found. Installing..."
        cargo install cargo-fuzz
    fi
    print_success "cargo-fuzz: $(cargo fuzz --version 2>&1 | head -n1 || echo 'installed')"
    
    echo
}

# ═══════════════════════════════════════════════════════════════════════════
# CORPUS SETUP
# ═══════════════════════════════════════════════════════════════════════════

setup_malformed_corpus() {
    print_step "Setting up malformed JSON corpus (crash triggers)..."
    
    local corpus_dir="$SCRIPT_DIR/corpus/$FUZZ_TARGET"
    mkdir -p "$corpus_dir"
    
    # Generate known crash patterns and edge cases
    
    # 1. Buffer overflow triggers
    printf '{"key":"' > "$corpus_dir/01_unclosed_string.json"
    printf '["' > "$corpus_dir/02_unclosed_array_string.json"
    
    # 2. Null byte injection
    printf '{"key":\x00"value"}' > "$corpus_dir/03_null_byte_injection.bin"
    printf '\x00{"valid":"json"}' > "$corpus_dir/04_leading_null.bin"
    
    # 3. Deep nesting (stack overflow triggers)
    local deep_nesting='['
    for i in {1..1000}; do
        deep_nesting="${deep_nesting}["
    done
    echo "$deep_nesting" > "$corpus_dir/05_deep_nesting_1000.json"
    
    # 4. Huge numbers (integer overflow)
    echo '999999999999999999999999999999999999999999999999999999999999' > "$corpus_dir/06_huge_number.json"
    echo '1e999999' > "$corpus_dir/07_huge_exponent.json"
    
    # 5. Invalid UTF-8 sequences
    printf '{"key":"\xFF\xFE\xFD"}' > "$corpus_dir/08_invalid_utf8.bin"
    printf '"\xC0\x80"' > "$corpus_dir/09_overlong_encoding.bin"
    
    # 6. Malformed escape sequences
    echo '"\u' > "$corpus_dir/10_incomplete_unicode.json"
    echo '"\uDEAD' > "$corpus_dir/11_lone_surrogate.json"
    
    # 7. Edge case strings
    echo '"\uD800\uDC00"' > "$corpus_dir/12_surrogate_pair.json"
    echo '"\u0000"' > "$corpus_dir/13_escaped_null.json"
    
    # 8. Truncated inputs
    echo '{' > "$corpus_dir/14_lone_brace.json"
    echo '{"key":' > "$corpus_dir/15_truncated_value.json"
    echo '{"key":tru' > "$corpus_dir/16_truncated_true.json"
    
    # 9. Repeated delimiters
    echo '{{{{' > "$corpus_dir/17_repeated_braces.json"
    echo '[[[[[' > "$corpus_dir/18_repeated_brackets.json"
    echo ',,,,,' > "$corpus_dir/19_repeated_commas.json"
    
    # 10. Malformed numbers
    echo '00000000000000000000' > "$corpus_dir/20_leading_zeros.json"
    echo '-.e-' > "$corpus_dir/21_malformed_float.json"
    echo '+123' > "$corpus_dir/22_plus_sign.json"
    
    # 11. Control characters in strings (unescaped)
    printf '{"key":"\x01\x02\x03"}' > "$corpus_dir/23_control_chars.bin"
    printf '"\n\r\t\b\f"' > "$corpus_dir/24_raw_escapes.bin"
    
    # 12. Memory stress patterns
    local huge_key=""
    for i in {1..10000}; do
        huge_key="${huge_key}x"
    done
    echo "{\"$huge_key\":1}" > "$corpus_dir/25_huge_key.json"
    
    # 13. Extreme whitespace
    printf '  \t\n\r\n  \t  {  }  \n\r' > "$corpus_dir/26_extreme_whitespace.json"
    
    # 14. Empty and minimal inputs
    echo '' > "$corpus_dir/27_empty.json"
    echo ' ' > "$corpus_dir/28_single_space.json"
    
    # 15. Mixed valid/invalid
    echo '{"valid":123}garbage' > "$corpus_dir/29_trailing_garbage.json"
    echo 'garbage{"valid":123}' > "$corpus_dir/30_leading_garbage.json"
    
    local count=$(ls -1 "$corpus_dir" | wc -l | tr -d ' ')
    print_success "Generated $count malformed JSON crash triggers"
    echo
}

# ═══════════════════════════════════════════════════════════════════════════
# FUZZER EXECUTION
# ═══════════════════════════════════════════════════════════════════════════

execute_fuzzer() {
    print_step "Launching differential fuzzer (targeting C segfaults/UB)..."
    
    cd "$PROJECT_ROOT"
    
    print_info "Target: $FUZZ_TARGET"
    print_info "Duration: ${FUZZ_DURATION}s"
    print_info "Timeout per input: ${FUZZ_TIMEOUT}s"
    print_info "Workers: $FUZZ_WORKERS"
    print_info "Log: $LOG_FILE"
    echo
    
    # Execute fuzzer and capture output
    # We expect this to fail (exit code != 0) when it finds a crash
    set +e  # Don't exit on error
    
    cargo +nightly fuzz run "$FUZZ_TARGET" \
        -- \
        -max_total_time="$FUZZ_DURATION" \
        -timeout="$FUZZ_TIMEOUT" \
        -workers="$FUZZ_WORKERS" \
        -print_final_stats=1 \
        -detect_leaks=1 \
        -use_value_profile=1 \
        2>&1 | tee "$LOG_FILE"
    
    local exit_code=$?
    set -e  # Re-enable exit on error
    
    echo
    print_info "Fuzzer exit code: $exit_code"
    
    # Exit codes:
    # 0 = No crashes found (completed successfully)
    # 77 = Crash/bug found (libFuzzer convention)
    # Other = System error or signal
    
    if [ $exit_code -eq 0 ]; then
        print_warning "Fuzzer completed without finding crashes"
        print_info "This might mean:"
        print_info "  - The C implementation is robust for this corpus"
        print_info "  - More fuzzing time needed"
        print_info "  - Corpus needs more diverse inputs"
        return 1
    elif [ $exit_code -eq 77 ] || [ $exit_code -ne 0 ]; then
        print_success "Fuzzer detected a crash! (exit code: $exit_code)"
        return 0
    fi
    
    return $exit_code
}

# ═══════════════════════════════════════════════════════════════════════════
# ARTIFACT EXTRACTION
# ═══════════════════════════════════════════════════════════════════════════

extract_crash_artifact() {
    print_step "Hunting for crash artifacts..."
    
    local artifacts_dir="$SCRIPT_DIR/artifacts/$FUZZ_TARGET"
    
    # Check if artifacts directory exists
    if [ ! -d "$artifacts_dir" ]; then
        print_warning "Artifacts directory not found: $artifacts_dir"
        
        # Try to find crash info in the log
        if grep -q "CRASH SECURED\|Test unit written to\|artifact_prefix=" "$LOG_FILE"; then
            print_info "Crash information detected in log, but no artifact directory yet"
        fi
        
        return 1
    fi
    
    # Find crash artifacts
    local crash_files=($(find "$artifacts_dir" -type f -name "crash-*" -o -name "leak-*" -o -name "timeout-*" 2>/dev/null))
    
    if [ ${#crash_files[@]} -eq 0 ]; then
        print_warning "No crash artifacts found in $artifacts_dir"
        return 1
    fi
    
    print_success "Found ${#crash_files[@]} artifact(s)"
    echo
    
    # List all artifacts
    for artifact in "${crash_files[@]}"; do
        local filename=$(basename "$artifact")
        local size=$(wc -c < "$artifact" | tr -d ' ')
        print_info "  - $filename ($size bytes)"
    done
    echo
    
    # Select the first (or most interesting) artifact
    local selected_artifact="${crash_files[0]}"
    local artifact_name=$(basename "$selected_artifact")
    
    print_step "Extracting artifact: $artifact_name"
    
    # Copy the raw bytes to the proof file
    cp "$selected_artifact" "$ARTIFACT_OUTPUT"
    
    print_success "Artifact saved to: $ARTIFACT_OUTPUT"
    
    # Generate detailed report
    generate_artifact_report "$selected_artifact"
    
    return 0
}

# ═══════════════════════════════════════════════════════════════════════════
# REPORT GENERATION
# ═══════════════════════════════════════════════════════════════════════════

generate_artifact_report() {
    local artifact_file="$1"
    local report_file="${ARTIFACT_OUTPUT%.json}_REPORT.txt"
    
    print_step "Generating detailed bug report..."
    
    {
        echo "═══════════════════════════════════════════════════════════════════════"
        echo "  CRASH ARTIFACT ANALYSIS REPORT"
        echo "═══════════════════════════════════════════════════════════════════════"
        echo
        echo "Generated: $(date)"
        echo "Artifact: $(basename "$artifact_file")"
        echo "Size: $(wc -c < "$artifact_file") bytes"
        echo
        echo "───────────────────────────────────────────────────────────────────────"
        echo "HEX DUMP:"
        echo "───────────────────────────────────────────────────────────────────────"
        hexdump -C "$artifact_file" | head -n 50
        echo
        echo "───────────────────────────────────────────────────────────────────────"
        echo "RAW BYTES (Rust literal):"
        echo "───────────────────────────────────────────────────────────────────────"
        echo -n "&["
        hexdump -v -e '/1 "0x%02x, "' "$artifact_file" | sed 's/, $//'
        echo "]"
        echo
        echo "───────────────────────────────────────────────────────────────────────"
        echo "BASE64 ENCODING:"
        echo "───────────────────────────────────────────────────────────────────────"
        base64 < "$artifact_file"
        echo
        echo "───────────────────────────────────────────────────────────────────────"
        echo "AS STRING (if printable):"
        echo "───────────────────────────────────────────────────────────────────────"
        cat "$artifact_file" 2>/dev/null || echo "[Binary data, not printable]"
        echo
        echo
        echo "───────────────────────────────────────────────────────────────────────"
        echo "REPRODUCTION INSTRUCTIONS:"
        echo "───────────────────────────────────────────────────────────────────────"
        echo
        echo "1. Re-run this specific crash:"
        echo "   cd $PROJECT_ROOT"
        echo "   cargo +nightly fuzz run $FUZZ_TARGET $artifact_file"
        echo
        echo "2. Debug with GDB:"
        echo "   cargo +nightly fuzz run -D $FUZZ_TARGET $artifact_file"
        echo
        echo "3. Use in your test suite:"
        echo "   cp $ARTIFACT_OUTPUT tests/crash_regression.json"
        echo
        echo "───────────────────────────────────────────────────────────────────────"
        echo "FUZZER LOG EXCERPT (Last 50 lines):"
        echo "───────────────────────────────────────────────────────────────────────"
        tail -n 50 "$LOG_FILE"
        echo
        echo "═══════════════════════════════════════════════════════════════════════"
        echo "  END OF REPORT"
        echo "═══════════════════════════════════════════════════════════════════════"
    } > "$report_file"
    
    print_success "Detailed report saved to: $report_file"
    echo
    
    # Display key information
    print_info "Quick preview:"
    echo
    echo -e "${DIM}─────────────────────────────────────────────────────────${NC}"
    hexdump -C "$artifact_file" | head -n 10
    echo -e "${DIM}─────────────────────────────────────────────────────────${NC}"
    echo
}

# ═══════════════════════════════════════════════════════════════════════════
# MAIN EXECUTION
# ═══════════════════════════════════════════════════════════════════════════

main() {
    print_banner "🔥 ARTIFACT EXTRACTOR: BUG CATCHER PROTOCOL 🔥" "$RED"
    
    print_info "Mission: Trap C segfaults, extract proof-of-vulnerability artifacts"
    print_info "Method: Differential fuzzing (C vs Rust implementations)"
    echo
    
    # Step 1: Pre-flight checks
    check_prerequisites
    
    # Step 2: Setup malformed corpus
    setup_malformed_corpus
    
    # Step 3: Execute fuzzer
    if execute_fuzzer; then
        # Step 4: Extract crash artifact
        if extract_crash_artifact; then
            # SUCCESS! 
            print_victory
            
            echo -e "${BOLD}${WHITE}Artifact Location:${NC}"
            echo -e "  ${CYAN}$ARTIFACT_OUTPUT${NC}"
            echo
            echo -e "${BOLD}${WHITE}Report Location:${NC}"
            echo -e "  ${CYAN}${ARTIFACT_OUTPUT%.json}_REPORT.txt${NC}"
            echo
            echo -e "${BOLD}${WHITE}Next Steps:${NC}"
            echo -e "  ${YELLOW}1.${NC} Review the crash artifact and report"
            echo -e "  ${YELLOW}2.${NC} Reproduce: ${DIM}cargo +nightly fuzz run $FUZZ_TARGET $ARTIFACT_OUTPUT${NC}"
            echo -e "  ${YELLOW}3.${NC} Add to regression tests"
            echo -e "  ${YELLOW}4.${NC} Document the vulnerability"
            echo
            
            exit 0
        else
            print_error "Failed to extract crash artifact"
            print_info "Check the fuzzer log: $LOG_FILE"
            exit 1
        fi
    else
        print_warning "No crashes detected during this fuzzing session"
        print_info "Recommendations:"
        print_info "  - Increase FUZZ_DURATION (current: ${FUZZ_DURATION}s)"
        print_info "  - Run: FUZZ_DURATION=300 $0"
        print_info "  - Review log: $LOG_FILE"
        echo
        exit 1
    fi
}

# ═══════════════════════════════════════════════════════════════════════════
# SIGNAL HANDLERS
# ═══════════════════════════════════════════════════════════════════════════

cleanup() {
    echo
    print_warning "Interrupted! Cleaning up..."
    
    # Try to extract any artifacts that were found before interruption
    if [ -f "$LOG_FILE" ]; then
        extract_crash_artifact 2>/dev/null || true
    fi
    
    exit 130
}

trap cleanup INT TERM

# ═══════════════════════════════════════════════════════════════════════════
# ENTRY POINT
# ═══════════════════════════════════════════════════════════════════════════

# Parse command-line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --duration)
            FUZZ_DURATION="$2"
            shift 2
            ;;
        --timeout)
            FUZZ_TIMEOUT="$2"
            shift 2
            ;;
        --workers)
            FUZZ_WORKERS="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 [OPTIONS]"
            echo
            echo "Options:"
            echo "  --duration SECONDS   Fuzzing duration (default: 60)"
            echo "  --timeout SECONDS    Timeout per input (default: 5)"
            echo "  --workers COUNT      Number of workers (default: 1)"
            echo "  --help              Show this help"
            echo
            echo "Environment Variables:"
            echo "  FUZZ_DURATION        Same as --duration"
            echo "  FUZZ_TIMEOUT         Same as --timeout"
            echo "  FUZZ_WORKERS         Same as --workers"
            echo
            echo "Examples:"
            echo "  $0"
            echo "  $0 --duration 300 --timeout 10"
            echo "  FUZZ_DURATION=600 $0"
            exit 0
            ;;
        *)
            print_error "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
    esac
done

# Execute main workflow
main
