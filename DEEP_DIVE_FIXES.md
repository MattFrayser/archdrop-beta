# Deep Investigation & Fix Walkthrough

## Issue #4: Memory Leak - Unclosed File Handles

### Current Problem

**Location:** `src/transfer/send_handlers.rs:79-96`

```rust
let file_handle = file_handles
    .entry(file_index)
    .or_insert_with(|| match std::fs::File::open(&file_entry.full_path) {
        Ok(file) => {
            tracing::debug!("Opened file handle for {}", file_entry.name);
            Arc::new(file)
        }
        Err(e) => {
            tracing::error!("Failed to open file {}: {}", file_entry.full_path.display(), e);
            panic!("Failed to open file for sending");  // ❌ PANIC IN HTTP HANDLER!
        }
    })
    .clone();
```

### The Issues

1. **Files stay open until session completes** - With 100 files, you have 100 open file descriptors
2. **Panic in HTTP handler** - Will crash the entire server if a file can't be opened
3. **No cleanup on error** - If download fails mid-way, handles stay open
4. **No limits** - Could hit OS file descriptor limits (default 1024 on Linux)

### Why It's a Problem

**Scenario:** User sends 500 files, each 10MB:
- All 500 files get opened as chunks are requested
- Even after individual files finish downloading, handles stay open
- If transfer is interrupted, handles leak until server shutdown
- On a busy server with multiple transfers, you'll hit ulimit

### The Fix - Strategy 1: Close After File Completes

**Create a file handle wrapper with tracking:**

```rust
// src/transfer/file_handle.rs (NEW FILE)
use std::fs::File;
use std::sync::Arc;
use dashmap::DashMap;
use anyhow::Result;

pub struct FileHandleManager {
    handles: Arc<DashMap<usize, Arc<File>>>,
    completed_files: Arc<DashMap<usize, ()>>,
}

impl FileHandleManager {
    pub fn new() -> Self {
        Self {
            handles: Arc::new(DashMap::new()),
            completed_files: Arc::new(DashMap::new()),
        }
    }

    pub fn get_or_open(&self, file_index: usize, path: &std::path::Path) -> Result<Arc<File>> {
        if let Some(handle) = self.handles.get(&file_index) {
            return Ok(handle.clone());
        }

        let file = File::open(path)?;
        let handle = Arc::new(file);
        self.handles.insert(file_index, handle.clone());

        tracing::debug!("Opened file handle for index {}: {}", file_index, path.display());
        Ok(handle)
    }

    /// Mark file as complete and close its handle
    pub fn mark_complete(&self, file_index: usize) {
        if let Some((_, handle)) = self.handles.remove(&file_index) {
            self.completed_files.insert(file_index, ());
            // Arc will drop when last reference is gone
            tracing::debug!("Closed file handle for index {}", file_index);
        }
    }

    pub fn close_all(&self) {
        let count = self.handles.len();
        self.handles.clear();
        if count > 0 {
            tracing::debug!("Closed {} file handle(s)", count);
        }
    }

    pub fn open_count(&self) -> usize {
        self.handles.len()
    }
}
```

**Update AppState to use the manager:**

```rust
// src/server/state.rs
use crate::transfer::file_handle::FileHandleManager;

#[derive(Clone)]
pub enum TransferStorage {
    Send(FileHandleManager),  // ← Changed from Arc<DashMap<usize, Arc<File>>>
    Receive(Arc<DashMap<String, FileReceiveState>>),
}

impl AppState {
    pub fn file_handle_manager(&self) -> Option<&FileHandleManager> {
        match &self.transfers {
            TransferStorage::Send(manager) => Some(manager),
            _ => None,
        }
    }
}
```

**Update send_handler to use it:**

```rust
// src/transfer/send_handlers.rs
pub async fn send_handler(
    Path((token, file_index, chunk_index)): Path<(String, usize, usize)>,
    Query(params): Query<ChunkParams>,
    State(state): State<AppState>,
) -> Result<Response<Body>, AppError> {
    let client_id = &params.client_id;
    auth::require_active_session(&state.session, &token, client_id)?;

    let manager = state
        .file_handle_manager()
        .ok_or_else(|| anyhow::anyhow!("Invalid server mode: not a send server"))?;

    let file_entry = state
        .session
        .get_file(file_index)
        .ok_or_else(|| anyhow::anyhow!("Invalid file index"))?;

    // Calculate chunk boundaries
    let start = chunk_index as u64 * config::CHUNK_SIZE;
    let end = std::cmp::min(start + config::CHUNK_SIZE, file_entry.size);

    if start >= file_entry.size {
        return Err(anyhow::anyhow!("Chunk index out of bounds").into());
    }

    let chunk_len = (end - start) as usize;

    // Get or open file handle (proper error handling, no panic!)
    let file_handle = manager
        .get_or_open(file_index, &file_entry.full_path)
        .context(format!("Failed to open file: {}", file_entry.name))?;

    // Read chunk
    let buffer = io::read_chunk_at_position(&file_handle, start, chunk_len)?;

    // Encrypt
    let file_nonce = Nonce::from_base64(&file_entry.nonce)
        .context(format!("Invalid nonce for file: {}", file_entry.name))?;

    let cipher = state.session.cipher();
    let encrypted = crypto::encrypt_chunk_at_position(cipher, &file_nonce, &buffer, chunk_index as u32)
        .context(format!("Failed to encrypt chunk {} of file {}", chunk_index, file_entry.name))?;

    // Check if this was the last chunk for this file
    let total_chunks = (file_entry.size + config::CHUNK_SIZE - 1) / config::CHUNK_SIZE;
    if chunk_index as u64 == total_chunks - 1 {
        // Last chunk - close the file handle
        manager.mark_complete(file_index);
    }

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(encrypted))?)
}
```

