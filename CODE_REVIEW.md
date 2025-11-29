# ArchDrop: Comprehensive Code Review

## Executive Summary

ArchDrop is a well-structured proof-of-concept for secure file transfer with a clean zero-knowledge architecture. The codebase demonstrates solid understanding of Rust, async programming, and cryptography. However, there are significant gaps in production readiness around error handling, resource management, and security hardening.

**Overall Assessment: 6.5/10**

**Strengths:**
- Clean separation of concerns
- Proper use of Rust's type system
- Zero-knowledge encryption architecture
- Streaming implementation for memory efficiency

**Weaknesses:**
- Security vulnerabilities (see detailed analysis)
- Missing resource cleanup and error recovery
- Inconsistent chunk sizes and design decisions
- No rate limiting or DoS protection
- Hardcoded paths and assumptions

---

## SECURITY ISSUES (Critical)

### 1. Session Validation Race Condition

**Location:** `src/server/session.rs:57-64`

```rust
pub async fn mark_used(&self) {
    *self.used.lock().await = true;
}

pub async fn is_valid(&self, token: &str) -> bool {
    token == self.token && !*self.used.lock().await
}
```

**Issue:** The check-and-use pattern is not atomic. A race condition exists between:
1. `is_valid()` check in handler
2. `mark_used()` call later in the flow

**Impact:** Multiple concurrent requests could pass validation before the session is marked used.

**Fix:**
```rust
pub async fn mark_used_if_valid(&self, token: &str) -> bool {
    let mut used = self.used.lock().await;
    if token == self.token && !*used {
        *used = true;
        true
    } else {
        false
    }
}
```

**YAGNI Violation:** You don't need separate `is_valid()` and `mark_used()`. Combine them atomically.

---

### 2. URL Fragment Key Leakage

**Location:** `templates/shared/crypto.js:41-52`

```javascript
async function getCredentialsFromUrl() {
    const fragment = window.location.hash.substring(1)
    const params = new URLSearchParams(fragment)
    const keyBase64 = params.get('key')

    // Clear url fragment immediatly after getti
    // window.location.replace(window.location.href.split('#')[0])
```

**Issue:** Fragment clearing is commented out. Encryption keys persist in:
- Browser history
- Referer headers (if user navigates away)
- Browser session storage
- DevTools console

**Impact:** Keys can leak to third parties or persist after transfer.

**Fix:** Uncomment the fragment clearing AND add immediate cleanup:
```javascript
const key = params.get('key')
const nonce = params.get('nonce')

// Clear immediately
history.replaceState(null, '', window.location.href.split('#')[0])
window.location.hash = ''
```

---

### 3. Hardcoded /tmp Directory Without Permissions Control

**Location:** `src/transfer/chunk.rs:66`, `receive.rs:112`

```rust
let chunk_dir = format!("/tmp/archdrop/{}/{}", token, file_id);
tokio::fs::create_dir_all(&chunk_dir).await?;
```

**Issues:**
- No permission checks on directory creation
- `/tmp` may not exist on all systems
- Other users can read files in `/tmp/archdrop` (default umask)
- No cleanup on errors or crashes

**Impact:**
- Encrypted chunks may be world-readable
- Disk space exhaustion
- Symlink attacks possible

**Fix:**
```rust
use std::os::unix::fs::DirBuilderExt;

let temp_base = std::env::temp_dir().join("archdrop");
let mut builder = tokio::fs::DirBuilder::new();
builder.mode(0o700); // Owner only
builder.create_all(&temp_base).await?;
```

**SOLID Violation (SRP):** Temp directory logic scattered across multiple files. Should be centralized.

---

### 4. No CSRF Protection

**Location:** All POST handlers in `receive.rs`

```rust
pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<axum::Json<Value>, AppError> {
```

**Issue:** No CSRF tokens. Any website can POST to the upload endpoint if they know the token.

**Impact:** Malicious website could upload files to a victim's receive session if they can guess/obtain the token.

**Severity:** Medium (token is random UUID, but defense-in-depth missing)

**Fix:** Add CSRF token to session and validate on state-changing operations.

---

### 5. No Rate Limiting

**Location:** All HTTP handlers

