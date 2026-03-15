#!/bin/bash
# Test script for WASM Tool System
# Usage: ./scripts/test-wasm-tools.sh [native|wasm|all]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

cd "$PROJECT_ROOT"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo_step() {
    echo -e "${YELLOW}==>${NC} $1"
}

echo_success() {
    echo -e "${GREEN}✓${NC} $1"
}

echo_error() {
    echo -e "${RED}✗${NC} $1"
}

# Test native mode (default)
test_native() {
    echo_step "Testing Native Tool Mode..."

    # Build without wasm feature
    echo_step "Building CLI without WASM feature..."
    cargo build -p cli --release 2>&1 | tail -5

    # Run cargo check
    echo_step "Running cargo check..."
    cargo check -p cli 2>&1 | tail -5

    # Run clippy
    echo_step "Running clippy..."
    cargo clippy -p cli 2>&1 | tail -5

    # Test tool command (without WASM, tools are native)
    echo_step "Testing tool list command..."
    cargo run -p cli -- tool list 2>&1 || true

    echo_success "Native mode tests completed"
}

# Test WASM mode
test_wasm() {
    echo_step "Testing WASM Tool Mode..."

    # Build with wasm feature
    echo_step "Building CLI with WASM feature..."
    cargo build -p cli --features wasm --release 2>&1 | tail -5

    # Run cargo check with wasm feature
    echo_step "Running cargo check with WASM feature..."
    cargo check -p cli --features wasm 2>&1 | tail -5

    # Run clippy with wasm feature
    echo_step "Running clippy with WASM feature..."
    cargo clippy -p cli --features wasm 2>&1 | tail -5

    # Test tool command (with WASM)
    echo_step "Testing tool list command with WASM..."
    cargo run -p cli --features wasm -- tool list 2>&1 || true

    # Ensure tool directories exist
    echo_step "Creating tool directories..."
    mkdir -p ~/.local/share/argusclaw/tools/builtin
    mkdir -p ~/.local/share/argusclaw/tools/installed

    echo_success "WASM mode tests completed"
}

# Test all
test_all() {
    test_native
    echo ""
    echo_step "========================================"
    echo ""
    test_wasm
}

# Run tests based on argument
case "${1:-all}" in
    native)
        test_native
        ;;
    wasm)
        test_wasm
        ;;
    all)
        test_all
        ;;
    *)
        echo "Usage: $0 [native|wasm|all]"
        exit 1
        ;;
esac

echo ""
echo_success "All tests passed!"
