#!/bin/bash
set -e

echo "🚀 Building Nettest with Docker and Cross-compilation"

# Check if Docker is available
if ! command -v docker &> /dev/null; then
    echo "❌ Docker is not installed. Please install Docker first."
    exit 1
fi

# Build Docker image
echo "📦 Building Docker image..."
docker build -f Dockerfile.build -t nettest-builder .

# Run builds
echo "🔨 Running cross-compilation builds..."
docker run --rm -v $(pwd):/app -w /app nettest-builder /usr/local/bin/build.sh

echo "✅ Build completed successfully!"
echo "📁 Generated packages:"
ls -la package/*.tar.gz

echo ""
echo "🎉 All builds are ready in the 'package/' directory!" 