**Issue:** No request throttling or rate limiting.

**Impact:**
- DoS via chunk upload spam
- Token brute-forcing possible (though impractical with UUIDs)
- Resource exhaustion

**Fix:** Add tower-governor or similar middleware:
```rust
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};

let governor_conf = GovernorConfigBuilder::default()
    .per_millisecond(100)
    .burst_size(10)
    .finish()
    .unwrap();

app.layer(GovernorLayer { config: Box::leak(Box::new(governor_conf)) })
```

**YAGNI Analysis:** You stated "DOS is not a concern" - this is reasonable for v1 but document this assumption clearly.

---

### 6. Nonce Reuse Risk (Theoretical)

**Location:** `src/crypto/decrypt.rs:11`

```rust
let mut full_nonce = [0u8; 12];
full_nonce[..7].copy_from_slice(nonce_base.as_bytes());
full_nonce[7..11].copy_from_slice(&counter.to_be_bytes());
```

**Issue:** Counter is u32, limiting to 2^32 chunks. For 256KB chunks, this limits files to ~1 PB. If counter wraps (unlikely but possible), nonce reuse would break AES-GCM security catastrophically.

**Impact:** Theoretical - would require >1PB file

**Fix:** Add assertion or use u64 counter:
```rust
const MAX_CHUNKS: u32 = u32::MAX - 1;
assert!(counter < MAX_CHUNKS, "Chunk counter overflow");
```

---

### 7. Missing Integrity Verification

**Location:** `src/transfer/receive.rs:156-171`

```rust
for i in 0..metadata.total_chunks {
    let chunk_path = format!("{}/{}.chunk", chunk_dir, i);
    let encrypted_chunk = tokio::fs::read(&chunk_path).await?;
    let decrypted = decrypt_chunk_at_position(...)?;
    output.write_all(&decrypted).await?;
}
```

**Issue:** No hash verification of final file. If chunks are corrupted or tampered with, decryption may succeed but produce garbage data.

**Impact:** Silent data corruption possible

**Fix:** Add file hash to manifest and verify after assembly:
```rust
let final_hash = sha256(&output_file).await?;
if final_hash != metadata.expected_hash {
    return Err("File integrity check failed");
}
```

**Note:** AES-GCM provides per-chunk authentication, but not cross-chunk integrity.

---

## ARCHITECTURE ISSUES

### 8. Session State Duplication

**Location:** `src/server/state.rs:18-31`

```rust
#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub session_key: String,  // ← Duplicate of session.session_key
    pub progress_sender: watch::Sender<f64>,
}
```

**Issue:** `session_key` is stored in both `Session` and `AppState`.

**Impact:** Maintenance burden, potential inconsistency

**SOLID Violation:** DRY principle - data duplicated without clear reason

**Fix:** Remove from AppState, access via `state.session.session_key()`

---

### 9. Tight Coupling: Session + State

**Location:** `src/server/state.rs`, `src/server/session.rs`

**Issue:** AppState directly embeds Session, making them inseparable. This violates Dependency Inversion Principle.

**Impact:**
- Hard to test session logic independently
- Hard to swap session storage (e.g., to Redis)

**Fix:** Use trait abstraction:
```rust
#[async_trait]
trait SessionStore {
    async fn validate(&self, token: &str) -> Result<bool>;
    async fn mark_used(&self, token: &str) -> Result<()>;
}

struct AppState<S: SessionStore> {
    session_store: Arc<S>,
    // ...
}
```

**YAGNI Analysis:** For single-session use case, current design is acceptable. But document the limitation.

---

### 10. Hardcoded Constants Without Configuration

**Locations:**
- `src/crypto/stream.rs:12` - `buffer: [u8; 65536]` (64KB send chunks)
- `templates/upload/upload.js` - `256 * 1024` (256KB receive chunks)
- `src/tunnel.rs:54` - `Duration::from_secs(30)` (tunnel timeout)

**Issue:** Magic numbers scattered throughout codebase.

**SOLID Violation:** Single Responsibility - configuration mixed with logic

**Fix:** Create config module:
```rust
pub mod config {
    pub const SEND_CHUNK_SIZE: usize = 64 * 1024;
    pub const RECEIVE_CHUNK_SIZE: usize = 256 * 1024;
    pub const TUNNEL_TIMEOUT_SECS: u64 = 30;
}
```

