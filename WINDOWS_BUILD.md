# Windows Build Instructions

## Overview

This document describes how to build the nettest binary for Windows platforms using the GitHub Actions workflow.

## Supported Architectures

- **x86_64**: 64-bit Intel/AMD processors
- **aarch64**: 64-bit ARM processors (Windows on ARM)

## Automated Build

The Windows build is automatically triggered on:
- Push to `main` branch
- Push to `develop` branch
- Creation of version tags (`v*`)

## Workflow Jobs

### 1. build-windows-x86_64
- **Runner**: `windows-latest`
- **Target**: `x86_64-pc-windows-msvc`
- **Output**: `nettest.exe` for x86_64 Windows

### 2. build-windows-aarch64
- **Runner**: `windows-latest`
- **Target**: `aarch64-pc-windows-msvc`
- **Output**: `nettest.exe` for ARM64 Windows

### 3. create-windows-release
- **Runner**: `windows-latest`
- **Dependencies**: Both build jobs must complete successfully
- **Output**: GitHub release with ZIP archives

## Build Artifacts

### Individual Builds
- `nettest-windows-x86_64-latest`: ZIP package with x86_64 binary
- `nettest-windows-aarch64-latest`: ZIP package with ARM64 binary

### Release Assets
- `nettest-windows-x86_64.zip`: Standalone x86_64 executable
- `nettest-windows-aarch64.zip`: Standalone ARM64 executable

## Local Build Instructions

If you want to build locally on Windows:

### Prerequisites
1. Install Rust: https://rustup.rs/
2. Install Visual Studio Build Tools (for MSVC toolchain)

### Build Commands

#### For x86_64:
```cmd
rustup target add x86_64-pc-windows-msvc
cargo build --release --target x86_64-pc-windows-msvc
```

#### For ARM64:
```cmd
rustup target add aarch64-pc-windows-msvc
cargo build --release --target aarch64-pc-windows-msvc
```

### Using Cross (Cross-Compilation)

If building from Linux/macOS for Windows:

```bash
# Install cross
cargo install cross

# Build for Windows x86_64
cross build --release --target x86_64-pc-windows-msvc

# Build for Windows ARM64
cross build --release --target aarch64-pc-windows-msvc
```

## Package Contents

Each Windows package contains:
- `nettest.exe`: The main executable
- `README.txt`: Installation and usage instructions

## Usage

### Server Mode
```cmd
nettest.exe -s
```

### Client Mode
```cmd
nettest.exe -c <server-address>
```

## Troubleshooting

### Common Issues

1. **Missing MSVC Toolchain**
   - Solution: Install Visual Studio Build Tools
   - Run: `rustup toolchain install stable-msvc`

2. **OpenSSL Issues**
   - The build uses vendored OpenSSL, so no external OpenSSL installation is required

3. **Build Failures**
   - Check that all dependencies are properly installed
   - Ensure you're using the correct target triple
   - Verify Rust toolchain is up to date

### Support

For build issues or questions:
- Check the GitHub Actions logs
- Review the workflow configuration in `.github/workflows/build-windows.yml`
- Open an issue in the repository

## Cross-Platform Compatibility

The Windows binaries are built with:
- **Toolchain**: MSVC (Microsoft Visual C++)
- **Runtime**: Windows 10+ compatible
- **Dependencies**: Statically linked where possible

## Release Process

1. Push code to `main` or `develop` branch
2. GitHub Actions automatically triggers Windows build
3. Build artifacts are uploaded
4. Release is created with tag `latest-windows`
5. ZIP files are attached to the release

## Performance Notes

- Windows builds may take longer than Linux builds due to MSVC toolchain
- ARM64 builds require additional setup and may have longer build times
- Consider using cross-compilation from Linux for faster builds
