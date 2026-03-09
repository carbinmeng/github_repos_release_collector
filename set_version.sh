#!/bin/bash

# set_version.sh - Update version number in all required files
# Usage: ./set_version.sh 1.1.0

NEW_VERSION=$1

if [ -z "$NEW_VERSION" ]; then
    echo "❌ Error: No version number provided"
    echo "Usage: ./set_version.sh 1.1.0"
    echo ""
    echo "Examples:"
    echo "  ./set_version.sh 1.1.0  # Minor feature release"
    echo "  ./set_version.sh 1.0.1  # Patch release"
    echo "  ./set_version.sh 2.0.0  # Major release"
    exit 1
fi

# Validate version format (semantic versioning)
if [[ ! $NEW_VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "❌ Error: Invalid version format"
    echo "Expected format: MAJOR.MINOR.PATCH (e.g., 1.1.0)"
    exit 1
fi

echo "🔄 Updating version to $NEW_VERSION..."

# Update Cargo.toml
if [ -f "Cargo.toml" ]; then
    sed -i.bak "s/^version = .*/version = \"$NEW_VERSION\"/" Cargo.toml
    echo "✅ Updated Cargo.toml"
    rm -f Cargo.toml.bak
else
    echo "❌ Error: Cargo.toml not found"
    exit 1
fi

# Update CLI module
if [ -f "src/cli/mod.rs" ]; then
    sed -i.bak "s/#\[command(version = .*\)]/#[command(version = \"$NEW_VERSION\")]/" src/cli/mod.rs
    echo "✅ Updated src/cli/mod.rs"
    rm -f src/cli/mod.rs.bak
else
    echo "❌ Error: src/cli/mod.rs not found"
    exit 1
fi

echo ""
echo "🎉 Version updated successfully!"
echo ""
echo "📋 Summary of changes:"
echo "   - Cargo.toml: version = \"$NEW_VERSION\""
echo "   - src/cli/mod.rs: #[command(version = \"$NEW_VERSION\")]"
echo ""
echo "🚀 Next steps:"
echo "   1. Review changes: git diff"
echo "   2. Rebuild: cargo build --release"
echo "   3. Test: ./target/release/github-release-collector --version"
echo "   4. Commit: git add . && git commit -m \"chore: bump version to $NEW_VERSION\""