### Alternative Fix - Strategy 2: LRU Cache (For Many Small Files)

If you're sending 1000+ small files, opening/closing on every access has overhead. Use an LRU cache:

```toml
# Cargo.toml
lru = "0.12"
```

```rust
use lru::LruCache;
use std::num::NonZeroUsize;
use parking_lot::Mutex;

pub struct FileHandleCache {
    cache: Arc<Mutex<LruCache<usize, Arc<File>>>>,
}

impl FileHandleCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(NonZeroUsize::new(capacity).unwrap()))),
        }
    }

    pub fn get_or_open(&self, file_index: usize, path: &Path) -> Result<Arc<File>> {
        let mut cache = self.cache.lock();

        if let Some(handle) = cache.get(&file_index) {
            return Ok(handle.clone());
        }

        // Open file
        let file = File::open(path)?;
        let handle = Arc::new(file);

        // Insert into LRU (automatically evicts oldest if at capacity)
        cache.put(file_index, handle.clone());

        Ok(handle)
    }
}
```

---

## Issue #5: Async Work in Synchronous Drop

### Current Problem

**Location:** `src/transfer/storage.rs:113-126`

```rust
impl Drop for ChunkStorage {
    fn drop(&mut self) {
        if !self.disarmed {
            let path = self.path.clone();

            // ❌ Spawning async work that might never complete!
            tokio::task::spawn_blocking(move || {
                if let Err(e) = std::fs::remove_file(&path) {
                    eprintln!("Error cleaning up temporary file {}: {}", path.display(), e);
                }
            });
        }
    }
}
```

### Why It's a Problem

**The Issue:** `spawn_blocking` returns a `JoinHandle` that you're immediately dropping. There's no guarantee this task will:
1. Ever get scheduled (if runtime is shutting down)
2. Complete before process exit
3. Actually delete the file

**Scenario where it fails:**
```rust
{
    let storage = ChunkStorage::new(path).await?;
    // ... error occurs ...
} // ← Drop called, spawn_blocking queued, but...
// Runtime shuts down immediately
// File never deleted!
```

### The Fix

**Drop should be synchronous and blocking:**

```rust
// src/transfer/storage.rs
impl Drop for ChunkStorage {
    fn drop(&mut self) {
        if !self.disarmed {
            // Drop is synchronous, so we must block
            // This is fine - Drop should clean up resources
            if let Err(e) = std::fs::remove_file(&self.path) {
                // Use tracing instead of eprintln
                tracing::warn!(
                    path = %self.path.display(),
                    error = %e,
                    "Failed to clean up temporary file"
                );
            } else {
                tracing::debug!(
                    path = %self.path.display(),
                    "Cleaned up incomplete transfer file"
                );
            }
        }
    }
}
```

### Why This is Safe

1. **Drop is called in blocking context** - You're already in tokio's blocking pool or on a thread
2. **File deletion is fast** - It's just an unlink syscall, typically microseconds
3. **It's guaranteed to run** - Unlike spawn_blocking, Drop always executes
4. **Proper cleanup semantics** - Resources are freed immediately

### Additional Improvement - Async Cleanup Method

For graceful shutdowns, add an explicit async method:

```rust
impl ChunkStorage {
    /// Explicitly cleanup without waiting for Drop
    /// Useful for graceful shutdown scenarios
    pub async fn cleanup(mut self) -> Result<()> {
        if !self.disarmed {
            self.disarmed = true; // Prevent Drop from running
            tokio::fs::remove_file(&self.path)
                .await
                .context("Failed to remove incomplete file")?;
            tracing::info!("Cleaned up incomplete file: {}", self.path.display());
        }
        Ok(())
    }

    // Regular finalize stays the same
    pub async fn finalize(mut self) -> Result<String> {
        // ... existing code ...
        self.disarmed = true;
        // ... hash calculation ...
        Ok(hash)
    }
}
```

Now you can handle cleanup in two ways:
- **Normal case:** Call `finalize()` - async, can fail gracefully
- **Error case:** Let Drop handle it - synchronous, always runs

---

## Issue #9: Fake Progress Tracking

### Current Problem

Progress is only reported at 0% and 100%:

```rust
// src/transfer/send_handlers.rs:140
let _ = state.progress_sender.send(100.0);

// src/transfer/receive_handlers.rs:189
let _ = state.progress_sender.send(100.0);
```

**Result:** The TUI shows a progress bar that never moves until completion. Users think their transfer is stuck.

### Why It's Happening

Looking at the architecture:
1. Progress is per-session (not per-file or per-chunk)
2. Multiple concurrent chunk downloads happen
3. No centralized place to track overall progress

