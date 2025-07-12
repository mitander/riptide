#!/usr/bin/env python3
"""
Fix documentation format violations systematically.
Adds missing blank lines after # Errors and # Panics headers.
"""

import os
import re
import sys
from pathlib import Path


def fix_documentation_format(file_path):
    """Fix documentation format in a single file."""
    try:
        with open(file_path, 'r') as f:
            content = f.read()
        
        original_content = content
        
        # Pattern 1: /// # Errors followed immediately by content (not blank line)
        # Replace with: /// # Errors\n///\n[content]
        pattern1 = r'(/// # Errors\n)(/// [^/\n])'
        replacement1 = r'\1///\n\2'
        content = re.sub(pattern1, replacement1, content)
        
        # Pattern 2: /// # Panics followed immediately by content (not blank line)  
        # Replace with: /// # Panics\n///\n[content]
        pattern2 = r'(/// # Panics\n)(/// [^/\n])'
        replacement2 = r'\1///\n\2'
        content = re.sub(pattern2, replacement2, content)
        
        # Check if we made any changes
        if content != original_content:
            # Write back to file
            with open(file_path, 'w') as f:
                f.write(content)
            return True
        
        return False
        
    except Exception as e:
        print(f"Error processing {file_path}: {e}")
        return False


def find_rust_files(root_dir):
    """Find all Rust files in the workspace, excluding test files."""
    rust_files = []
    
    for path in Path(root_dir).rglob("*.rs"):
        path_str = str(path)
        
        # Skip target directories and test files
        if ("/target/" in path_str or 
            "/tests/" in path_str or
            "test_" in path_str or
            path.name == "naming_violations.rs"):
            continue
            
        rust_files.append(path)
    
    return rust_files


def main():
    """Fix documentation format violations across the workspace."""
    print("🔧 Fixing documentation format violations...")
    
    # Find all Rust files
    rust_files = find_rust_files(".")
    print(f"📁 Found {len(rust_files)} Rust files to check")
    
    fixed_count = 0
    
    for file_path in rust_files:
        if fix_documentation_format(file_path):
            fixed_count += 1
            print(f"✅ Fixed: {file_path}")
    
    print(f"\n🎉 Fixed documentation format in {fixed_count} files")
    print("📋 Run the style enforcement test to verify:")
    print("   cargo test --test integration naming_convention_enforcement")


if __name__ == "__main__":
    main()