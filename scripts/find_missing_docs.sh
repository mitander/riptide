#!/bin/bash
# Find all public items missing documentation

echo "=== PUBLIC ITEMS MISSING DOCUMENTATION ==="
echo

# Find public structs without docs
echo "🏗️  PUBLIC STRUCTS WITHOUT DOCS:"
grep -rn "^pub struct" src/ | while IFS=: read -r file line content; do
    # Check if previous line has doc comment
    prev_line=$((line - 1))
    if [ $prev_line -gt 0 ]; then
        prev_content=$(sed -n "${prev_line}p" "$file")
        if [[ ! "$prev_content" =~ ^[[:space:]]*/// ]]; then
            echo "  $file:$line - $content"
        fi
    fi
done
echo

# Find public enums without docs
echo "📋 PUBLIC ENUMS WITHOUT DOCS:"
grep -rn "^pub enum" src/ | while IFS=: read -r file line content; do
    prev_line=$((line - 1))
    if [ $prev_line -gt 0 ]; then
        prev_content=$(sed -n "${prev_line}p" "$file")
        if [[ ! "$prev_content" =~ ^[[:space:]]*/// ]]; then
            echo "  $file:$line - $content"
        fi
    fi
done
echo

# Find public functions without docs
echo "⚙️  PUBLIC FUNCTIONS WITHOUT DOCS:"
grep -rn "^[[:space:]]*pub.*fn" src/ | while IFS=: read -r file line content; do
    prev_line=$((line - 1))
    if [ $prev_line -gt 0 ]; then
        prev_content=$(sed -n "${prev_line}p" "$file")
        if [[ ! "$prev_content" =~ ^[[:space:]]*/// ]]; then
            echo "  $file:$line - $content"
        fi
    fi
done
echo

# Find Result-returning functions missing # Errors
echo "❌ RESULT FUNCTIONS MISSING # Errors SECTION:"
grep -rn "-> Result<" src/ | grep "pub.*fn" | while IFS=: read -r file line content; do
    # Check if function has # Errors section in its documentation
    start_line=$((line - 10))
    if [ $start_line -lt 1 ]; then start_line=1; fi
    doc_block=$(sed -n "${start_line},${line}p" "$file")
    if [[ "$doc_block" =~ /// ]] && [[ ! "$doc_block" =~ "# Errors" ]]; then
        echo "  $file:$line - $content"
    fi
done
echo

# Summary
total_structs=$(grep -rc "^pub struct" src/ | awk -F: '{sum += $2} END {print sum}')
total_enums=$(grep -rc "^pub enum" src/ | awk -F: '{sum += $2} END {print sum}')
total_fns=$(grep -rc "^[[:space:]]*pub.*fn" src/ | awk -F: '{sum += $2} END {print sum}')

echo "📊 SUMMARY:"
echo "  Total public structs: $total_structs"
echo "  Total public enums: $total_enums"  
echo "  Total public functions: $total_fns"
echo
echo "Run this script to track documentation progress."
echo "Goal: 100% documentation coverage for all public APIs."