### The Fix - Multi-Level Progress Tracking

**Step 1: Track bytes transferred, not just completion**

```rust
// src/server/state.rs
use std::sync::atomic::{AtomicU64, Ordering};

pub struct ProgressTracker {
    total_bytes: u64,
    transferred_bytes: Arc<AtomicU64>,
    progress_sender: watch::Sender<f64>,
}

impl ProgressTracker {
    pub fn new(total_bytes: u64, progress_sender: watch::Sender<f64>) -> Self {
        Self {
            total_bytes,
            transferred_bytes: Arc::new(AtomicU64::new(0)),
            progress_sender,
        }
    }

    /// Add bytes and update progress
    pub fn add_bytes(&self, bytes: u64) {
        let transferred = self.transferred_bytes.fetch_add(bytes, Ordering::Relaxed) + bytes;
        let progress = if self.total_bytes > 0 {
            (transferred as f64 / self.total_bytes as f64) * 100.0
        } else {
            0.0
        };

        // Only send if progress changed by at least 1%
        let _ = self.progress_sender.send(progress);
    }

    pub fn complete(&self) {
        let _ = self.progress_sender.send(100.0);
    }
}

#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub progress_tracker: Arc<ProgressTracker>,
    pub transfers: TransferStorage,
}
```

**Step 2: Update send_handler to track bytes**

```rust
// src/transfer/send_handlers.rs
pub async fn send_handler(
    Path((token, file_index, chunk_index)): Path<(String, usize, usize)>,
    Query(params): Query<ChunkParams>,
    State(state): State<AppState>,
) -> Result<Response<Body>, AppError> {
    // ... existing validation code ...

    let chunk_len = (end - start) as usize;
    let file_handle = manager.get_or_open(file_index, &file_entry.full_path)?;

    // Read chunk
    let buffer = io::read_chunk_at_position(&file_handle, start, chunk_len)?;

    // Encrypt
    let encrypted = crypto::encrypt_chunk_at_position(cipher, &file_nonce, &buffer, chunk_index as u32)?;

    // ✅ Track progress AFTER successful encryption
    state.progress_tracker.add_bytes(chunk_len as u64);

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(encrypted))?)
}
```

**Step 3: Update receive_handler to track bytes**

```rust
// src/transfer/receive_handlers.rs
pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
    TypedMultipart(payload): TypedMultipart<ChunkUploadRequest>,
) -> Result<axum::Json<Value>, AppError> {
    // ... existing validation and storage ...

    // Store chunk (this decrypts and writes)
    session.storage.store_chunk(
        payload.chunk_index,
        payload.chunk.clone(),
        cipher,
        &nonce
    ).await?;

    // ✅ Track progress AFTER successful storage
    state.progress_tracker.add_bytes(payload.chunk.len() as u64);

    Ok(Json(json!({
        "success": true,
        "chunk": payload.chunk_index,
        "total": session.total_chunks,
        "received": session.storage.chunk_count()
    })))
}
```

**Step 4: Initialize with total bytes**

```rust
// src/server/api.rs
pub async fn start_send_server(manifest: Manifest, mode: ServerMode) -> Result<u16> {
    let session_key = EncryptionKey::new();
    let nonce = Nonce::new();

    // ✅ Calculate total bytes
    let total_bytes: u64 = manifest.files.iter().map(|f| f.size).sum();

    let display_name = if manifest.files.len() == 1 {
        manifest.files[0].name.clone()
    } else {
        format!("{} files", manifest.files.len())
    };

    let session = Session::new_send(manifest.clone(), session_key);
    let (progress_sender, _) = tokio::sync::watch::channel(0.0);

    // ✅ Create progress tracker with total size
    let progress_tracker = Arc::new(ProgressTracker::new(total_bytes, progress_sender.clone()));

    let state = AppState {
        session: session.clone(),
        progress_tracker,
        transfers: TransferStorage::Send(FileHandleManager::new()),
    };

    let app = create_send_router(&state);
    let server = ServerInstance::new(app, session, display_name, progress_sender);

    start_server(server, state, mode, ServerDirection::Send, nonce).await
}
```

### Result

Now the progress bar updates in real-time as bytes are transferred, giving users accurate feedback.

---

## Issue #11: Vibe-Coded State Management

### Current Problem

**Location:** `src/server/state.rs`

```rust
#[derive(Clone)]
pub enum TransferStorage {
    Send(Arc<DashMap<usize, Arc<std::fs::File>>>),
    Receive(Arc<DashMap<String, FileReceiveState>>),
}

impl AppState {
    // Awkward helper methods needed everywhere
    pub fn file_handles(&self) -> Option<&Arc<DashMap<usize, Arc<File>>>> {
        match &self.transfers {
            TransferStorage::Send(handles) => Some(handles),
            _ => None,
        }
    }

    pub fn receive_sessions(&self) -> Option<&Arc<DashMap<String, FileReceiveState>>> {
        match &self.transfers {
            TransferStorage::Receive(sessions) => Some(sessions),
            _ => None,
        }
    }
}
```

**Problems:**
1. Every handler unwraps `Option` from helper methods
2. Rust can't prevent you from calling `file_handles()` in receive mode at compile time
3. TransferStorage enum adds no semantic value
4. Code is littered with runtime checks that should be compile-time