---

### 11. Inconsistent Chunk Sizes

**Send mode:** 64KB chunks (`src/crypto/stream.rs:12`)
**Receive mode:** 256KB chunks (`templates/upload/upload.js`)

**Issue:** No justification for different sizes. Asymmetry complicates reasoning about performance.

**Impact:** Suboptimal performance in one direction

**Recommendation:** Benchmark and standardize on one size, or make configurable with same default.

---

### 12. Error Handling: Generic anyhow::Error

**Location:** Throughout codebase

```rust
use anyhow::{Result, Context};

pub async fn finalize_upload(...) -> Result<axum::Json<Value>, AppError> {
    let json_string = tokio::fs::read_to_string(&metadata_path).await?;
    // ...
}
```

**Issue:** Using `anyhow::Error` loses type information. Cannot match on error variants for recovery.

**Impact:**
- All errors treated the same
- Impossible to implement retry logic
- Poor error messages to users

**SOLID Violation:** Interface Segregation - all errors lumped together

**Fix:** Define domain-specific error types:
```rust
#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("Invalid session token")]
    InvalidToken,

    #[error("Missing chunks: {missing}/{total}")]
    IncompleteUpload { missing: usize, total: usize },

    #[error("Path traversal attempt: {path}")]
    PathTraversal { path: String },

    #[error("Decryption failed")]
    DecryptionError(#[from] aes_gcm::Error),
}
```

**YAGNI Analysis:** For quick prototyping, anyhow is fine. But for production, typed errors are essential.

---

## PERFORMANCE ISSUES

### 13. Sequential Chunk Processing

**Location:** `src/transfer/receive.rs:156-171`

```rust
for i in 0..metadata.total_chunks {
    let encrypted_chunk = tokio::fs::read(&chunk_path).await?;
    let decrypted = decrypt_chunk_at_position(...)?;
    output.write_all(&decrypted).await?;
}
```

**Issue:** Chunks processed sequentially. Decryption is CPU-bound and could be parallelized.

**Impact:** Receive finalization slower than necessary for large files

**Optimization:**
```rust
use futures::stream::{self, StreamExt};

stream::iter(0..metadata.total_chunks)
    .map(|i| async move {
        let chunk = tokio::fs::read(format!("{}/{}.chunk", dir, i)).await?;
        decrypt_chunk_at_position(key, nonce, &chunk, i)
    })
    .buffer_unordered(4) // Parallel decryption
    .try_collect::<Vec<_>>()
    .await?
```

**YAGNI:** You said large files aren't a concern. Sequential is simpler. Document this tradeoff.

---

### 14. Unnecessary Disk I/O in Receive Mode

**Location:** `src/transfer/chunk.rs:92-100`

```rust
pub async fn save_encrypted_chunk(...) -> anyhow::Result<()> {
    let chunk_path = format!("/tmp/archdrop/{}/{}/{}.chunk", token, file_id, chunk_index);
    tokio::fs::write(&chunk_path, encrypted_data).await?;
    Ok(())
}
```

**Issue:** Encrypted chunks written to disk, then read back for decryption. For small transfers, this is pure overhead.

**Impact:**
- Unnecessary SSD wear
- Slower for small files
- Temp storage required

**Optimization:** For files under threshold (e.g., 10MB), keep chunks in memory:
```rust
enum ChunkStorage {
    Memory(HashMap<usize, Vec<u8>>),
    Disk { path: PathBuf },
}
```

**YAGNI Analysis:** Current approach is simpler and handles unbounded file sizes. Memory approach requires size limits. Document this choice.

---

### 15. Metadata Rewritten on Every Chunk

**Location:** `src/transfer/chunk.rs:102-115`

```rust
pub async fn update_chunk_metadata(...) -> anyhow::Result<()> {
    metadata.completed_chunks.insert(chunk_index);

    let metadata_path = format!("/tmp/archdrop/{}/{}/metadata.json", token, file_id);
    let json = serde_json::to_string_pretty(metadata)?;
    tokio::fs::write(&metadata_path, json).await?; // ← Rewrite entire file
    Ok(())
}
```

