#!/usr/bin/env python3
"""
Artifact Extractor: Parse libFuzzer Crash Logs and Extract Crash Seeds

This script parses raw libFuzzer (cargo-fuzz) terminal output to:
1. Identify the exact hex/byte seed that caused the crash
2. Decode the seed into the original malformed JSON
3. Write the crash-proof payload to crash_proof.json
4. Format it for live stage demonstrations

Usage:
    python3 extract_seed.py < fuzzer_crash.log
    python3 extract_seed.py fuzzer_crash.log
    cat fuzzer_crash.log | python3 extract_seed.py
"""

import sys
import re
import os
import json
from pathlib import Path
from typing import Optional, Tuple


class FuzzerLogParser:
    """Parse libFuzzer crash logs and extract crash artifacts."""
    
    # Patterns to match different libFuzzer output formats
    PATTERNS = {
        # Pattern 1: "Test unit written to <path>"
        'test_unit': re.compile(r'Test unit written to\s+(.+)'),
        
        # Pattern 2: "artifact_prefix='<path>'; Test unit written to <file>"
        'artifact_prefix': re.compile(r"artifact_prefix='([^']+)'.*Test unit written to\s+(\S+)"),
        
        # Pattern 3: Hex dump in crash report (0xXX, 0xXX, ...)
        'hex_dump': re.compile(r'0x([0-9a-fA-F]{2})'),
        
        # Pattern 4: Base64-encoded crash input
        'base64': re.compile(r'base64:\s*([A-Za-z0-9+/=]+)'),
        
        # Pattern 5: "CRASH SECURED" marker from our differential harness
        'crash_marker': re.compile(r'CRASH SECURED|VULNERABILITY CAUGHT|C_PANIC_RUST_ERR'),
        
        # Pattern 6: Artifact file path in fuzz output
        'artifact_file': re.compile(r'(artifacts/[^\s]+/crash-[a-f0-9]+)'),
        
        # Pattern 7: Direct hex bytes in crash summary
        'crash_bytes': re.compile(r'Crash input:\s*\[([0-9a-fA-F,\s]+)\]'),
    }
    
    def __init__(self, log_content: str):
        """Initialize parser with raw log content."""
        self.log_content = log_content
        self.lines = log_content.splitlines()
    
    def extract_artifact_path(self) -> Optional[str]:
        """Extract the artifact file path from the log."""
        # Try each pattern
        for line in self.lines:
            # Check for "Test unit written to" pattern
            match = self.PATTERNS['test_unit'].search(line)
            if match:
                return match.group(1).strip()
            
            # Check for artifact_prefix pattern
            match = self.PATTERNS['artifact_prefix'].search(line)
            if match:
                prefix = match.group(1)
                filename = match.group(2)
                return os.path.join(prefix, filename)
            
            # Check for artifact file path
            match = self.PATTERNS['artifact_file'].search(line)
            if match:
                return match.group(1).strip()
        
        return None
    
    def extract_hex_dump(self) -> Optional[bytes]:
        """Extract crash bytes from hex dump in the log."""
        hex_bytes = []
        
        # Look for lines with hex dumps
        for line in self.lines:
            # Check for crash bytes pattern
            match = self.PATTERNS['crash_bytes'].search(line)
            if match:
                hex_str = match.group(1).replace(',', ' ').strip()
                hex_bytes = [int(h, 16) for h in hex_str.split()]
                break
            
            # Check for hex dump pattern (0xXX format)
            matches = self.PATTERNS['hex_dump'].findall(line)
            if matches and len(matches) > 5:  # Likely a hex dump line
                hex_bytes.extend([int(h, 16) for h in matches])
        
        return bytes(hex_bytes) if hex_bytes else None
    
    def extract_base64(self) -> Optional[bytes]:
        """Extract crash bytes from base64-encoded input."""
        import base64
        
        for line in self.lines:
            match = self.PATTERNS['base64'].search(line)
            if match:
                try:
                    return base64.b64decode(match.group(1))
                except Exception:
                    continue
        
        return None
    
    def read_artifact_file(self, artifact_path: str) -> Optional[bytes]:
        """Read the crash artifact from disk."""
        # Try relative to current directory
        paths_to_try = [
            artifact_path,
            os.path.join('fuzz', artifact_path),
            os.path.join('..', artifact_path),
            os.path.join(os.getcwd(), artifact_path),
        ]
        
        for path in paths_to_try:
            if os.path.exists(path):
                try:
                    with open(path, 'rb') as f:
                        return f.read()
                except Exception as e:
                    print(f"Warning: Could not read {path}: {e}", file=sys.stderr)
        
        return None
    
    def extract_crash_seed(self) -> Tuple[Optional[bytes], str]:
        """
        Extract the crash-triggering seed from the log.
        
        Returns:
            (crash_bytes, source_description)
        """
        # Method 1: Try to find artifact file path and read it
        artifact_path = self.extract_artifact_path()
        if artifact_path:
            crash_bytes = self.read_artifact_file(artifact_path)
            if crash_bytes:
                return crash_bytes, f"artifact file: {artifact_path}"
        
        # Method 2: Try to extract from hex dump
        crash_bytes = self.extract_hex_dump()
        if crash_bytes:
            return crash_bytes, "hex dump in log"
        
        # Method 3: Try to extract from base64
        crash_bytes = self.extract_base64()
        if crash_bytes:
            return crash_bytes, "base64-encoded input"
        
        return None, "no crash seed found"