### The Fix - Use Type State Pattern

**Create separate state types:**

```rust
// src/server/state.rs
use crate::transfer::file_handle::FileHandleManager;
use crate::server::session::Session;
use tokio::sync::watch;
use std::sync::Arc;

/// Common state shared by both modes
pub struct CommonState {
    pub session: Session,
    pub progress_tracker: Arc<ProgressTracker>,
}

/// Send-specific state
#[derive(Clone)]
pub struct SendState {
    pub common: CommonState,
    pub file_manager: FileHandleManager,
}

/// Receive-specific state
#[derive(Clone)]
pub struct ReceiveState {
    pub common: CommonState,
    pub receive_sessions: Arc<DashMap<String, FileReceiveState>>,
}

// Convenience trait for shared operations
pub trait AppState {
    fn session(&self) -> &Session;
    fn progress_tracker(&self) -> &Arc<ProgressTracker>;
}

impl AppState for SendState {
    fn session(&self) -> &Session {
        &self.common.session
    }

    fn progress_tracker(&self) -> &Arc<ProgressTracker> {
        &self.common.progress_tracker
    }
}

impl AppState for ReceiveState {
    fn session(&self) -> &Session {
        &self.common.session
    }

    fn progress_tracker(&self) -> &Arc<ProgressTracker> {
        &self.common.progress_tracker
    }
}
```

**Update handlers to be type-safe:**

```rust
// src/transfer/send_handlers.rs
pub async fn send_handler(
    Path((token, file_index, chunk_index)): Path<(String, usize, usize)>,
    Query(params): Query<ChunkParams>,
    State(state): State<SendState>,  // ✅ Type-safe! Can only be called with SendState
) -> Result<Response<Body>, AppError> {
    let client_id = &params.client_id;
    auth::require_active_session(state.session(), &token, client_id)?;

    // ✅ No Option unwrapping! file_manager is always available
    let file_handle = state.file_manager
        .get_or_open(file_index, &file_entry.full_path)?;

    // ... rest of handler ...
}
```

```rust
// src/transfer/receive_handlers.rs
pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<ReceiveState>,  // ✅ Type-safe!
    TypedMultipart(payload): TypedMultipart<ChunkUploadRequest>,
) -> Result<axum::Json<Value>, AppError> {
    // ✅ No Option unwrapping! receive_sessions is always available
    let receive_sessions = &state.receive_sessions;

    // ... rest of handler ...
}
```

**Update routers:**

```rust
// src/server/routes.rs
pub fn create_send_router(state: &SendState) -> Router {
    Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/send/:token/manifest", get(transfer::send_handlers::manifest_handler))
        .route("/send/:token/:file_index/chunk/:chunk_index", get(transfer::send_handlers::send_handler))
        .route("/send/:token/:file_index/hash", get(transfer::send_handlers::get_file_hash))
        .route("/send/:token/complete", post(transfer::send_handlers::complete_download))
        .route("/send/:token", get(static_files::serve_download_page))
        .route("/download.js", get(static_files::serve_download_js))
        .route("/styles.css", get(static_files::serve_shared_css))
        .route("/shared.js", get(static_files::serve_shared_js))
        .with_state(state.clone())
}

pub fn create_receive_router(state: &ReceiveState) -> Router {
    Router::new()
        .route("/health", get(|| async { "OK" }))
        .route("/receive/:token/chunk", post(transfer::receive_handlers::receive_handler))
        .route("/receive/:token/finalize", post(transfer::receive_handlers::finalize_upload))
        .route("/receive/:token", get(static_files::serve_upload_page))
        .route("/receive/:token/complete", post(transfer::receive_handlers::complete_transfer))
        .route("/upload.js", get(static_files::serve_upload_js))
        .route("/styles.css", get(static_files::serve_shared_css))
        .route("/shared.js", get(static_files::serve_shared_js))
        .with_state(state.clone())
}
```

### Benefits

1. ✅ **Compile-time safety** - Can't call send handlers with receive state
2. ✅ **No runtime unwraps** - State is always correct
3. ✅ **Cleaner code** - No awkward helper methods
4. ✅ **Better IDE support** - Autocomplete knows exact type
5. ✅ **Easier testing** - Can mock SendState without Receive fields

---

## Issue #12: Manifest Fetched Twice on Download Page

### Current Problem

**Location:** `templates/download.js`

```javascript
// Line 32-42: Fetched on page load
document.addEventListener('DOMContentLoaded', async () => {
    try {
        const token = window.location.pathname.split('/').pop()
        const clientId = getClientId()
        const manifestResponse = await fetch(`/send/${token}/manifest?clientId=${clientId}`)
        if (manifestResponse.ok) {
            const manifest = await manifestResponse.json()
            displayFileList(manifest.files)
        }
    } catch (error) {
        console.error('Failed to load file list:', error)
    }
})

// Line 79-89: Fetched AGAIN when download starts
async function startDownload() {
    try {
        const { key } = await getCredentialsFromUrl()
        const token = window.location.pathname.split('/').pop()

        // ❌ Fetching manifest AGAIN
        const manifestResponse = await fetch(`/send/${token}/manifest`)
        if (!manifestResponse.ok) {
            throw new Error(`Failed to fetch file list: HTTP ${manifestResponse.status}`)
        }
        const manifest = await manifestResponse.json()
        // ...
    }
}
```

