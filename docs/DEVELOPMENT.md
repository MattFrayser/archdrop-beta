# ArchDrop Development Guide

Complete guide for developers working on ArchDrop.

## Table of Contents

1. [Getting Started](#getting-started)
2. [Development Environment](#development-environment)
3. [Building](#building)
4. [Testing](#testing)
5. [Debugging](#debugging)
6. [Code Organization](#code-organization)
7. [Development Workflow](#development-workflow)
8. [Performance Profiling](#performance-profiling)
9. [Release Process](#release-process)

---

## Getting Started

### Prerequisites

**Required**:
- Rust 1.70+ (stable toolchain)
- Linux operating system
- Git

**Optional**:
- `cloudflared` (for tunnel mode testing)
- `rust-analyzer` (for IDE integration)
- `cargo-watch` (for automatic rebuilds)

### Clone Repository

```bash
git clone https://github.com/your-username/archdrop-beta.git
cd archdrop-beta
```

### Quick Start

```bash
# Build in debug mode
cargo build

# Run in debug mode
cargo run -- send test_file.txt --local

# Build and run in release mode
cargo build --release
./target/release/archdrop send test_file.txt --local
```

---

## Development Environment

### IDE Setup

#### VS Code

**Install Extensions**:
- [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
- [CodeLLDB](https://marketplace.visualstudio.com/items?itemName=vadimcn.vscode-lldb) (debugger)
- [crates](https://marketplace.visualstudio.com/items?itemName=serayuzgur.crates) (dependency management)
- [Error Lens](https://marketplace.visualstudio.com/items?itemName=usernamehw.errorlens) (inline errors)

**Settings** (.vscode/settings.json):
```json
{
  "rust-analyzer.cargo.features": "all",
  "rust-analyzer.check.command": "clippy",
  "rust-analyzer.check.allTargets": true,
  "editor.formatOnSave": true,
  "[rust]": {
    "editor.defaultFormatter": "rust-lang.rust-analyzer"
  }
}
```

**Launch Configuration** (.vscode/launch.json):
```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "Debug Send Mode",
      "cargo": {
        "args": ["build", "--bin=archdrop"],
        "filter": {
          "name": "archdrop",
          "kind": "bin"
        }
      },
      "args": ["send", "test_file.txt", "--local"],
      "cwd": "${workspaceFolder}"
    }
  ]
}
```

#### IntelliJ IDEA / CLion

**Install Plugin**:
- IntelliJ Rust

**Configuration**:
- File → Settings → Languages & Frameworks → Rust
- Check "Use rustfmt instead of built-in formatter"
- Check "Run Clippy on file save"

### Development Tools

**Install Useful Cargo Tools**:
```bash
# Automatic rebuilds on file changes
cargo install cargo-watch

# Dependency tree visualization
cargo install cargo-tree

# Outdated dependency checking
cargo install cargo-outdated

# Security audit
cargo install cargo-audit

# Benchmarking
cargo install cargo-criterion

# Code coverage
cargo install cargo-tarpaulin

# Unused dependency detection
cargo install cargo-udeps

# Generate call graphs (requires graphviz)
cargo install cargo-call-stack
```

---

## Building

### Debug Build

```bash
cargo build
```

**Characteristics**:
- Fast compilation (~30-60 seconds)
- Includes debug symbols
- No optimizations
- Larger binary (~50MB)
- Slower runtime performance

**Use Cases**:
- Development
- Debugging
- Testing

### Release Build

```bash
cargo build --release
```

**Characteristics**:
- Slower compilation (~2-5 minutes)
- Optimizations enabled
- Smaller binary (~10-15MB)
- Fast runtime performance

**Use Cases**:
- Production deployment
- Performance testing
- Distribution

### Build Profiles

**Custom Profile** (Cargo.toml):
```toml
[profile.dev]
opt-level = 0           # No optimization
debug = true            # Full debug info
overflow-checks = true  # Integer overflow checks

[profile.release]
opt-level = 3           # Max optimization
debug = false           # No debug info
lto = "fat"             # Link-time optimization
codegen-units = 1       # Single codegen unit (slower build, faster binary)
strip = true            # Strip symbols
panic = "abort"         # Abort on panic (smaller binary)
```

### Cross-Compilation

**For Different Linux Architectures**:

```bash
# Install target
rustup target add x86_64-unknown-linux-musl

# Build with musl (static linking)
cargo build --release --target x86_64-unknown-linux-musl
```

**For ARM**:
```bash
rustup target add aarch64-unknown-linux-gnu
cargo build --release --target aarch64-unknown-linux-gnu
```

### Build Optimization Tips

**Faster Debug Builds**:
```toml
# Cargo.toml
[profile.dev]
incremental = true  # Incremental compilation (default)

[profile.dev.package."*"]
opt-level = 1  # Optimize dependencies slightly
```

**Faster Link Times** (Linux):
```bash
# Install mold linker
sudo apt install mold  # or build from source

# Use in Cargo config
# ~/.cargo/config.toml
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

---

## Testing

### Run All Tests

```bash
cargo test
```

### Run Specific Test Module

```bash
cargo test crypto_tests
cargo test manifest_tests
cargo test session_tests
```

### Run Single Test

```bash
cargo test test_encrypt_decrypt_chunk
```

### Run with Output

```bash
cargo test -- --nocapture
```

### Run in Parallel

```bash
cargo test -- --test-threads=4
```

### Integration Tests

```bash
cargo test --test '*'
```

### Test Structure

```
tests/
├── crypto_tests.rs         # Encryption/decryption
├── manifest_tests.rs       # File manifest creation
├── session_tests.rs        # Session management
└── traverse/
    └── util.rs             # Path traversal prevention
```

### Writing Tests

**Unit Test Example**:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_key_generation() {
        let key1 = EncryptionKey::new();
        let key2 = EncryptionKey::new();

        // Keys should be different
        assert_ne!(key1.as_bytes(), key2.as_bytes());
    }

    #[tokio::test]
    async fn test_file_transfer() {
        // Async test example
        let result = some_async_function().await;
        assert!(result.is_ok());
    }
}
```

### Test Coverage

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --out Html --output-dir coverage/

# Open coverage/index.html in browser
```

### Benchmarking

```bash
# Run benchmarks (if implemented)
cargo bench

# Benchmark specific function
cargo bench encrypt_chunk
```

---

## Debugging

### Debug Logging

**Add Logging** (using `eprintln!`):
```rust
eprintln!("[DEBUG] Token: {}, Active: {}", token, active);
```

**Use `dbg!` Macro**:
```rust
let result = dbg!(some_function());
// Prints: [src/main.rs:42] some_function() = Ok(...)
```

### GDB Debugging

```bash
# Build with debug symbols
cargo build

# Run with GDB
rust-gdb ./target/debug/archdrop

# Set breakpoint
(gdb) break main.rs:42

# Run with args
(gdb) run send file.txt --local

# Inspect variables
(gdb) print token
(gdb) print *session
```

### LLDB Debugging

```bash
# Run with LLDB
rust-lldb ./target/debug/archdrop

# Set breakpoint
(lldb) breakpoint set --file main.rs --line 42

# Run
(lldb) run send file.txt --local

# Backtrace
(lldb) bt

# Inspect
(lldb) frame variable
(lldb) expr token
```

### VS Code Debugging

1. Set breakpoint (click left of line number)
2. Press F5 or Run → Start Debugging
3. Use Debug Console to inspect variables

### Debugging Async Code

**Tokio Console** (runtime inspection):
```bash
# Add to Cargo.toml
[dependencies]
console-subscriber = "0.1"

# In main.rs
console_subscriber::init();

# Run tokio-console
cargo install tokio-console
tokio-console
```

### Memory Debugging

**Valgrind** (memory leaks):
```bash
cargo build
valgrind --leak-check=full ./target/debug/archdrop send file.txt --local
```

**Heaptrack** (heap profiling):
```bash
heaptrack ./target/release/archdrop send large_file.bin --local
heaptrack_gui heaptrack.archdrop.*.gz
```

### Network Debugging

**Wireshark** (packet capture):
```bash
# Capture HTTPS traffic
sudo wireshark -i lo -f "port 54321"

# Decrypt requires SSL keys (complex)
```

**curl** (API testing):
```bash
# Health check
curl -k https://127.0.0.1:54321/health

# Get manifest
TOKEN="your-token-here"
curl -k "https://127.0.0.1:54321/send/${TOKEN}/manifest"

# Download chunk
curl -k "https://127.0.0.1:54321/send/${TOKEN}/0/chunk/0" -o chunk.enc
```

---

## Code Organization

### Module Structure

```
src/
├── main.rs               # Entry point, CLI parsing
├── lib.rs                # Library root, module declarations
├── types.rs              # Core types (EncryptionKey, Nonce)
├── crypto.rs             # Encryption/decryption functions
├── tunnel.rs             # Cloudflare tunnel management
├── server/
│   ├── mod.rs            # Server initialization, routing
│   ├── session.rs        # Session lifecycle management
│   ├── state.rs          # Shared application state
│   ├── modes.rs          # Local vs Tunnel mode
│   ├── utils.rs          # Helper functions (cert gen, TUI spawn)
│   └── web.rs            # HTML/JS template serving
├── transfer/
│   ├── mod.rs            # Transfer module exports
│   ├── manifest.rs       # File metadata structure
│   ├── send.rs           # Send mode HTTP handlers
│   ├── receive.rs        # Receive mode HTTP handlers
│   ├── storage.rs        # Chunk storage, RAII cleanup
│   └── util.rs           # Path validation, hashing
└── ui/
    ├── mod.rs            # UI module exports
    ├── tui.rs            # Terminal UI (ratatui)
    ├── qr.rs             # QR code generation
    └── output.rs         # Console output helpers
```

### Adding a New Module

1. **Create File**:
   ```bash
   touch src/mymodule.rs
   ```

2. **Declare in lib.rs**:
   ```rust
   pub mod mymodule;
   ```

3. **Implement**:
   ```rust
   // src/mymodule.rs
   pub fn my_function() {
       // Implementation
   }
   ```

4. **Use in Other Modules**:
   ```rust
   use crate::mymodule::my_function;
   ```

### Dependency Management

**Add Dependency**:
```bash
cargo add tokio --features full
```

**Or manually edit Cargo.toml**:
```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
```

**Update Dependencies**:
```bash
cargo update
```

**Check for Outdated**:
```bash
cargo outdated
```

**Security Audit**:
```bash
cargo audit
```

---

## Development Workflow

### Typical Development Cycle

1. **Create Branch**:
   ```bash
   git checkout -b feature/my-feature
   ```

2. **Make Changes**: Edit code

3. **Check Format**:
   ```bash
   cargo fmt --check
   ```

4. **Run Linter**:
   ```bash
   cargo clippy -- -D warnings
   ```

5. **Run Tests**:
   ```bash
   cargo test
   ```

6. **Build**:
   ```bash
   cargo build --release
   ```

7. **Manual Testing**:
   ```bash
   ./target/release/archdrop send file.txt --local
   ```

8. **Commit**:
   ```bash
   git add .
   git commit -m "Add my feature"
   ```

9. **Push**:
   ```bash
   git push origin feature/my-feature
   ```

10. **Open Pull Request**: On GitHub

### Continuous Development

**Watch Mode** (auto-rebuild on changes):
```bash
cargo watch -x 'build'
```

**Watch and Test**:
```bash
cargo watch -x 'test'
```

**Watch, Build, and Run**:
```bash
cargo watch -x 'run -- send file.txt --local'
```

### Code Style

**Auto-Format**:
```bash
cargo fmt
```

**Format Check** (CI):
```bash
cargo fmt --check
```

**Clippy** (linter):
```bash
cargo clippy
```

**Clippy Strict**:
```bash
cargo clippy -- -D warnings -D clippy::all -D clippy::pedantic
```

### Git Hooks

**Pre-Commit Hook** (.git/hooks/pre-commit):
```bash
#!/bin/bash
cargo fmt --check || exit 1
cargo clippy -- -D warnings || exit 1
cargo test || exit 1
```

Make executable:
```bash
chmod +x .git/hooks/pre-commit
```

---

## Performance Profiling

### CPU Profiling

**perf** (Linux):
```bash
# Record
cargo build --release
perf record --call-graph dwarf ./target/release/archdrop send large_file.bin --local

# Report
perf report

# Flamegraph
cargo install flamegraph
cargo flamegraph --bin archdrop -- send large_file.bin --local
# Open flamegraph.svg in browser
```

**Instruments** (macOS):
```bash
# Run with Instruments
instruments -t "Time Profiler" ./target/release/archdrop send file.txt --local
```

### Memory Profiling

**Heaptrack**:
```bash
heaptrack ./target/release/archdrop send large_file.bin --local
heaptrack_gui heaptrack.archdrop.*.gz
```

**Valgrind Massif** (heap profiler):
```bash
valgrind --tool=massif ./target/release/archdrop send file.txt --local
ms_print massif.out.*
```

### Benchmarking

**Criterion** (micro-benchmarks):
```rust
// benches/crypto_bench.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use archdrop::crypto::encrypt_chunk_at_position;

fn benchmark_encryption(c: &mut Criterion) {
    let cipher = /* ... */;
    let nonce = /* ... */;
    let data = vec![0u8; 1024 * 1024]; // 1MB

    c.bench_function("encrypt_1mb", |b| {
        b.iter(|| {
            encrypt_chunk_at_position(black_box(&cipher), black_box(&nonce), black_box(&data), 0)
        });
    });
}

criterion_group!(benches, benchmark_encryption);
criterion_main!(benches);
```

Run:
```bash
cargo bench
```

### Bottleneck Analysis

**Identify Bottlenecks**:
1. Profile with `perf` or `flamegraph`
2. Look for "hot" functions (high % time)
3. Focus optimization efforts there

**Common Bottlenecks**:
- Encryption: CPU-bound, consider hardware acceleration
- Disk I/O: Use buffered readers/writers
- Network: Increase parallelism (more concurrent chunks)
- Serialization: Use efficient formats (bincode vs JSON)

---

## Release Process

### Version Bump

**Update Cargo.toml**:
```toml
[package]
version = "0.2.0"
```

### Generate Changelog

```markdown
## [0.2.0] - 2024-01-15

### Added
- Resumable transfers
- Progress persistence

### Fixed
- Memory leak in chunk storage
- Race condition in session claiming

### Changed
- Increased default chunk size to 2MB
```

### Build Release Binaries

```bash
# Native build
cargo build --release

# Strip symbols
strip target/release/archdrop

# Verify size
ls -lh target/release/archdrop
```

### Create Release Tag

```bash
git tag -a v0.2.0 -m "Release version 0.2.0"
git push origin v0.2.0
```

### Generate Checksums

```bash
sha256sum target/release/archdrop > archdrop-v0.2.0-linux-x86_64.sha256
```

### GitHub Release

1. Go to GitHub → Releases → Draft a new release
2. Choose tag: v0.2.0
3. Release title: v0.2.0
4. Description: Paste changelog
5. Upload binaries:
   - `archdrop-v0.2.0-linux-x86_64`
   - `archdrop-v0.2.0-linux-x86_64.sha256`
6. Publish release

### Distribution

**Cargo** (publish to crates.io):
```bash
cargo login
cargo publish
```

**AUR** (Arch User Repository):
```bash
# Create PKGBUILD
# Submit to AUR
```

**Homebrew** (macOS):
```ruby
# Create formula
# Submit PR to homebrew-core
```

---

## Common Development Tasks

### Adding a New Endpoint

1. **Define Handler**:
   ```rust
   // src/transfer/send.rs
   pub async fn new_endpoint(
       Path(token): Path<String>,
       State(state): State<AppState>,
   ) -> Result<Json<Value>, AppError> {
       // Implementation
       Ok(Json(json!({"success": true})))
   }
   ```

2. **Register Route**:
   ```rust
   // src/server/mod.rs
   let app = Router::new()
       .route("/send/:token/new", get(send::new_endpoint))
       .with_state(state);
   ```

3. **Test**:
   ```rust
   #[tokio::test]
   async fn test_new_endpoint() {
       // Test implementation
   }
   ```

### Modifying Crypto

1. **Update Function**:
   ```rust
   // src/crypto.rs
   pub fn encrypt_chunk_at_position(/* ... */) -> Result<Vec<u8>> {
       // Modified implementation
   }
   ```

2. **Run Crypto Tests**:
   ```bash
   cargo test crypto_tests
   ```

3. **Verify Security**: Review with security expert

### Adding Configuration Option

1. **Add to CLI**:
   ```rust
   // src/main.rs
   #[derive(Parser)]
   struct Cli {
       #[arg(long, default_value = "1048576")]
       chunk_size: u64,
   }
   ```

2. **Use in Code**:
   ```rust
   let config = Config { chunk_size: cli.chunk_size };
   ```

3. **Update Documentation**: README, API docs

---

## Troubleshooting Development Issues

### Build Errors

**Error**: `error: linker 'cc' not found`
```bash
# Install build-essential
sudo apt install build-essential
```

**Error**: `error: failed to run custom build command for openssl-sys`
```bash
# Install OpenSSL development files
sudo apt install libssl-dev pkg-config
```

### Test Failures

**Flaky Tests** (timing issues):
```rust
// Add retry logic or increase timeouts
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_with_single_thread() {
    // Test implementation
}
```

**Tests Hang**:
- Check for deadlocks
- Ensure async runtime is spawned
- Use `timeout` wrapper

### IDE Issues

**rust-analyzer Slow**:
```bash
# Clear cache
rm -rf ~/.cache/rust-analyzer

# Restart rust-analyzer
```

**Formatting Not Working**:
```bash
# Install rustfmt
rustup component add rustfmt
```

---

## Resources

### Documentation

- [Rust Book](https://doc.rust-lang.org/book/)
- [Rust by Example](https://doc.rust-lang.org/rust-by-example/)
- [Tokio Tutorial](https://tokio.rs/tokio/tutorial)
- [Axum Documentation](https://docs.rs/axum/)

### Tools

- [crates.io](https://crates.io/) - Rust package registry
- [docs.rs](https://docs.rs/) - Rust documentation
- [rust-analyzer](https://rust-analyzer.github.io/) - IDE support
- [rustfmt](https://github.com/rust-lang/rustfmt) - Code formatter
- [clippy](https://github.com/rust-lang/rust-clippy) - Linter

### Community

- [Rust Users Forum](https://users.rust-lang.org/)
- [Rust Discord](https://discord.gg/rust-lang)
- [r/rust](https://www.reddit.com/r/rust/)

---

## FAQ

**Q: How long does a full rebuild take?**
A: ~2-5 minutes for release build, ~30-60 seconds for debug build.

**Q: Can I develop on Windows?**
A: ArchDrop targets Linux. Use WSL2 for Windows development.

**Q: How do I add a new dependency?**
A: `cargo add <crate>` or edit Cargo.toml and run `cargo build`.

**Q: Tests are failing, what do I do?**
A: Run `cargo test -- --nocapture` to see output. Check for timing issues or missing test files.

**Q: How do I profile performance?**
A: Use `perf` on Linux or `flamegraph` for visual profiling.

**Q: Can I use a different async runtime?**
A: Possible but not recommended. ArchDrop is built on Tokio.

For more questions, see [TROUBLESHOOTING.md](TROUBLESHOOTING.md) or open an issue.