def format_hex_dump(data: bytes, width: int = 16) -> str:
    """Format bytes as a readable hex dump."""
    lines = []
    for i in range(0, len(data), width):
        chunk = data[i:i+width]
        hex_part = ' '.join(f'{b:02x}' for b in chunk)
        ascii_part = ''.join(chr(b) if 32 <= b < 127 else '.' for b in chunk)
        lines.append(f'{i:04x}  {hex_part:<{width*3}}  |{ascii_part}|')
    return '\n'.join(lines)


def analyze_crash_payload(data: bytes) -> dict:
    """Analyze the crash payload and extract metadata."""
    analysis = {
        'size': len(data),
        'is_printable': all(32 <= b < 127 or b in [9, 10, 13] for b in data),
        'has_null_bytes': b'\x00' in data,
        'likely_json': data.strip().startswith(b'{') or data.strip().startswith(b'['),
        'truncated': not data.endswith((b'}', b']', b'"')) if data else False,
    }
    
    # Try to decode as UTF-8
    try:
        text = data.decode('utf-8')
        analysis['utf8_valid'] = True
        analysis['text_preview'] = text[:100] if len(text) > 100 else text
    except UnicodeDecodeError:
        analysis['utf8_valid'] = False
        analysis['text_preview'] = None
    
    return analysis


def write_crash_proof(crash_bytes: bytes, output_path: str = 'crash_proof.json') -> str:
    """
    Write the crash payload to crash_proof.json.
    
    Returns the absolute path to the created file.
    """
    # Determine output path (root directory or current directory)
    if not os.path.isabs(output_path):
        # Try to write to project root
        script_dir = Path(__file__).parent
        project_root = script_dir.parent.parent  # fuzz/.. -> cjson-rs/.. -> PORT-rs
        
        candidates = [
            project_root / output_path,
            Path.cwd() / output_path,
            script_dir / output_path,
        ]
        
        # Use first writable location
        for candidate in candidates:
            try:
                candidate.parent.mkdir(parents=True, exist_ok=True)
                output_path = str(candidate)
                break
            except Exception:
                continue
    
    # Write the crash payload
    with open(output_path, 'wb') as f:
        f.write(crash_bytes)
    
    return os.path.abspath(output_path)


