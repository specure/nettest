#!/bin/bash

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Function to check if command exists
command_exists() {
    command -v "$1" >/dev/null 2>&1
}

# Function to install dependencies based on OS
install_dependencies() {
    print_status "Checking and installing dependencies..."
    
    # Detect OS
    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS
        print_status "Detected macOS"
        
        # Check if Homebrew is installed
        if ! command_exists brew; then
            print_status "Installing Homebrew..."
            /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
        fi
        
        # Install Rust if not present
        if ! command_exists rustc; then
            print_status "Installing Rust..."
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
            source ~/.cargo/env
        fi
        
        # Install OpenSSL development libraries
        if ! brew list openssl@3 >/dev/null 2>&1; then
            print_status "Installing OpenSSL..."
            brew install openssl@3
        fi
        
        # Install pkg-config if not present
        if ! command_exists pkg-config; then
            print_status "Installing pkg-config..."
            brew install pkg-config
        fi
        
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        # Linux
        print_status "Detected Linux"
        
        # Detect package manager
        if command_exists apt-get; then
            # Debian/Ubuntu
            print_status "Using apt package manager"
            sudo apt-get update
            sudo apt-get install -y build-essential curl pkg-config libssl-dev
        elif command_exists yum; then
            # CentOS/RHEL
            print_status "Using yum package manager"
            sudo yum groupinstall -y "Development Tools"
            sudo yum install -y curl pkg-config openssl-devel
        elif command_exists dnf; then
            # Fedora
            print_status "Using dnf package manager"
            sudo dnf groupinstall -y "Development Tools"
            sudo dnf install -y curl pkg-config openssl-devel
        else
            print_error "Unsupported Linux distribution. Please install build-essential, curl, pkg-config, and libssl-dev manually."
            exit 1
        fi
        
        # Install Rust if not present
        if ! command_exists rustc; then
            print_status "Installing Rust..."
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
            source ~/.cargo/env
        fi
        
    else
        print_error "Unsupported operating system: $OSTYPE"
        exit 1
    fi
    
    print_success "Dependencies check completed"
}

# Function to clone repository
clone_repository() {
    local repo_url="https://github.com/specure/measurement-server.git"
    local branch="client"
    local target_dir="measurement-server"
    
    print_status "Cloning repository from $repo_url (branch: $branch)..."
    
    # Remove existing directory if it exists
    if [ -d "$target_dir" ]; then
        print_warning "Directory $target_dir already exists. Removing..."
        rm -rf "$target_dir"
    fi
    
    # Check if git is available
    if command_exists git; then
        # Clone repository using git
        if git clone -b "$branch" "$repo_url" "$target_dir"; then
            print_success "Repository cloned successfully using git"
            cd "$target_dir"
        else
            print_error "Failed to clone repository with git"
            exit 1
        fi
    else
        print_warning "Git not found, downloading archive from GitHub..."
        
        # Download archive using curl or wget
        local download_url="https://github.com/specure/measurement-server/archive/refs/heads/$branch.tar.gz"
        local archive_name="$branch.tar.gz"
        
        if command_exists curl; then
            print_status "Downloading using curl..."
            if curl -L -o "$archive_name" "$download_url"; then
                print_success "Archive downloaded successfully"
            else
                print_error "Failed to download archive with curl"
                exit 1
            fi
        elif command_exists wget; then
            print_status "Downloading using wget..."
            if wget -O "$archive_name" "$download_url"; then
                print_success "Archive downloaded successfully"
            else
                print_error "Failed to download archive with wget"
                exit 1
            fi
        else
            print_error "Neither git, curl, nor wget found. Please install one of them."
            exit 1
        fi
        
        # Extract archive
        print_status "Extracting archive..."
        if tar -xzf "$archive_name"; then
            print_success "Archive extracted successfully"
            # Rename directory to match expected name
            mv "measurement-server-$branch" "$target_dir"
            cd "$target_dir"
            # Clean up archive
            rm -f "../$archive_name"
        else
            print_error "Failed to extract archive"
            exit 1
        fi
    fi
}

# Function to build the project
build_project() {
    print_status "Building the project..."
    
    # Update Rust toolchain
    print_status "Updating Rust toolchain..."
    rustup update
    
    # Build the project
    print_status "Running cargo build..."
    if  RUSTFLAGS="-C target-cpu=native" cargo build --release; then
        print_success "Project built successfully!"
        print_status "Binary location: target/release/rmbt_server"
    else
        print_error "Build failed"
        exit 1
    fi
}

# Function to copy binaries from target
copy_binaries() {
    print_status "Copying binaries from target directory..."
    
    # Create bin directory if it doesn't exist
    if [ ! -d "bin" ]; then
        mkdir -p bin
        print_status "Created bin directory"
    fi
    
    # Copy release binaries
    if [ -f "target/release/nettest" ]; then
        cp target/release/nettest bin/
        print_success "Copied nettest to bin/"
    else
        print_warning "nettest binary not found in target/release/"
    fi
    
    # Make binaries executable
    chmod +x bin/*
    print_success "Binaries copied and made executable"
    print_status "Binaries are now available in: $(pwd)/bin/"
}

# Main execution
main() {
    print_status "Starting build process..."
    
    # Install dependencies
    install_dependencies
    
    # Clone repository
    clone_repository
    
    # Build project
    build_project
    
    # Copy binaries
    copy_binaries
    
    print_success "Build process completed successfully!"
}

# Run main function
main "$@"