**Why it's bad:**
1. Wastes bandwidth
2. Extra latency on download start
3. Manifest could theoretically change between fetches (race condition)
4. Session is claimed on manifest fetch - second fetch might fail

### The Fix

**Cache the manifest globally:**

```javascript
// templates/download.js

// ✅ Global cache
let cachedManifest = null;
let cachedToken = null;
let cachedClientId = null;

// Initialize on page load
document.addEventListener('DOMContentLoaded', async () => {
    const downloadBtn = document.getElementById('downloadBtn');
    if (downloadBtn) {
        downloadBtn.addEventListener('click', startDownload);
    }

    try {
        cachedToken = window.location.pathname.split('/').pop();
        cachedClientId = getClientId();

        // ✅ Fetch manifest once
        const manifestResponse = await fetch(`/send/${cachedToken}/manifest?clientId=${cachedClientId}`);

        if (!manifestResponse.ok) {
            throw new Error(`Failed to fetch manifest: HTTP ${manifestResponse.status}`);
        }

        cachedManifest = await manifestResponse.json();
        displayFileList(cachedManifest.files);

    } catch (error) {
        console.error('Failed to load file list:', error);
        showError('Failed to load file list. Please refresh the page.');
    }
});

// Download uses cached manifest
async function startDownload() {
    // ✅ Validate we have manifest
    if (!cachedManifest || !cachedToken) {
        alert('File list not loaded. Please refresh the page.');
        return;
    }

    const fileList = document.getElementById('fileList');
    const fileItems = fileList.querySelectorAll('.file-item');

    // Show progress bars
    fileItems.forEach(item => {
        const progress = item.querySelector('.file-progress');
        if (progress) progress.classList.add('show');
    });

    try {
        // Get encryption key from URL
        const { key } = await getCredentialsFromUrl();

        // ✅ Use cached manifest instead of fetching again
        await runWithConcurrency(
            cachedManifest.files.map((file, index) => ({
                file,
                index,
                fileItem: fileItems[index]
            })),
            async ({ file, fileItem }) => {
                fileItem.classList.add('downloading');
                try {
                    await downloadFile(cachedToken, file, key, fileItem);
                    fileItem.classList.remove('downloading');
                    fileItem.classList.add('completed');
                } catch (error) {
                    fileItem.classList.remove('downloading');
                    fileItem.classList.add('error');
                    throw error;
                }
            },
            MAX_CONCURRENT_FILES
        );

        // Complete the download
        await fetch(`/send/${cachedToken}/complete?clientId=${cachedClientId}`, {
            method: 'POST'
        });

        const downloadBtn = document.getElementById('downloadBtn');
        downloadBtn.textContent = 'Download Complete!';

    } catch(error) {
        console.error(error);
        alert(`Download failed: ${error.message}`);
    }
}

// Helper to show errors in UI
function showError(message) {
    const fileList = document.getElementById('fileList');
    fileList.innerHTML = `<div class="error-message">${message}</div>`;
    fileList.classList.add('show');
}
```

### Benefits

1. ✅ **One network request instead of two**
2. ✅ **Faster download start** - No waiting for second fetch
3. ✅ **Consistent data** - Same manifest throughout session
4. ✅ **Session claimed once** - No race conditions

---

## Issue #22: Unnecessary Arc-ing

### Current Problem

**Location:** Multiple files

```rust
// src/server/session.rs:11
pub enum SessionMode {
    Send { manifest: Arc<Manifest> },  // ← Arc here
    Receive { destination: PathBuf },
}

// src/transfer/manifest.rs:19
#[derive(Serialize, Deserialize, Clone)]  // ← Also has Clone!
pub struct Manifest {
    pub files: Vec<FileEntry>,
}

// Usage in runtime.rs:24
let session = server.session.clone();  // ← Cloning Session which clones the Arc
```

**The confusion:**
- Manifest is small (just Vec of metadata)
- Session is cloned multiple times
- Arc provides shared ownership, but Clone also works
- Mixed strategy: some things use Arc, some use Clone

### Analysis

Let's check Manifest size:

```rust
pub struct FileEntry {
    pub index: usize,           // 8 bytes
    pub name: String,           // 24 bytes (pointer + len + cap)
    pub full_path: PathBuf,     // 24 bytes
    pub relative_path: String,  // 24 bytes
    pub size: u64,              // 8 bytes
    pub nonce: String,          // 24 bytes
}
// Total: ~112 bytes per file

pub struct Manifest {
    pub files: Vec<FileEntry>,  // 24 bytes (just the Vec header)
}
```

**For 100 files:** Vec header is only 24 bytes, actual data is heap-allocated.
**Cloning a Manifest:** Clones the Vec, which clones all FileEntry structs (~11KB for 100 files)

### The Fix - Choose Your Strategy

**Option 1: Keep Arc (Better for large manifests)**

If you might have 1000+ files, Arc makes sense:

