#!/bin/bash
# Development script for Riptide TypeScript frontend

set -e

echo "Starting Riptide TypeScript development..."

# Check if Node.js dependencies are installed
if [ ! -d "node_modules" ]; then
    echo "Installing npm dependencies..."
    npm install
fi

# Type check
echo "Type checking TypeScript..."
npm run typecheck

# Build TypeScript
echo "Building TypeScript..."
npm run build

echo "TypeScript build complete!"
echo "Output: static/js/app.js"
echo ""
echo "To watch for changes, run: npm run dev"
echo "To start the Rust server: cargo run --bin riptide serve"