**Issue:** Metadata file rewritten for every chunk upload. For 1000-chunk file, this is 1000 writes.

**Impact:** Disk I/O bottleneck, SSD wear

**Optimization:**
1. Batch metadata updates
2. Use append-only log
3. Keep metadata in memory, flush on finalize

**YAGNI Analysis:** For your use case (quick transfers), this is acceptable. But document it.

---

### 16. Memory Allocation in Hot Path

**Location:** `src/crypto/stream.rs:56-59`

```rust
let len = encrypted.len() as u32;
let mut framed = len.to_be_bytes().to_vec(); // ← Allocation
framed.extend_from_slice(&encrypted);        // ← Reallocation
```

**Issue:** Creates Vec, then extends it. Could preallocate:

```rust
let mut framed = Vec::with_capacity(4 + encrypted.len());
framed.extend_from_slice(&len.to_be_bytes());
framed.extend_from_slice(&encrypted);
```

**Impact:** Minor - allocation is amortized, but adds GC pressure

---

## CODE QUALITY ISSUES

### 17. Commented-Out Code

**Location:** `templates/shared/crypto.js:52`

```javascript
// Clear url fragment immediatly after getti
// window.location.replace(window.location.href.split('#')[0])
```

**Issue:** Dead code left in production build. Typo in comment ("immediatly", "getti").

**DRY/SIMPLE Violation:** Clean up or enable it.

---

### 18. Inconsistent Error Messages

**Examples:**
- `src/main.rs:51` - "File not found: {}"
- `src/transfer/receive.rs:21` - "Invalid token"
- `src/transfer/receive.rs:52` - "Invalid or expired token"

**Issue:** Same condition (invalid token) has different messages.

**Impact:** Confusing logs, hard to grep

**Fix:** Use consistent error message constants or enums.

---

### 19. Missing Documentation

**Observation:** No doc comments on public APIs.

**Examples:**
```rust
pub struct Session { ... } // What is this?
pub async fn receive_handler(...) // What does this return?
```

**Impact:** Hard for new contributors to understand intent

**Fix:** Add doc comments:
```rust
/// Session represents a single file transfer session.
/// Sessions are single-use and tied to a unique token.
pub struct Session { ... }
```

---

### 20. Magic String Paths

**Location:** Multiple files

```rust
format!("/tmp/archdrop/{}/{}/metadata.json", token, file_id);
```

**Issue:** Path construction logic duplicated. If structure changes, must update multiple locations.

**DRY Violation:** Path logic should be centralized

**Fix:**
```rust
mod paths {
    pub fn chunk_dir(token: &str, file_id: &str) -> PathBuf {
        PathBuf::from("/tmp/archdrop").join(token).join(file_id)
    }

    pub fn metadata_path(token: &str, file_id: &str) -> PathBuf {
        chunk_dir(token, file_id).join("metadata.json")
    }
}
```

---

### 21. No Cleanup on Error

**Location:** `src/transfer/receive.rs:83-181`

**Issue:** If finalization fails midway, partial files and temp data remain.

```rust
pub async fn finalize_upload(...) -> Result<...> {
    // ... decryption loop ...
    output.write_all(&decrypted).await?; // ← If this fails, partial file left

    // Cleanup only on success
    tokio::fs::remove_dir_all(&chunk_dir).await.ok();
}
```

**Impact:** Disk space leaks, corrupt partial files

**Fix:** Use RAII cleanup guard:
```rust
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
```

---

### 22. Panic on Progress Send Failure

**Location:** `src/crypto/stream.rs:39,64`

```rust
let _ = self.progress_sender.send(100.0);
```

**Issue:** Ignored result. If progress receiver is dropped, this fails silently.

**Impact:** Progress may not update, but transfer continues

**Analysis:** This is acceptable - progress is best-effort. But add comment explaining why it's ignored.

---

### 23. Print Statement in Production Code

**Location:** `src/transfer/send.rs:49`

```rust
println!("Starting stream");
```

**Issue:** Debug print left in release code

**Fix:** Use proper logging:
```rust
tracing::info!("Starting stream for file {}", file_index);
```

