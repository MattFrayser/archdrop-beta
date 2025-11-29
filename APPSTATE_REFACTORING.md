# AppState Refactoring Analysis

## Current Implementation

### Current AppState Structure

```rust
#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub session_key: String,
    pub progress_sender: watch::Sender<f64>,
}
```

**Current Session:**
```rust
#[derive(Clone)]
pub struct Session {
    token: String,
    manifest: Option<Arc<Manifest>>,    // Send mode only
    destination: Option<PathBuf>,       // Receive mode only
    session_key: String,
    used: Arc<Mutex<bool>>,
}
```

**How it's used now:**
```rust
// In start_receive_server:
let (session, _token) = Session::new_receive(destination, session_key_b64.clone());

let state = AppState {
    session,
    session_key: session_key_b64.clone(),  // DUPLICATE!
    progress_sender,
};

let app = Router::new()
    .route("/receive/:token/chunk", post(receive::receive_handler))
    .with_state(state);
```

---

## Issues with Current Design

### 1. Data Duplication

```rust
pub struct AppState {
    pub session: Session,          // Has session_key inside
    pub session_key: String,       // DUPLICATE - same data
```

**Why is this redundant?**
- `session.session_key()` returns `&str`
- Could just use `state.session.session_key()`
- Violates DRY principle

**Historical reason:**
- Probably added for convenience (avoid calling method)
- Or because Session's session_key is private

### 2. No Per-File State Tracking

Current receive handler has nowhere to store:
- Per-file chunk storage (ChunkStorage enum)
- Which chunks have been received
- File metadata (size, total chunks, nonce)

**Current workaround:**
- Writes everything to disk (`/tmp/archdrop`)
- Uses filesystem as state storage
- Inefficient for small files

### 3. Single Global Progress Sender

```rust
pub progress_sender: watch::Sender<f64>,
```

**Problem:**
- Can only track one file's progress at a time
- If uploading multiple files, progress jumps around
- Not critical but confusing for user

---

## Proposed Refactoring

### New AppState Structure

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub progress_sender: watch::Sender<f64>,

    // NEW: Per-file upload tracking
    pub upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
}

pub struct UploadSession {
    storage: ChunkStorage,
    total_chunks: usize,
    nonce: String,
    relative_path: String,
    file_size: u64,
}
```

### Why This Design?

**Separation of Concerns:**
- `Session`: Handles token validation, used flag, manifest (send) or destination (receive)
- `UploadSession`: Handles per-file upload state
- `AppState`: Glues them together

**Why HashMap?**
- Key: `file_id` (hash of relative path)
- Value: `UploadSession` for that file
- Supports multiple concurrent file uploads

**Why Arc<RwLock<...>>?**
- `Arc`: Shared ownership (AppState is cloned for each request)
- `RwLock`: Multiple readers OR one writer
- Async-aware (doesn't block threads)

---

## Complete Refactored Code

### state.rs Changes

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::transfer::storage::ChunkStorage;

#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub progress_sender: watch::Sender<f64>,

    // Per-file upload state (only used in receive mode)
    pub upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
}

impl AppState {
    pub fn new_send(session: Session, progress_sender: watch::Sender<f64>) -> Self {
        Self {
            session,
            progress_sender,
            upload_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn new_receive(session: Session, progress_sender: watch::Sender<f64>) -> Self {
        Self {
            session,
            progress_sender,
            upload_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

pub struct UploadSession {
    pub storage: ChunkStorage,
    pub total_chunks: usize,
    pub nonce: String,
    pub relative_path: String,
    pub file_size: u64,
}
```

**Key changes:**
1. Removed `session_key` field (use `session.session_key()`)
2. Added `upload_sessions` HashMap
3. Added constructors (`new_send`, `new_receive`)
4. `UploadSession` owns the `ChunkStorage`

### server/mod.rs Changes

**Send mode (no change needed):**
```rust
pub async fn start_send_server(manifest: Manifest, mode: ServerMode) -> Result<u16> {
    let session_key = EncryptionKey::new();
    let session_key_b64 = session_key.to_base64();

    let (session, _token) = Session::new_send(manifest.clone(), session_key_b64.clone());
    let (progress_sender, _) = tokio::sync::watch::channel(0.0);

    let state = AppState::new_send(session, progress_sender.clone());

    let app = Router::new()
        .route("/send/:token/manifest", get(chunk::serve_manifest))
        .route("/send/:token/:file_index/chunk/:chunk_index", get(send::send_chunk_handler))  // NEW
        .with_state(state);

    // ... rest unchanged
}
```

**Receive mode:**
```rust
pub async fn start_receive_server(destination: PathBuf, mode: ServerMode) -> Result<u16> {
    let session_key = EncryptionKey::new();
    let session_key_b64 = session_key.to_base64();

    let (session, _token) = Session::new_receive(destination.clone(), session_key_b64.clone());
    let (progress_sender, _) = tokio::sync::watch::channel(0.0);

    let state = AppState::new_receive(session, progress_sender.clone());

    let app = Router::new()
        .route("/receive/:token/chunk", post(receive::receive_handler))
        .route("/receive/:token/finalize", post(receive::finalize_upload))
        // Remove /receive/:token/status - no longer needed
        .with_state(state);

    // ... rest unchanged
}
```