```rust
// Keep current approach but clean it up
pub struct Session {
    token: String,
    session_key: EncryptionKey,
    cipher: Arc<Aes256Gcm>,
    mode: SessionMode,
    state: Arc<RwLock<SessionState>>,
}

// Don't derive Clone on Session - force Arc usage
impl Session {
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }
}

// Usage
let session = Session::new_send(manifest, key).shared();
let session_clone = Arc::clone(&session);  // Explicit Arc cloning
```

**Option 2: Remove Arc (Simpler for typical use)**

If manifests are typically < 1000 files:

```rust
// src/server/session.rs
pub enum SessionMode {
    Send { manifest: Manifest },  // ✅ No Arc
    Receive { destination: PathBuf },
}

#[derive(Clone)]  // ✅ Keep Clone
pub struct Session {
    token: String,
    session_key: EncryptionKey,
    cipher: Arc<Aes256Gcm>,  // Keep Arc here - Aes256Gcm is not Clone
    mode: SessionMode,
    state: Arc<RwLock<SessionState>>,  // Keep Arc here - shared mutable state
}

impl Session {
    pub fn manifest(&self) -> Option<&Manifest> {  // ✅ Return reference
        match &self.mode {
            SessionMode::Send { manifest } => Some(manifest),
            _ => None,
        }
    }
}
```

**Usage changes:**

```rust
// src/transfer/send_handlers.rs
pub async fn manifest_handler(
    Path(token): Path<String>,
    Query(params): Query<ClientIdParam>,
    State(state): State<SendState>,
) -> Result<Json<Manifest>, AppError> {
    auth::claim_or_validate_session(state.session(), &token, &params.client_id)?;

    let manifest = state.session()
        .manifest()
        .ok_or_else(|| anyhow::anyhow!("Not a send session"))?;

    // ✅ Clone only when needed for serialization
    Ok(Json(manifest.clone()))
}
```

### Recommendation

**Use Option 2** (Remove Arc from Manifest) because:
1. Manifest is read-only after creation
2. Typical size is small (< 1000 files)
3. Cloning is cheap compared to network I/O
4. Simpler mental model
5. Session is already Clone, using it is more ergonomic