Or remove if not needed (YAGNI).

---

### 24. Unused Variables

**Location:** `templates/shared/crypto.js:97`

```javascript
let remaining = buffer.slice(4 + length) // ← Not declared with 'let' in loop
yield { frame, remaining }
buffer = remaining
```

**Issue:** `remaining` not declared in this scope (works but sloppy)

**Fix:**
```javascript
const remaining = buffer.slice(4 + length)
```

---

## TESTING GAPS

### 25. Missing Test Coverage

**Current tests:**
- `decryption_tests.rs` - Basic crypto
- `hash_tests.rs` - Path hashing
- `integration_test.rs` - End-to-end

**Missing:**
- Session validation edge cases
- Path traversal attack scenarios
- Concurrent upload race conditions
- Error recovery flows
- Large file handling
- Network failure simulation

**Recommendation:** Add property-based tests for crypto, fuzzing for path validation.

---

## PRINCIPLE VIOLATIONS SUMMARY

### YAGNI (You Aren't Gonna Need It)

**Good:**
- No premature optimization (sequential processing is fine for your use case)
- No complex configuration system (not needed for single-user CLI)

**Bad:**
- Session abstraction could be simpler (just a HashMap would work)
- Separate `is_valid()` and `mark_used()` when you always call them together

---

### SOLID

**Single Responsibility:**
- ❌ `Session` handles validation AND state storage
- ❌ Temp path logic scattered across files
- ✅ Crypto module cleanly separated

**Open/Closed:**
- ❌ Hard to extend to multiple sessions without rewriting `AppState`
- ✅ Encryption abstraction allows swapping algorithms

**Liskov Substitution:**
- N/A (no inheritance)

**Interface Segregation:**
- ❌ `anyhow::Error` too broad - should have specific error types

**Dependency Inversion:**
- ❌ `AppState` directly depends on concrete `Session` struct
- Should depend on `SessionStore` trait

---

### SIMPLE

**Good:**
- Clear module structure
- Straightforward control flow
- Minimal dependencies

**Bad:**
- Nonce construction is subtle and error-prone
- Frame parsing logic is complex (could use a library)

---

### DRY (Don't Repeat Yourself)

**Violations:**
- `session_key` duplicated in `Session` and `AppState`
- Path construction duplicated across files
- Error messages inconsistent and duplicated

---

## POSITIVE OBSERVATIONS

1. **Good use of Rust's type system:** EncryptionKey and Nonce are newtype wrappers preventing misuse
2. **Proper async/await:** No blocking operations in async contexts
3. **Streaming approach:** Memory-efficient for large files
4. **Zero-knowledge architecture:** Clean separation of key management
5. **Path traversal protection:** Properly implemented with canonicalization
6. **Tests exist:** Many projects have zero tests

---

## RECOMMENDATIONS BY PRIORITY

### P0 (Security - Fix Before Production)
1. Fix session validation race condition
2. Enable URL fragment clearing
3. Fix temp directory permissions (mode 0o700)
4. Add file integrity verification

### P1 (Stability - Fix Soon)
1. Define typed errors (replace anyhow)
2. Add cleanup on error paths
3. Centralize temp path logic
4. Add rate limiting

### P2 (Quality - Fix Eventually)
1. Remove code duplication (session_key, paths)
2. Add proper logging (replace println!)
3. Standardize chunk sizes
4. Add doc comments

### P3 (Nice-to-Have)
1. Parallel chunk decryption
2. Memory-based chunk storage for small files
3. Batch metadata updates

---

## FINAL ASSESSMENT

This is a strong proof-of-concept that demonstrates good architectural thinking and solid Rust skills. The zero-knowledge design is well-executed, and the streaming approach is appropriate.

However, there are several security and reliability issues that prevent this from being production-ready. The main concerns are:

1. Race conditions in session management
2. Temp file security
3. Missing error recovery
4. No resource cleanup guarantees

For a personal tool or demo, this is excellent work. For a production service handling sensitive data, it needs hardening.

**Recommendation:** Fix P0 security issues immediately. Consider this a v0.9 that's 90% there.

The codebase shows you understand the fundamentals. Focus on defensive programming, error handling, and the boring stuff that makes software reliable.
