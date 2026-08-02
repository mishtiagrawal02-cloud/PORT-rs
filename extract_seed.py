#!/usr/bin/env python3
"""
Extract crash-inducing seed from libFuzzer/cargo-fuzz crash logs.
Parses raw terminal output and writes the malformed JSON to crash_proof.json.

Usage:
    python3 extract_seed.py < fuzzer_crash.log
    python3 extract_seed.py fuzzer_crash.log
"""

import sys
import re
import os


def extract_crash_seed(log_content):
    """Extract the crash seed from libFuzzer log output."""
    
    # Pattern 1: Boxed hex dump format (║ 0000  34 00 01 00 ...)
    hex_bytes = []
    for line in log_content.splitlines():
        # Match lines like: ║ 0000  34 00 01 00 05 00 00 00 00 00 00 00 0a 00 00 00  │ 4...
        hex_match = re.search(r'║\s+[0-9a-fA-F]{4}\s+((?:[0-9a-fA-F]{2}\s+)+)', line)
        if hex_match:
            hex_str = hex_match.group(1).strip()
            for byte_str in hex_str.split():
                hex_bytes.append(int(byte_str, 16))
    
    if hex_bytes:
        return bytes(hex_bytes), "differential fuzzer hex dump"
    
    # Pattern 2: "Test unit written to <path>"
    artifact_match = re.search(r'Test unit written to\s+(.+)', log_content)
    if artifact_match:
        artifact_path = artifact_match.group(1).strip()
        # Try to read the artifact file
        for base_path in ['', 'fuzz/', 'cjson-rs/fuzz/', '../']:
            full_path = os.path.join(base_path, artifact_path)
            if os.path.exists(full_path):
                with open(full_path, 'rb') as f:
                    return f.read(), f"artifact: {artifact_path}"
    
    # Pattern 3: Hex dump with 0x prefix (lines with multiple "0xXX" values)
    hex_bytes = []
    for line in log_content.splitlines():
        hex_matches = re.findall(r'0x([0-9a-fA-F]{2})', line)
        if len(hex_matches) > 5:  # Likely a hex dump line
            hex_bytes = [int(h, 16) for h in hex_matches]
            break
    
    if hex_bytes:
        return bytes(hex_bytes), "hex dump (0x format)"
    
    # Pattern 4: artifact_prefix path
    artifact_match = re.search(r"artifact_prefix='([^']+)'", log_content)
    if artifact_match:
        artifact_dir = artifact_match.group(1)
        crash_match = re.search(r'(crash-[a-f0-9]+)', log_content)
        if crash_match:
            crash_file = crash_match.group(1)
            full_path = os.path.join(artifact_dir, crash_file)
            if os.path.exists(full_path):
                with open(full_path, 'rb') as f:
                    return f.read(), f"artifact: {full_path}"
    
    # Pattern 5: Direct artifact file path
    artifact_match = re.search(r'(artifacts/[^\s]+/crash-[a-f0-9]+)', log_content)
    if artifact_match:
        artifact_path = artifact_match.group(1).strip()
        for base_path in ['', 'fuzz/', 'cjson-rs/fuzz/']:
            full_path = os.path.join(base_path, artifact_path)
            if os.path.exists(full_path):
                with open(full_path, 'rb') as f:
                    return f.read(), f"artifact: {artifact_path}"
    
    return None, "no crash seed found"


def main():
    # Read input from file argument or stdin
    if len(sys.argv) > 1:
        with open(sys.argv[1], 'r', encoding='utf-8', errors='replace') as f:
            log_content = f.read()
    elif not sys.stdin.isatty():
        log_content = sys.stdin.read()
    else:
        print("Usage: python3 extract_seed.py <fuzzer_crash.log>")
        print("   or: cat fuzzer_crash.log | python3 extract_seed.py")
        sys.exit(1)
    
    # Extract the crash seed
    crash_bytes, source = extract_crash_seed(log_content)
    
    if crash_bytes is None:
        print(f"❌ ERROR: Could not extract crash seed from log")
        print(f"\nExpected libFuzzer output patterns:")
        print(f"  - 'Test unit written to <path>'")
        print(f"  - Hex dump (0xXX, 0xXX, ...)")
        print(f"  - artifact_prefix='<path>'")
        print()
        print("📄 Log content preview:")
        print("-" * 70)
        preview = log_content[:500] if len(log_content) > 500 else log_content
        print(preview)
        if len(log_content) > 500:
            print(f"\n... ({len(log_content) - 500} more characters)")
        print()
        print("💡 TIP: See EXAMPLE_CRASH_LOG.txt for expected format")
        print("💡 TIP: Run fuzzer with: cd cjson-rs/fuzz && cargo +nightly fuzz run fuzz_differential")
        sys.exit(1)
    
    # Write to crash_proof.json in root directory
    output_file = 'crash_proof.json'
    with open(output_file, 'wb') as f:
        f.write(crash_bytes)
    
    # Display results
    print("═" * 70)
    print("🎯 CRASH SEED EXTRACTED SUCCESSFULLY")
    print("═" * 70)
    print(f"Source:      {source}")
    print(f"Output:      {os.path.abspath(output_file)}")
    print(f"Size:        {len(crash_bytes)} bytes")
    print()
    
    # Show preview
    try:
        text = crash_bytes.decode('utf-8')
        print("Content Preview:")
        print("-" * 70)
        preview = text[:200] if len(text) > 200 else text
        print(preview)
        if len(text) > 200:
            print(f"... ({len(text) - 200} more bytes)")
    except UnicodeDecodeError:
        print("Content (hex):")
        print("-" * 70)
        hex_preview = ' '.join(f'{b:02x}' for b in crash_bytes[:64])
        print(hex_preview)
        if len(crash_bytes) > 64:
            print(f"... ({len(crash_bytes) - 64} more bytes)")
    
    print()
    print("═" * 70)
    print("Ready for live demo! Use with:")
    print(f"  cat {output_file} | <your_demo_command>")
    print(f"  cargo +nightly fuzz run fuzz_differential {output_file}")
    print("═" * 70)


if __name__ == '__main__':
    main()
