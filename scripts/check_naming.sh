#!/bin/bash
# Naming conventions enforcement script

set -e

echo "Checking naming conventions..."

# Check for banned module names
echo "Checking for banned module names..."
banned_files=$(find src/ -name "utils.rs" -o -name "helpers.rs" -o -name "common.rs" -o -name "shared.rs")
if [ -n "$banned_files" ]; then
    echo "FAIL: Found banned module names:"
    echo "$banned_files"
    exit 1
fi

# Check for vague TODO comments
echo "Checking for vague TODO comments..."
if grep -r "TODO.*fix" src/ --include="*.rs" -i; then
    echo "FAIL: Found vague TODO comments containing 'fix'"
    exit 1
fi

if grep -r "TODO: Fix\|TODO:Fix\|TODO Fix" src/ --include="*.rs"; then
    echo "FAIL: Found generic 'TODO: Fix' comments"
    exit 1
fi

# Check for missing documentation on public functions (basic check)
echo "Checking for undocumented public functions..."
if grep -r "pub fn" src/ --include="*.rs" -A 1 | grep -B 1 -v "///" | grep "pub fn" | head -5; then
    echo "WARN: Found potentially undocumented public functions (manual review needed)"
fi

# Check for consistent builder patterns
echo "Checking builder pattern consistency..."
inconsistent_builders=$(grep -r "with_\|\.reliability\|\.upload_speed" src/ --include="*.rs" | wc -l)
if [ "$inconsistent_builders" -gt 0 ]; then
    echo "INFO: Found mixed builder patterns - consider standardizing"
fi

# Check for abbreviations in public APIs
echo "Checking for abbreviations in public APIs..."
if grep -r "pub fn.*addr\|pub fn.*msg\|pub fn.*ctx\|pub fn.*info\|pub fn.*tmp" src/ --include="*.rs"; then
    echo "WARN: Found potential abbreviations in public APIs"
fi

echo "PASS: Naming conventions check completed"
echo "INFO: Manual review recommended for any warnings above"