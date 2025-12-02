# Contributing to ArchDrop

Thank you for your interest in contributing to ArchDrop! This document provides guidelines and instructions for contributing.

## Table of Contents

1. [Code of Conduct](#code-of-conduct)
2. [How Can I Contribute?](#how-can-i-contribute)
3. [Development Setup](#development-setup)
4. [Making Changes](#making-changes)
5. [Coding Standards](#coding-standards)
6. [Testing Guidelines](#testing-guidelines)
7. [Submitting Changes](#submitting-changes)
8. [Review Process](#review-process)

---

## Code of Conduct

### Our Pledge

We are committed to providing a welcoming and inclusive environment for all contributors.

### Expected Behavior

- Be respectful and professional
- Accept constructive criticism gracefully
- Focus on what's best for the project
- Show empathy towards other contributors

### Unacceptable Behavior

- Harassment, discrimination, or personal attacks
- Trolling or inflammatory comments
- Publishing others' private information
- Any conduct that disrupts the community

### Enforcement

Project maintainers may remove, edit, or reject contributions that violate this code of conduct.

---

## How Can I Contribute?

### Reporting Bugs

**Before Submitting**:
1. Check existing issues to avoid duplicates
2. Try latest version to see if bug is fixed
3. Collect information (see template below)

**Bug Report Template**:
```markdown
**Description**
Clear description of the bug

**To Reproduce**
Steps to reproduce:
1. Run `archdrop send file.txt`
2. Open browser to URL
3. Click...
4. See error

**Expected Behavior**
What should happen

**Actual Behavior**
What actually happens

**Environment**
- OS: [e.g., Arch Linux]
- ArchDrop version: [e.g., 0.1.0]
- Rust version: [e.g., 1.75.0]
- Browser: [e.g., Chrome 120]

**Logs**
```
Paste relevant logs here
```

**Additional Context**
Any other relevant information
```

**Submit Issue**: https://github.com/your-username/archdrop/issues/new

### Suggesting Enhancements

**Before Submitting**:
1. Check if enhancement already requested
2. Consider if it fits project scope
3. Think about implementation approach

**Enhancement Template**:
```markdown
**Feature Description**
Clear description of proposed feature

**Motivation**
Why is this feature needed?

**Proposed Solution**
How could this be implemented?

**Alternatives Considered**
Other approaches you've thought about

**Additional Context**
Mockups, examples, or related projects
```

### Writing Documentation

Documentation contributions are always welcome:

- Fix typos or unclear explanations
- Add examples and tutorials
- Improve code comments
- Translate documentation (future)

Areas needing documentation:
- API usage examples
- Common use cases
- Deployment guides
- Performance tuning

### Submitting Code

Code contributions can include:

- Bug fixes
- New features
- Performance improvements
- Code refactoring
- Test additions

See [Making Changes](#making-changes) for details.

---

## Development Setup

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install development dependencies (Debian/Ubuntu)
sudo apt install build-essential libssl-dev pkg-config

# Install optional tools
cargo install cargo-watch cargo-audit cargo-outdated
```

### Fork and Clone

```bash
# Fork on GitHub (click "Fork" button)

# Clone your fork
git clone https://github.com/YOUR_USERNAME/archdrop-beta.git
cd archdrop-beta

# Add upstream remote
git remote add upstream https://github.com/ORIGINAL_OWNER/archdrop-beta.git
```

### Build and Test

```bash
# Build
cargo build

# Run tests
cargo test

# Run locally
cargo run -- send test_file.txt --local
```

---

## Making Changes

### 1. Create a Branch

```bash
# Update main
git checkout main
git pull upstream main

# Create feature branch
git checkout -b feature/my-feature
# Or for bug fix
git checkout -b fix/bug-description
```

**Branch Naming**:
- `feature/` - New features
- `fix/` - Bug fixes
- `docs/` - Documentation only
- `refactor/` - Code refactoring
- `test/` - Test additions
- `perf/` - Performance improvements

### 2. Make Your Changes

**Follow these principles**:
- Keep changes focused and atomic
- One feature/fix per branch
- Write clear, self-documenting code
- Add tests for new functionality
- Update documentation as needed

### 3. Write Tests

Every code change should include tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_feature() {
        // Arrange
        let input = setup_test_data();

        // Act
        let result = my_feature(input);

        // Assert
        assert_eq!(result, expected);
    }

    #[tokio::test]
    async fn test_async_feature() {
        let result = async_feature().await;
        assert!(result.is_ok());
    }
}
```

### 4. Update Documentation

If your change affects:
- **Public API**: Update `docs/API.md`
- **Security**: Update `docs/SECURITY.md`
- **Architecture**: Update `docs/ARCHITECTURE.md`
- **Usage**: Update `README.md`
- **Code**: Add/update inline comments

### 5. Test Your Changes

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_my_feature

# Run with output
cargo test -- --nocapture

# Check formatting
cargo fmt --check

# Run linter
cargo clippy -- -D warnings

# Build release
cargo build --release
```

---

## Coding Standards

### Rust Style Guide

Follow the [Rust Style Guide](https://doc.rust-lang.org/nightly/style-guide/).

**Key Points**:
- Use `cargo fmt` for automatic formatting
- Follow naming conventions (snake_case, CamelCase)
- Prefer explicit over implicit
- Write idiomatic Rust code

### Code Organization

**Module Structure**:
```rust
// src/module.rs

// Imports
use std::path::PathBuf;
use crate::types::EncryptionKey;

// Constants
const CHUNK_SIZE: u64 = 1024 * 1024;

// Types
pub struct MyStruct {
    field: String,
}

// Implementations
impl MyStruct {
    pub fn new() -> Self {
        Self { field: String::new() }
    }

    pub fn method(&self) -> Result<()> {
        Ok(())
    }
}

// Tests
#[cfg(test)]
mod tests {
    use super::*;
    // Test implementations
}
```

### Naming Conventions

| Item | Convention | Example |
|------|-----------|---------|
| Functions | snake_case | `encrypt_chunk()` |
| Types | CamelCase | `EncryptionKey` |
| Constants | SCREAMING_SNAKE_CASE | `CHUNK_SIZE` |
| Modules | snake_case | `mod crypto;` |
| Lifetimes | short, lowercase | `'a`, `'static` |

### Documentation Comments

**Public Items**:
```rust
/// Encrypts a chunk at a specific position.
///
/// # Arguments
///
/// * `cipher` - The AES-GCM cipher instance
/// * `nonce_base` - The 7-byte nonce base
/// * `plaintext` - The data to encrypt
/// * `counter` - The chunk position counter
///
/// # Returns
///
/// Encrypted bytes with authentication tag
///
/// # Errors
///
/// Returns error if encryption fails
///
/// # Examples
///
/// ```
/// let encrypted = encrypt_chunk_at_position(&cipher, &nonce, &data, 0)?;
/// ```
pub fn encrypt_chunk_at_position(
    cipher: &Aes256Gcm,
    nonce_base: &Nonce,
    plaintext: &[u8],
    counter: u32,
) -> Result<Vec<u8>> {
    // Implementation
}
```

**Private Implementation Details**:
```rust
// Construct full 12-byte nonce from base + counter
let mut full_nonce = [0u8; 12];
full_nonce[..7].copy_from_slice(nonce_base.as_bytes());
full_nonce[7..11].copy_from_slice(&counter.to_be_bytes());
```

### Error Handling

**Use `Result` Type**:
```rust
pub fn process() -> Result<()> {
    let data = read_file()?;  // Propagate errors
    validate_data(&data)?;
    Ok(())
}
```

**Provide Context**:
```rust
use anyhow::Context;

let file = tokio::fs::File::open(&path)
    .await
    .context(format!("Failed to open file: {}", path.display()))?;
```

**Handle Errors Appropriately**:
```rust
// Don't ignore errors
let _ = file.close();  // BAD

// Handle explicitly
if let Err(e) = file.close() {
    eprintln!("Warning: failed to close file: {}", e);
}
```

### Safety and Security

**Avoid `unsafe`**:
```rust
// Prefer safe alternatives
// Only use unsafe when absolutely necessary and well-documented
```

**Validate Inputs**:
```rust
pub fn process_path(path: &str) -> Result<()> {
    validate_path(path)?;  // Prevent path traversal
    // Process path
}
```

**Use Secure Defaults**:
```rust
// Use OS RNG for cryptographic keys
let mut key = [0u8; 32];
OsRng::default().fill_bytes(&mut key);
```

---

## Testing Guidelines

### Test Categories

**Unit Tests**:
- Test individual functions in isolation
- Place in same file as code (`#[cfg(test)]`)
- Fast execution

**Integration Tests**:
- Test module interactions
- Place in `tests/` directory
- May be slower

### Writing Good Tests

**Structure** (Arrange-Act-Assert):
```rust
#[test]
fn test_session_claiming() {
    // Arrange: Set up test data
    let (session, token) = Session::new_send(manifest, key, nonce);

    // Act: Perform action
    let result = session.claim(&token);

    // Assert: Verify results
    assert!(result);
    assert!(session.is_active(&token));
}
```

**Test Names**:
```rust
// Good: Descriptive test names
#[test]
fn test_claim_succeeds_on_first_attempt() { }

#[test]
fn test_claim_fails_on_second_attempt() { }

// Bad: Vague names
#[test]
fn test1() { }
```

**Coverage Goals**:
- New code: Aim for 80%+ coverage
- Critical paths: 100% coverage (crypto, auth)
- Happy path and error cases

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_encryption

# Show output
cargo test -- --nocapture

# Parallel execution
cargo test -- --test-threads=4

# Coverage
cargo install cargo-tarpaulin
cargo tarpaulin --out Html
```

---

## Submitting Changes

### 1. Commit Your Changes

**Commit Message Format**:
```
<type>: <subject>

<body>

<footer>
```

**Types**:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation only
- `refactor`: Code restructuring
- `test`: Adding tests
- `perf`: Performance improvement
- `chore`: Maintenance tasks

**Example**:
```
feat: Add resumable transfer support

Implements chunk tracking to allow resuming interrupted transfers.
Each chunk is stored with metadata about its status (pending,
received, finalized).

Closes #42
```

**Commit Guidelines**:
- Use present tense ("Add feature" not "Added feature")
- Use imperative mood ("Move cursor to..." not "Moves cursor to...")
- Limit subject line to 50 characters
- Wrap body at 72 characters
- Reference issues and PRs

### 2. Push to Your Fork

```bash
# Push feature branch
git push origin feature/my-feature
```

### 3. Create Pull Request

**On GitHub**:
1. Navigate to your fork
2. Click "Pull Request"
3. Select your branch
4. Fill out template

**PR Template**:
```markdown
## Description
What does this PR do?

## Type of Change
- [ ] Bug fix
- [ ] New feature
- [ ] Breaking change
- [ ] Documentation update

## Testing
How was this tested?

## Checklist
- [ ] Code follows style guidelines
- [ ] Self-review completed
- [ ] Tests added
- [ ] Documentation updated
- [ ] All tests pass
- [ ] No new warnings

## Related Issues
Closes #123
```

### 4. Respond to Feedback

- Be open to suggestions
- Make requested changes promptly
- Ask questions if unclear
- Push additional commits to same branch

---

## Review Process

### What Reviewers Look For

**Code Quality**:
- Follows Rust idioms
- Clear and readable
- Appropriate abstractions
- No unnecessary complexity

**Correctness**:
- Logic is sound
- Edge cases handled
- No obvious bugs

**Testing**:
- Adequate test coverage
- Tests are meaningful
- Tests pass consistently

**Security**:
- No vulnerabilities introduced
- Input validation present
- Secure coding practices

**Documentation**:
- Public APIs documented
- Complex logic explained
- Examples provided

### Review Timeline

- **Initial Response**: Within 48 hours
- **Full Review**: Within 1 week
- **Follow-up**: Within 2-3 days

### Approval Criteria

PR must have:
- [ ] At least one approval from maintainer
- [ ] All tests passing
- [ ] No unresolved comments
- [ ] Clean commit history
- [ ] Up-to-date with main branch

### After Approval

1. Maintainer will merge PR
2. PR branch can be deleted
3. Contribution appears in next release
4. Contributor added to CONTRIBUTORS file (if not already present)

---

## Areas Needing Contributions

### High Priority

- [ ] **Resumable Transfers**: Track chunks, allow reconnection
- [ ] **Progress Persistence**: Save transfer state to disk
- [ ] **Multi-Recipient**: Broadcast to multiple clients
- [ ] **Compression**: Optional zstd compression before encryption

### Medium Priority

- [ ] **Subresource Integrity**: JavaScript integrity verification
- [ ] **Content Security Policy**: Add CSP headers
- [ ] **Logging**: Structured logging with tracing
- [ ] **Metrics**: Prometheus metrics export

### Low Priority

- [ ] **GUI**: Electron or Tauri-based GUI
- [ ] **Mobile Support**: Android app (receive only)
- [ ] **Plugin System**: Extensibility for transforms
- [ ] **P2P Discovery**: mDNS/zeroconf discovery

### Documentation

- [ ] Video tutorials
- [ ] Deployment guides (Docker, systemd)
- [ ] Performance tuning guide
- [ ] Translations (internationalization)

---

## Community

### Communication Channels

- **GitHub Issues**: Bug reports and feature requests
- **GitHub Discussions**: Q&A and general discussion
- **Email**: (if provided)

### Recognition

Contributors will be:
- Added to CONTRIBUTORS file
- Mentioned in release notes
- Credited in documentation (if desired)

### Becoming a Maintainer

Active contributors may be invited to become maintainers. Criteria:
- Consistent high-quality contributions
- Understanding of project goals
- Good communication skills
- Willingness to review PRs

---

## License

By contributing, you agree that your contributions will be licensed under the MIT License (same as the project).

---

## Questions?

If you have questions about contributing:
1. Check this document
2. Search existing issues
3. Ask in GitHub Discussions
4. Contact maintainers

Thank you for contributing to ArchDrop!