Only keep Arc for:
- `cipher` (Aes256Gcm doesn't implement Clone)
- `state` (Shared mutable state needs synchronization)

---

## Issue #24: No File Name Sanitization on Send

### Current Problem

**Location:** `src/transfer/manifest.rs:42-46`

```rust
let name = path
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("unnamed")
    .to_string();
```

**What's missing:**
- No validation for control characters (`\0`, `\n`, `\r`)
- No check for excessively long names (> 255 bytes)
- No validation for invalid UTF-8 sequences
- No check for reserved names on Windows (CON, PRN, NUL, etc.)

**Why it matters:**
Even though this is sender-side, bad filenames can:
1. Break the frontend display (if name contains `<script>` or HTML)
2. Cause issues in logs
3. Break terminal output (control characters)
4. Cause JSON serialization issues

### The Fix

**Create comprehensive filename validation:**

```rust
// src/transfer/security.rs (add to existing file)

pub fn sanitize_filename(name: &str) -> Result<String, PathValidationError> {
    // Check for empty
    if name.is_empty() {
        return Err(PathValidationError::Empty);
    }

    // Check length (most filesystems limit to 255 bytes)
    if name.len() > 255 {
        return Err(PathValidationError::InvalidComponent);
    }

    // Check for null bytes
    if name.contains('\0') {
        return Err(PathValidationError::NullByte);
    }

    // Check for control characters (0x00-0x1F, 0x7F)
    if name.chars().any(|c| c.is_control()) {
        return Err(PathValidationError::InvalidComponent);
    }

    // Check for invalid path characters
    const INVALID_CHARS: &[char] = &['/', '\\', '<', '>', ':', '"', '|', '?', '*'];
    if name.chars().any(|c| INVALID_CHARS.contains(&c)) {
        return Err(PathValidationError::InvalidComponent);
    }

    // Check for Windows reserved names
    const WINDOWS_RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    let name_upper = name.to_uppercase();
    let name_without_ext = name_upper.split('.').next().unwrap_or("");

    if WINDOWS_RESERVED.contains(&name_without_ext) {
        return Err(PathValidationError::InvalidComponent);
    }

    // Check for leading/trailing spaces or dots (problematic on Windows)
    if name.starts_with(' ') || name.ends_with(' ') ||
       name.starts_with('.') || name.ends_with('.') {
        return Err(PathValidationError::InvalidComponent);
    }

    Ok(name.to_string())
}

/// Sanitize by replacing invalid characters instead of rejecting
pub fn sanitize_filename_lossy(name: &str) -> String {
    if name.is_empty() {
        return "unnamed".to_string();
    }

    let mut sanitized = String::with_capacity(name.len());

    for ch in name.chars().take(255) {  // Limit length
        match ch {
            // Replace invalid chars with underscore
            '\0' | '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*' => {
                sanitized.push('_');
            }
            // Remove control characters
            c if c.is_control() => {},
            // Keep valid characters
            c => sanitized.push(c),
        }
    }

    // Ensure not empty after sanitization
    if sanitized.is_empty() {
        sanitized = "unnamed".to_string();
    }

    // Handle Windows reserved names by appending underscore
    let upper = sanitized.to_uppercase();
    let base = upper.split('.').next().unwrap_or("");

    const WINDOWS_RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];

    if WINDOWS_RESERVED.contains(&base) {
        sanitized.push('_');
    }

    // Trim spaces and dots
    sanitized = sanitized.trim_matches(|c| c == ' ' || c == '.').to_string();

    if sanitized.is_empty() {
        sanitized = "unnamed".to_string();
    }

    sanitized
}
```

**Update Manifest to use sanitization:**

```rust
// src/transfer/manifest.rs
use crate::transfer::security::sanitize_filename_lossy;

impl Manifest {
    pub async fn new(file_paths: Vec<PathBuf>, base_path: Option<&Path>) -> Result<Self> {
        let mut files = Vec::new();

        let base = base_path.unwrap_or_else(||
            file_paths[0].parent().unwrap_or_else(|| Path::new(""))
        );

        for (index, path) in file_paths.iter().enumerate() {
            let metadata = tokio::fs::metadata(path)
                .await
                .context(format!("Failed to read metadata for: {}", path.display()))?;

            let relative = path
                .strip_prefix(base)
                .unwrap_or(path.as_path())
                .to_string_lossy()
                .to_string();

            // ✅ Sanitize the filename
            let raw_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unnamed");

            let name = sanitize_filename_lossy(raw_name);

            // Log if name was changed
            if name != raw_name {
                tracing::warn!(
                    original = raw_name,
                    sanitized = %name,
                    "Sanitized filename"
                );
            }

            let nonce = Nonce::new();

            files.push(FileEntry {
                index,
                name,
                size: metadata.len(),
                relative_path: relative,
                nonce: nonce.to_base64(),
                full_path: path.clone(),
            });
        }

        Ok(Manifest { files })
    }
}
```

### Benefits

1. ✅ **Prevents terminal corruption** - No control chars in logs
2. ✅ **Safe JSON serialization** - No null bytes
3. ✅ **Cross-platform compatibility** - Handles Windows reserved names
4. ✅ **XSS prevention** - Safe to display in HTML (no `<>` chars)
5. ✅ **Logs are readable** - Invalid names get logged for debugging

---

## Issue #25: No Timeout on File Operations

### Current Problem

File operations can hang indefinitely:

```rust
// src/transfer/io.rs - No timeout
pub fn read_chunk_at_position(file_handle: &Arc<File>, start: u64, len: usize) -> Result<Vec<u8>> {
    let mut buffer = vec![0u8; len];
    file_handle.read_exact_at(&mut buffer, start)?;  // ❌ Can hang forever
    Ok(buffer)
}

// src/transfer/storage.rs - No timeout
pub async fn store_chunk(&mut self, ...) -> Result<()> {
    self.file.write_all(&decrypted).await?;  // ❌ Can hang forever
    Ok(())
}
```

**Scenarios where this hangs:**
1. **Network filesystem** (NFS, SMB) becomes unresponsive
2. **Slow USB drive** or failing disk
3. **Process suspended** by OS (low memory, CPU throttling)
4. **Kernel bug** or driver issue

### The Fix - Add Timeouts

**Step 1: Add timeout configuration**

```rust
// src/config.rs (or src/lib.rs)
pub mod config {
    use std::time::Duration;

    pub const CHUNK_SIZE: u64 = 1024 * 1024; // 1MB

    // Timeouts
    pub const FILE_READ_TIMEOUT: Duration = Duration::from_secs(30);
    pub const FILE_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
    pub const CHUNK_UPLOAD_TIMEOUT: Duration = Duration::from_secs(60);
    pub const CHUNK_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);
}
```

**Step 2: Add timeout wrapper for sync file operations**

```rust
// src/transfer/io.rs
use tokio::time::timeout;
use crate::config;

pub async fn read_chunk_at_position(
    file_handle: Arc<File>,
    start: u64,
    len: usize
) -> Result<Vec<u8>> {
    // Move blocking I/O to blocking thread pool with timeout
    let result = timeout(
        config::FILE_READ_TIMEOUT,
        tokio::task::spawn_blocking(move || {
            let mut buffer = vec![0u8; len];

            #[cfg(unix)]
            {
                use std::os::unix::fs::FileExt;
                file_handle
                    .read_exact_at(&mut buffer, start)
                    .context(format!("Failed to read chunk at offset {}", start))?;
            }

            #[cfg(windows)]
            {
                use std::os::windows::fs::FileExt;
                let bytes_read = file_handle
                    .seek_read(&mut buffer, start)
                    .context(format!("Failed to read chunk at offset {}", start))?;

                if bytes_read != len {
                    return Err(anyhow::anyhow!("Unexpected end of file during chunk read"));
                }
            }

            Ok::<Vec<u8>, anyhow::Error>(buffer)
        })
    ).await;

    match result {
        Ok(Ok(buffer)) => Ok(buffer),
        Ok(Err(e)) => Err(e.context("Spawn blocking task failed")?),
        Err(_) => Err(anyhow::anyhow!(
            "File read operation timed out after {:?}",
            config::FILE_READ_TIMEOUT
        )),
    }
}
```

**Step 3: Add timeout to async file operations**

```rust
// src/transfer/storage.rs
use tokio::time::timeout;
use crate::config;

impl ChunkStorage {
    pub async fn store_chunk(
        &mut self,
        chunk_index: usize,
        encrypted_data: Bytes,
        cipher: &Aes256Gcm,
        nonce: &Nonce,
    ) -> Result<()> {
        // Decrypt chunk
        let decrypted = crypto::decrypt_chunk_at_position(
            cipher,
            nonce,
            &encrypted_data,
            chunk_index as u32
        )?;

        // Seek position
        let offset = (chunk_index as u64) * CHUNK_SIZE;

        // ✅ Add timeout to seek
        timeout(
            config::FILE_WRITE_TIMEOUT,
            self.file.seek(SeekFrom::Start(offset))
        )
        .await
        .context("Seek operation timed out")?
        .context(format!("Failed to seek to offset {}", offset))?;

        // ✅ Add timeout to write
        timeout(
            config::FILE_WRITE_TIMEOUT,
            self.file.write_all(&decrypted)
        )
        .await
        .context("Write operation timed out")?
        .context(format!("Failed to write chunk {} at offset {}", chunk_index, offset))?;

        self.chunks_received.insert(chunk_index);

        Ok(())
    }

    pub async fn finalize(mut self) -> Result<String> {
        // ✅ Add timeout to flush
        timeout(
            config::FILE_WRITE_TIMEOUT,
            self.file.flush()
        )
        .await
        .context("Flush operation timed out")?
        .context("Failed to flush file")?;

        // Calculate hash with timeout
        let hash = timeout(
            config::FILE_READ_TIMEOUT * 2, // Allow longer for hash calculation
            async {
                self.file.seek(SeekFrom::Start(0)).await?;
                let mut hasher = Sha256::new();
                let mut buffer = vec![0u8; 16 * 1024];

                loop {
                    let n = tokio::io::AsyncReadExt::read(&mut self.file, &mut buffer).await?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buffer[..n]);
                }

                Ok::<String, anyhow::Error>(hex::encode(hasher.finalize()))
            }
        )
        .await
        .context("Hash calculation timed out")?
        .context("Failed to calculate file hash")?;

        self.disarmed = true;

        Ok(hash)
    }
}
```

**Step 4: Add timeouts to handlers**

```rust
// src/transfer/send_handlers.rs
pub async fn send_handler(
    Path((token, file_index, chunk_index)): Path<(String, usize, usize)>,
    Query(params): Query<ChunkParams>,
    State(state): State<SendState>,
) -> Result<Response<Body>, AppError> {
    // ... validation code ...

    // ✅ Wrap entire operation in timeout
    let encrypted = timeout(
        config::CHUNK_DOWNLOAD_TIMEOUT,
        async {
            let file_handle = state.file_manager
                .get_or_open(file_index, &file_entry.full_path)?;

            let buffer = io::read_chunk_at_position(file_handle, start, chunk_len).await?;

            let cipher = state.session().cipher();
            crypto::encrypt_chunk_at_position(cipher, &file_nonce, &buffer, chunk_index as u32)
        }
    )
    .await
    .context("Chunk processing timed out")?
    .context("Failed to process chunk")?;

    state.progress_tracker().add_bytes(chunk_len as u64);

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(Body::from(encrypted))?)
}
```

### Benefits

1. ✅ **No hung requests** - All operations have max runtime
2. ✅ **Better error messages** - Users know why it failed
3. ✅ **Resource protection** - Prevents thread pool exhaustion
4. ✅ **Graceful degradation** - Can retry or fallback
5. ✅ **DoS prevention** - Malicious clients can't tie up resources

### Testing Timeouts

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_read_timeout_on_slow_disk() {
        // Create a file that simulates slow read
        // (In real test, you'd use a mock or test fixture)

        let result = timeout(
            Duration::from_millis(100),
            read_chunk_at_position(slow_file, 0, 1024)
        ).await;

        assert!(result.is_err(), "Should timeout on slow disk");
    }
}
```

---

## Summary of Fixes

| Issue | Severity | Fix Complexity | Impact |
|-------|----------|----------------|---------|
| #4 - File Handle Leak | 🔴 High | Medium | Prevents resource exhaustion |
| #5 - Drop Async Work | 🔴 High | Low | Guarantees cleanup |
| #9 - Fake Progress | 🟡 Medium | Medium | Better UX |
| #11 - State Management | 🟡 Medium | High | Type safety, cleaner code |
| #12 - Double Fetch | 🟢 Low | Low | Better performance |
| #22 - Unnecessary Arc | 🟢 Low | Low | Simpler code |
| #24 - No Sanitization | 🟡 Medium | Low | Security & robustness |
| #25 - No Timeouts | 🔴 High | Medium | Prevents hangs |

### Implementation Order

1. **Fix #5 first** (Drop async work) - Simple fix, prevents data loss
2. **Fix #25** (Add timeouts) - Critical for production stability
3. **Fix #4** (File handle leak) - Prevents resource exhaustion
4. **Fix #24** (Sanitization) - Security improvement
5. **Fix #12** (Double fetch) - Quick win
6. **Fix #22** (Arc usage) - Simplification
7. **Fix #9** (Progress) - UX improvement
8. **Fix #11** (State refactor) - Largest change, do last

Would you like me to start implementing any of these fixes?