---

## How It's Used in Handlers

### Receive Handler

```rust
pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    // Validate token
    if !state.session.is_valid(&token).await {
        return Err(anyhow::anyhow!("Invalid token").into());
    }

    let chunk = parse_chunk_upload(multipart).await?;
    let file_id = hash_path(&chunk.relative_path);

    // Lock upload_sessions
    let mut sessions = state.upload_sessions.write().await;

    // Get or create session
    let session = sessions.entry(file_id.clone()).or_insert_with(|| {
        let destination = state.session.get_destination().unwrap().clone();
        let dest_path = destination.join(&chunk.relative_path);

        UploadSession {
            storage: ChunkStorage::new(chunk.file_size, dest_path).await.unwrap(),
            total_chunks: chunk.total_chunks,
            nonce: chunk.nonce.clone().unwrap_or_default(),
            relative_path: chunk.relative_path.clone(),
            file_size: chunk.file_size,
        }
    });

    // Check duplicate
    if session.storage.has_chunk(chunk.chunk_index) {
        return Ok(Json(json!({ "success": true, "duplicate": true })));
    }

    // Get session key from Session (not duplicated in AppState)
    let session_key = EncryptionKey::from_base64(state.session.session_key())?;
    let file_nonce = Nonce::from_base64(&session.nonce)?;

    // Store chunk
    session
        .storage
        .store_chunk(chunk.chunk_index, chunk.data, &session_key, &file_nonce)
        .await?;

    Ok(Json(json!({ "success": true })))
}
```

**How RwLock works:**
```rust
let mut sessions = state.upload_sessions.write().await;
//                                       ^^^^^
//                                  Acquires exclusive lock
//                                  (async, non-blocking)

// sessions is RwLockWriteGuard<HashMap<...>>
let session = sessions.entry(file_id).or_insert_with(...);

// ... use session ...

// End of scope: lock automatically released (Drop)
```

---

## Is This The Best Implementation?

### Alternative 1: Everything in Session

```rust
pub struct Session {
    token: String,
    manifest: Option<Arc<Manifest>>,
    destination: Option<PathBuf>,
    session_key: String,
    used: Arc<Mutex<bool>>,

    // Add this:
    upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
}
```

**Pros:**
- Everything in one place
- Fewer struct definitions

**Cons:**
- Session does too much (violates SRP)
- Send mode doesn't need upload_sessions (wasted field)
- Harder to test in isolation

**Verdict:** ❌ Not ideal

---

### Alternative 2: Separate UploadManager

```rust
pub struct UploadManager {
    sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
}

pub struct AppState {
    pub session: Session,
    pub upload_manager: UploadManager,
    pub progress_sender: watch::Sender<f64>,
}
```

**Pros:**
- Clean separation of concerns
- Can test UploadManager independently
- Could add methods to UploadManager (e.g., cleanup expired sessions)

**Cons:**
- Extra indirection: `state.upload_manager.sessions.write().await`
- More boilerplate
- YAGNI? (Do we need a whole struct for one HashMap?)

**Verdict:** ✅ Good for larger projects, overkill for this

---

### Alternative 3: Direct in AppState (Proposed)

```rust
pub struct AppState {
    pub session: Session,
    pub upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
    pub progress_sender: watch::Sender<f64>,
}
```

**Pros:**
- Simple and direct
- Easy to access: `state.upload_sessions.write().await`
- Clear separation: Session = validation, upload_sessions = file state
- Minimal boilerplate

**Cons:**
- AppState has multiple responsibilities (Session + upload tracking)
- Could argue it violates SRP

**Verdict:** ✅ **Best for this project**

**Why?**
- Your use case is simple: single transfer at a time
- No complex lifecycle management needed
- Prioritizes simplicity over abstract purity
- Easy to refactor later if needed

---

### Alternative 4: No Global State (Pass Around)

```rust
pub async fn receive_handler(
    Path(token): Path<String>,
    session: Session,
    upload_sessions: Arc<RwLock<HashMap<...>>>,
    multipart: Multipart,
) -> Result<...> {
```

**Pros:**
- Explicit dependencies
- No global state

**Cons:**
- Axum doesn't work this way (State must be Clone)
- Would need custom extractors
- More complex

**Verdict:** ❌ Doesn't fit Axum's model

---

## Potential Improvements

### 1. Remove session_key Duplication

**Current (bad):**
```rust
pub struct AppState {
    pub session: Session,
    pub session_key: String,  // Duplicate!
```

**Fixed:**
```rust
pub struct AppState {
    pub session: Session,
    // No session_key field
```

**Usage:**
```rust
// Old:
let key = EncryptionKey::from_base64(&state.session_key)?;

// New:
let key = EncryptionKey::from_base64(state.session.session_key())?;
```

