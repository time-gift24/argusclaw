#!/bin/bash
# Build and test WASM tools
# This script builds the built-in WASM tools and runs tests

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

# Check if wasm32 target is available
check_wasm_target() {
    echo_step "Checking WASM build environment..."

    if ! command -v rustup &> /dev/null; then
        echo_error "rustup not found. Please install rustup."
        exit 1
    fi

    # Add wasm32 target if not present
    if ! rustup target list | grep -q "wasm32-unknown-unknown (installed)"; then
        echo_step "Adding wasm32-unknown-unknown target..."
        rustup target add wasm32-unknown-unknown
    fi

    echo_success "WASM target ready"
}

# Build built-in WASM tools
build_builtin_tools() {
    echo_step "Building built-in WASM tools..."

    TOOLS_DIR="crates/tools"

    if [ ! -d "$TOOLS_DIR" ]; then
        echo_error "Tools directory not found: $TOOLS_DIR"
        echo "  This may need to be created separately."
        return 1
    fi

    # Build each tool
    for tool_dir in "$TOOLS_DIR"/{shell,read,glob,grep}; do
        if [ -d "$tool_dir" ]; then
            tool_name=$(basename "$tool_dir")
            echo_step "Building $tool_name tool..."

            cd "$tool_dir"
            cargo build --target wasm32-unknown-unknown --release
            cd "$PROJECT_ROOT"

            echo_success "Built $tool_name"
        fi
    done

    echo_success "All built-in tools built"
}

# Copy tools to data directory
install_tools() {
    echo_step "Installing tools to user directory..."

    TOOLS_DIR="$HOME/.local/share/argusclaw/tools"
    BUILTIN_DIR="$TOOLS_DIR/builtin"

    mkdir -p "$BUILTIN_DIR"

    # Copy built WASM files
    for wasm_file in crates/tools/target/wasm32-unknown-unknown/release/*.wasm; do
        if [ -f "$wasm_file" ]; then
            cp "$wasm_file" "$BUILTIN_DIR/"
            echo_success "Installed $(basename $wasm_file)"
        fi
    done

    echo_success "Tools installed to $BUILTIN_DIR"
}

# Run integration tests
run_tests() {
    echo_step "Running WASM tool tests..."

    # Build CLI with WASM feature
    echo_step "Building CLI with WASM feature..."
    cargo build -p cli --features wasm --release

    # Run cargo tests
    echo_step "Running cargo tests with WASM feature..."
    cargo test -p claw --features wasm -- --nocapture

    echo_success "Tests completed"
}

# Main
main() {
    echo_step "=========================================="
    echo_step "WASM Tools Build and Test Script"
    echo_step "=========================================="
    echo ""

    check_wasm_target
    echo ""

    # Check if tools crate exists
    if [ -d "crates/tools" ]; then
        build_builtin_tools
        echo ""
        install_tools
        echo ""
    else
        echo_step "Skipping tool build (crates/tools not found)"
        echo ""
    fi

    run_tests

    echo ""
    echo_success "=========================================="
    echo_success "All build and test steps completed!"
    echo_success "=========================================="
}

main "$@"