def generate_report(crash_bytes: bytes, source: str, output_path: str):
    """Generate a comprehensive crash report."""
    analysis = analyze_crash_payload(crash_bytes)
    
    print("╔═══════════════════════════════════════════════════════════════════════╗")
    print("║          🎯 CRASH ARTIFACT EXTRACTION SUCCESSFUL 🎯                   ║")
    print("╚═══════════════════════════════════════════════════════════════════════╝")
    print()
    print(f"📁 Artifact Source: {source}")
    print(f"💾 Output File: {output_path}")
    print(f"📏 Payload Size: {analysis['size']} bytes")
    print()
    
    print("─────────────────────────────────────────────────────────────────────────")
    print("📊 PAYLOAD ANALYSIS")
    print("─────────────────────────────────────────────────────────────────────────")
    print(f"✓ UTF-8 Valid: {analysis['utf8_valid']}")
    print(f"✓ All Printable: {analysis['is_printable']}")
    print(f"✓ Contains Null Bytes: {analysis['has_null_bytes']}")
    print(f"✓ Looks Like JSON: {analysis['likely_json']}")
    print(f"✓ Appears Truncated: {analysis['truncated']}")
    print()
    
    if analysis['text_preview']:
        print("─────────────────────────────────────────────────────────────────────────")
        print("📝 TEXT PREVIEW")
        print("─────────────────────────────────────────────────────────────────────────")
        print(analysis['text_preview'])
        print()
    
    print("─────────────────────────────────────────────────────────────────────────")
    print("🔢 HEX DUMP (First 256 bytes)")
    print("─────────────────────────────────────────────────────────────────────────")
    print(format_hex_dump(crash_bytes[:256]))
    print()
    
    print("─────────────────────────────────────────────────────────────────────────")
    print("🧪 REPRODUCTION COMMANDS")
    print("─────────────────────────────────────────────────────────────────────────")
    print(f"# Re-run this specific crash:")
    print(f"cargo +nightly fuzz run fuzz_differential {output_path}")
    print()
    print(f"# Debug with GDB:")
    print(f"cargo +nightly fuzz run -D fuzz_differential {output_path}")
    print()
    print(f"# Use in live demo:")
    print(f"cat {output_path} | ./demo_script")
    print()
    
    print("─────────────────────────────────────────────────────────────────────────")
    print("🚀 NEXT STEPS")
    print("─────────────────────────────────────────────────────────────────────────")
    print("1. Review the crash payload above")
    print("2. Test reproduction with cargo fuzz")
    print("3. Add to regression test suite")
    print("4. Prepare for stage demonstration")
    print()
    print("╔═══════════════════════════════════════════════════════════════════════╗")
    print("║                    ✅ EXTRACTION COMPLETE                             ║")
    print("╚═══════════════════════════════════════════════════════════════════════╝")


def main():
    """Main entry point."""
    # Read fuzzer log from stdin or file argument
    if len(sys.argv) > 1:
        log_file = sys.argv[1]
        try:
            with open(log_file, 'r', encoding='utf-8', errors='replace') as f:
                log_content = f.read()
        except Exception as e:
            print(f"Error: Could not read log file '{log_file}': {e}", file=sys.stderr)
            sys.exit(1)
    else:
        if sys.stdin.isatty():
            print("Usage: python3 extract_seed.py <fuzzer_crash.log>", file=sys.stderr)
            print("   or: cat fuzzer_crash.log | python3 extract_seed.py", file=sys.stderr)
            sys.exit(1)
        log_content = sys.stdin.read()
    
    # Parse the log
    parser = FuzzerLogParser(log_content)
    crash_bytes, source = parser.extract_crash_seed()
    
    if crash_bytes is None:
        print("❌ Error: Could not extract crash seed from fuzzer log", file=sys.stderr)
        print("", file=sys.stderr)
        print("The log should contain one of:", file=sys.stderr)
        print("  - 'Test unit written to <path>'", file=sys.stderr)
        print("  - Hex dump of crash input", file=sys.stderr)
        print("  - Base64-encoded crash input", file=sys.stderr)
        print("  - Artifact file path", file=sys.stderr)
        sys.exit(1)
    
    # Write crash proof file
    output_path = write_crash_proof(crash_bytes)
    
    # Generate report
    generate_report(crash_bytes, source, output_path)


if __name__ == '__main__':
    main()
