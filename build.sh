#!/bin/bash
set -e

# Build targets with architecture names
declare -A TARGETS=(
    ["x86_64-unknown-linux-musl"]="x86_64"
    ["aarch64-unknown-linux-musl"]="aarch64"
)

# Environment variables for static linking
export OPENSSL_STATIC=1
export OPENSSL_VENDORED=1
export RUSTFLAGS="-C target-feature=+crt-static"

# Fontconfig environment variables - use system libraries
export PKG_CONFIG_ALLOW_CROSS=1
export FONTCONFIG_NO_PKG_CONFIG=0
export RUST_FONTCONFIG_DLOPEN=0
export FONTCONFIG_STATIC=0
export PKG_CONFIG_PATH="/usr/lib/pkgconfig:/usr/share/pkgconfig"

echo "Building for multiple targets with musl..."

# Create package directory
mkdir -p package

for target in "${!TARGETS[@]}"; do
    arch="${TARGETS[$target]}"
    echo "Building for target: $target (architecture: $arch)"
    
    # Build with cross
    if cross build --release --target "$target"; then
        echo "✓ Build successful for $arch"
        
        # Create package directory
        mkdir -p "package/nettest-linux-$arch"
        
        # Copy binary
        cp "target/$target/release/nettest" "package/nettest-linux-$arch/"
        
        # Create README for package
        cat > "package/nettest-linux-$arch/README.txt" << EOF
Nettest - Network Speed Measurement Tool

Architecture: $arch
Build Date: $(date)
Build Type: Static (musl)
Target: $target

Installation:
1. Make binary executable: chmod +x nettest
2. Run: ./nettest -s (server) or ./nettest -c <address> (client)

For more information, visit: https://github.com/your-repo/nettest
EOF
        
        # Create archive
        cd package
        tar -czf "nettest-linux-$arch.tar.gz" "nettest-linux-$arch/"
        cd ..
        
        echo "✓ Built: nettest-linux-$arch.tar.gz"
    else
        echo "✗ Build failed for $arch"
        exit 1
    fi
done

echo "All builds completed successfully!"
echo "Generated packages:"
ls -la package/*.tar.gz