**Benefit:** DRY, single source of truth

---

### 2. Make Session Immutable After Creation

**Current:**
```rust
pub async fn mark_used(&self) {
    *self.used.lock().await = true;
}
```

**Problem:** Mutable state via interior mutability (Mutex)

**Alternative (more functional):**
```rust
pub struct Session {
    token: String,
    // Remove: used: Arc<Mutex<bool>>,
}

// Track used externally
pub struct AppState {
    pub session: Session,
    pub session_used: Arc<Mutex<bool>>,  // Or AtomicBool
```

**Benefit:** Session is immutable, easier to reason about

**Trade-off:** More state in AppState

**Verdict:** Current design is fine (interior mutability is common in Rust)

---

### 3. Cleanup Old Upload Sessions

**Problem:** HashMap grows indefinitely

**Solution:** Add cleanup on finalize

```rust
pub async fn finalize_upload(...) -> Result<...> {
    let file_id = hash_path(&relative_path);
    let mut sessions = state.upload_sessions.write().await;

    let session = sessions.remove(&file_id)  // Remove from map
        .ok_or_else(|| anyhow::anyhow!("No session"))?;

    // ... process ...
}
```

**Already handled in implementation guide!** ✅

---

### 4. Type-Safe File IDs

**Current:**
```rust
let file_id = hash_path(&chunk.relative_path);  // Returns String
let session = sessions.get(&file_id);
```

**Could use newtype:**
```rust
pub struct FileId(String);

impl FileId {
    pub fn from_path(path: &str) -> Self {
        FileId(hash_path(path))
    }
}
```

**Benefit:** Can't accidentally use wrong string as file_id

**Trade-off:** More boilerplate

**Verdict:** YAGNI for this project

---

## Performance Considerations

### RwLock vs Mutex

**RwLock:**
```rust
Arc<RwLock<HashMap<...>>>
```

**Characteristics:**
- Multiple readers OR one writer
- Good when reads >> writes
- Slightly more overhead than Mutex

**Our case:**
- Each chunk upload = write operation
- No read-only operations on HashMap
- RwLock might be overkill

**Alternative: Mutex**
```rust
Arc<Mutex<HashMap<...>>>
```

**Simpler, but:**
- Only one thread at a time (read or write)
- For your use case (single transfer), no difference

**Verdict:** Either is fine, RwLock is future-proof

---

### DashMap (Lock-Free HashMap)

**Alternative:**
```rust
use dashmap::DashMap;

pub upload_sessions: Arc<DashMap<String, UploadSession>>,
```

**Pros:**
- Lock-free, faster for concurrent access
- No need for .write().await
- Just: `sessions.insert(key, value)`

**Cons:**
- External dependency
- Overkill for single-user CLI

**Verdict:** YAGNI

---

## Recommended Implementation

### Final AppState

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub progress_sender: watch::Sender<f64>,
    pub upload_sessions: Arc<RwLock<HashMap<String, UploadSession>>>,
}

impl AppState {
    pub fn new(session: Session, progress_sender: watch::Sender<f64>) -> Self {
        Self {
            session,
            progress_sender,
            upload_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

pub struct UploadSession {
    pub storage: ChunkStorage,
    pub total_chunks: usize,
    pub nonce: String,
    pub relative_path: String,
    pub file_size: u64,
}
```

**Changes from current:**
1. ✅ Remove `session_key` field (use `session.session_key()`)
2. ✅ Add `upload_sessions` HashMap
3. ✅ Single constructor (works for both send/receive)

**Why this is best:**
- **Simple:** Direct access, minimal indirection
- **Clear:** Session = auth, upload_sessions = file state
- **Efficient:** Arc for sharing, RwLock for concurrency
- **YAGNI:** No over-engineering

---

## Migration Steps

1. **Update state.rs:**
   - Add `upload_sessions` field
   - Remove `session_key` field
   - Add `UploadSession` struct

2. **Update server/mod.rs:**
   - Use `AppState::new()` instead of struct literal
   - Add chunk endpoint route

3. **Update handlers:**
   - Replace `state.session_key` with `state.session.session_key()`
   - Use `state.upload_sessions` for per-file tracking

4. **Test:**
   - Upload small file (< 100MB)
   - Upload large file (> 100MB)
   - Upload multiple files simultaneously
   - Verify no session_key errors

---

## Summary

**Current issues:**
- Duplicated session_key
- No per-file state tracking
- Inefficient (everything to disk)

**Proposed solution:**
- Remove session_key from AppState
- Add upload_sessions HashMap
- Each file gets own UploadSession with ChunkStorage

**Is it the best?**
- ✅ Yes for this project size
- Alternative 2 (UploadManager) is more "correct" but overkill
- Balances simplicity with functionality
- Easy to refactor later if needed

**Key insight:**
Your app handles ONE transfer at a time, but that transfer might include multiple files. The HashMap handles per-file state within that single transfer.
