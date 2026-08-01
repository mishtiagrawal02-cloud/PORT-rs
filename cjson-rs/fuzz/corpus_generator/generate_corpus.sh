#!/usr/bin/env bash

# CVE-2023-50471 Corpus Generator - Quick Run Script

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║${NC}  ${RED}⚠️  CVE-2023-50471 Vulnerability Corpus Generator${NC}        ${BLUE}║${NC}"
echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"
echo

# Warning
echo -e "${RED}⚠️  WARNING: This tool generates MALICIOUS payloads${NC}"
echo -e "${YELLOW}These payloads are designed to crash and exploit the C parser.${NC}"
echo -e "${YELLOW}Use ONLY for authorized security testing and fuzzing.${NC}"
echo
read -p "Do you understand and wish to proceed? (yes/no): " confirm

if [ "$confirm" != "yes" ]; then
    echo -e "${RED}Aborted.${NC}"
    exit 0
fi

echo

# Build
echo -e "${GREEN}▶ Building corpus generator...${NC}"
cargo build --release

echo
echo -e "${GREEN}▶ Running generator...${NC}"
cargo run --release

echo
echo -e "${GREEN}✓ Corpus generation complete!${NC}"

# Stats
CORPUS_DIR="../corpus/fuzz_differential"
if [ -d "$CORPUS_DIR" ]; then
    FILE_COUNT=$(ls -1 "$CORPUS_DIR" | wc -l | tr -d ' ')
    TOTAL_SIZE=$(du -sh "$CORPUS_DIR" | cut -f1)
    
    echo
    echo -e "${BLUE}╔═══════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║${NC}  Corpus Statistics                                         ${BLUE}║${NC}"
    echo -e "${BLUE}╠═══════════════════════════════════════════════════════════════╣${NC}"
    echo -e "${BLUE}║${NC}  Total Payloads: ${GREEN}${FILE_COUNT}${NC}"
    echo -e "${BLUE}║${NC}  Total Size: ${GREEN}${TOTAL_SIZE}${NC}"
    echo -e "${BLUE}║${NC}  Location: ${CORPUS_DIR}"
    echo -e "${BLUE}╚═══════════════════════════════════════════════════════════════╝${NC}"
    
    echo
    echo -e "${YELLOW}Next steps:${NC}"
    echo -e "  1. cd ../"
    echo -e "  2. cargo +nightly fuzz run fuzz_differential"
    echo -e "  3. Watch for crashes in artifacts/fuzz_differential/"
    echo
    
    # Show sample payloads
    echo -e "${YELLOW}Sample payloads generated:${NC}"
    ls -1 "$CORPUS_DIR" | head -10
    if [ $FILE_COUNT -gt 10 ]; then
        echo "  ... and $((FILE_COUNT - 10)) more"
    fi
fi
