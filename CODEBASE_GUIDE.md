# ArchDrop Codebase Guide

A comprehensive guide to understanding the ArchDrop Rust codebase for developers new to Rust and the project.

## Table of Contents

1. [Overview](#overview)
2. [Project Architecture](#project-architecture)
3. [Module Breakdown](#module-breakdown)
4. [Core Concepts](#core-concepts)
5. [Data Flow](#data-flow)
6. [Security Model](#security-model)
7. [Key Rust Patterns Used](#key-rust-patterns-used)

---

## Overview

ArchDrop is a secure peer-to-peer file transfer service for Linux, similar to Apple's AirDrop. It allows users to send and receive files over the network with end-to-end encryption, either locally via HTTPS or over the internet via Cloudflare tunnels.

**Key Features:**
- End-to-end AES-256-GCM encryption
- Zero-knowledge architecture (server never sees unencrypted data)
- Browser-based client (no app installation required on receiver)
- QR code for easy connection
- Supports both local network and internet transfers

---

## Project Architecture

### High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     CLI (main.rs)                           │
│         Parses commands: send/receive, local/tunnel         │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                   Server Layer (server/)                    │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │   Session    │  │    State     │  │    Modes     │     │
│  │  Management  │  │  Management  │  │ Local/Tunnel │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                  Transfer Layer (transfer/)                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │   Manifest   │  │     Send     │  │   Receive    │     │
│  │  (File List) │  │   (Stream)   │  │   (Upload)   │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│               Crypto Layer (crypto.rs, types.rs)            │
│         AES-256-GCM Encryption/Decryption                   │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                     UI Layer (ui/)                          │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │     TUI      │  │   QR Code    │  │   Spinner    │     │
│  │  (ratatui)   │  │  Generator   │  │   Output     │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
└─────────────────────────────────────────────────────────────┘
```

### Directory Structure

```
archdrop-beta/
├── src/
│   ├── main.rs              # Entry point, CLI parsing
│   ├── lib.rs               # Module declarations
│   ├── types.rs             # Core types (EncryptionKey, Nonce)
│   ├── crypto.rs            # Encryption/decryption functions
│   ├── tunnel.rs            # Cloudflare tunnel integration
│   ├── server/
│   │   ├── mod.rs           # Server initialization
│   │   ├── modes.rs         # Local HTTPS vs Tunnel mode
│   │   ├── session.rs       # Session management
│   │   ├── state.rs         # Application state
│   │   ├── utils.rs         # Helper functions
│   │   └── web.rs           # HTML/JS templates
│   ├── transfer/
│   │   ├── mod.rs           # Transfer module exports
│   │   ├── manifest.rs      # File metadata structure
│   │   ├── send.rs          # Send mode handlers
│   │   ├── receive.rs       # Receive mode handlers
│   │   ├── storage.rs       # Chunk storage management
│   │   └── util.rs          # Path validation, hashing
│   └── ui/
│       ├── mod.rs           # UI module exports
│       ├── tui.rs           # Terminal UI (ratatui)
│       ├── qr.rs            # QR code generation
│       └── output.rs        # Console output helpers
├── templates/               # HTML/JavaScript templates
├── tests/                   # Test files
└── Cargo.toml              # Dependencies
```

---

## Module Breakdown

### 1. Entry Point: `main.rs`

**Purpose:** Parse command-line arguments and route to appropriate server mode.

**Key Concepts:**

```rust
#[derive(Parser)]
#[command(name = "archdrop")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
```

- **`#[derive(Parser)]`**: This is a **derive macro** from the `clap` crate. It automatically generates code to parse command-line arguments into the struct.
- **Enums for Commands**: Rust enums are powerful - they can hold different data for each variant.

```rust
enum Commands {
    Send { paths: Vec<PathBuf>, local: bool },
    Receive { destination: PathBuf, local: bool },
}
```

**Learning Point - `match` expressions:**
Rust's `match` is exhaustive - the compiler ensures you handle all cases:

```rust
match cli.command {
    Commands::Send { paths, local } => { /* ... */ }
    Commands::Receive { destination, local } => { /* ... */ }
    // Compiler error if you forget a case!
}
```

**Architecture Decision:**
- ✅ **Benefit**: Type-safe CLI parsing with clear error messages
- ✅ **Benefit**: No runtime string parsing errors
- ⚠️ **Cost**: Additional compile-time dependency (clap)

**Async Main:**
```rust
#[tokio::main]
async fn main() -> Result<()> { }
```

- The `#[tokio::main]` macro transforms `async fn main()` into a regular `fn main()` that initializes the Tokio runtime.
- `Result<()>` return type allows using `?` operator for error propagation.

---

### 2. Core Types: `types.rs`

**Purpose:** Define cryptographic types used throughout the application.

#### EncryptionKey

```rust
#[derive(Debug, Clone)]
pub struct EncryptionKey([u8; 32]);
```

**Learning Points:**

1. **Newtype Pattern**: Wrapping `[u8; 32]` in a struct provides type safety. You can't accidentally pass the wrong array type.

2. **Fixed-Size Arrays**: `[u8; 32]` is a stack-allocated array (256 bits for AES-256).

3. **Random Generation**:
```rust
pub fn new() -> Self {
    let mut key = [0u8; 32];
    OsRng::default().fill_bytes(&mut key);
    Self(key)
}
```
   - `OsRng` uses the operating system's cryptographically secure random number generator.
   - `fill_bytes(&mut key)` mutably borrows the array to fill it with random bytes.

4. **Base64 Encoding**:
```rust
pub fn to_base64(&self) -> String {
    general_purpose::URL_SAFE_NO_PAD.encode(&self.0)
}
```
   - Uses URL-safe Base64 without padding for inclusion in URLs.
   - `&self.0` accesses the inner array through the tuple struct syntax.

**Architecture Decision:**
- ✅ **Benefit**: Type safety prevents mixing keys and nonces
- ✅ **Benefit**: Encapsulation - crypto details hidden from callers
- ⚠️ **Cost**: Small memory overhead for the wrapper (negligible)

#### Nonce

```rust
#[derive(Debug, Clone)]
pub struct Nonce([u8; 7]);
```

**Why 7 bytes?** AES-GCM requires a 12-byte nonce. ArchDrop uses a structure:
- 7 bytes: Random nonce base
- 4 bytes: Counter (allows encrypting multiple chunks)
- 1 byte: Flag (for future use)

This design is implemented in `crypto.rs`:

```rust
let mut full_nonce = [0u8; 12];
full_nonce[..7].copy_from_slice(nonce_base.as_bytes());
full_nonce[7..11].copy_from_slice(&counter.to_be_bytes());
```

**Architecture Decision:**
- ✅ **Benefit**: Unique nonce per chunk prevents nonce reuse (critical for GCM security)
- ✅ **Benefit**: Deterministic nonce construction allows random-access decryption
- ⚠️ **Cost**: More complex than simple incrementing counter

---

### 3. Cryptography: `crypto.rs`

**Purpose:** Encrypt and decrypt file chunks using AES-256-GCM.

#### Encryption Function

```rust
pub fn encrypt_chunk_at_position(
    cipher: &Aes256Gcm,
    nonce_base: &Nonce,
    plaintext: &[u8],
    counter: u32,
) -> Result<Vec<u8>>
```

**Learning Points:**

1. **Borrowing vs Ownership**:
   - `&Aes256Gcm` - Borrows the cipher (no ownership transfer)
   - `&[u8]` - Borrows a slice of bytes (the plaintext)
   - `Vec<u8>` - Returns owned data (caller gets ownership)

2. **Generic Array Construction**:
```rust
let nonce_array = GenericArray::from_slice(&full_nonce);
```
   - `GenericArray` from the `aes_gcm` crate is a fixed-size array type used for compile-time size checking.

3. **Error Handling**:
```rust
cipher
    .encrypt(nonce_array, plaintext)
    .map_err(|e| anyhow::anyhow!("Encryption failed: {:?}", e))
```
   - `.map_err()` converts the error type from the AES library to `anyhow::Error`.
   - `anyhow!` macro creates an error with a formatted message.

**AES-GCM Security Properties:**
- **Authenticated Encryption**: Provides both confidentiality and integrity
- **AEAD (Authenticated Encryption with Associated Data)**: Each chunk has an authentication tag
- **Prevents**: Tampering, bit-flipping, chunk reordering attacks

**Architecture Decision:**
- ✅ **Benefit**: Industry-standard encryption (AES-256-GCM)
- ✅ **Benefit**: Per-chunk authentication catches corruption early
- ✅ **Benefit**: Position-based nonces allow parallel/random-access decryption
- ⚠️ **Cost**: 16-byte authentication tag per chunk (overhead)

---

### 4. Server Module: `server/`

#### 4.1 Session Management: `session.rs`

**Purpose:** Track transfer sessions with token-based authentication.

```rust
pub struct Session {
    token: String,
    active: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
    manifest: Option<Arc<Manifest>>,      // Send mode
    destination: Option<PathBuf>,          // Receive mode
    session_key: EncryptionKey,
    session_nonce: Nonce,
    cipher: Arc<Aes256Gcm>,
}
```

**Learning Points:**

1. **`Arc<T>` - Atomic Reference Counting**:
```rust
active: Arc<AtomicBool>
```
   - Allows sharing data across threads safely.
   - `Arc` is immutable by default - you can't modify data through it.
   - `AtomicBool` provides lock-free thread-safe mutations.

2. **Atomic Operations**:
```rust
pub fn claim(&self, token: &str) -> bool {
    self.active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}
```
   - **`compare_exchange`**: Atomic "compare and swap" - only succeeds once
   - **Single-use sessions**: First caller wins, others are rejected
   - **Memory Ordering**: `AcqRel`/`Acquire` ensures proper synchronization across threads

3. **Option Types for Mode-Specific Data**:
```rust
manifest: Option<Arc<Manifest>>,    // Some() in send mode, None in receive
destination: Option<PathBuf>,        // Some() in receive mode, None in send
```

**Why this design?**
- Could have used an enum with two variants, but this approach is simpler
- Methods like `get_manifest()` return `Option` to handle both cases

**Architecture Decision:**
- ✅ **Benefit**: Lock-free atomic operations (no mutex contention)
- ✅ **Benefit**: Single-use tokens prevent replay attacks
- ✅ **Benefit**: UUID tokens are cryptographically random
- ⚠️ **Cost**: Session not explicitly deleted (relies on process exit)

#### 4.2 Application State: `state.rs`

**Purpose:** Shared application state passed to Axum handlers.

```rust
#[derive(Clone)]
pub struct AppState {
    pub session: Session,
    pub progress_sender: watch::Sender<f64>,
    pub receive_sessions: Arc<DashMap<String, ReceiveSession>>,
}
```

**Learning Points:**

1. **`DashMap` - Concurrent HashMap**:
```rust
receive_sessions: Arc<DashMap<String, ReceiveSession>>
```
   - Standard `HashMap` is not thread-safe
   - `DashMap` provides lock-free concurrent access
   - Allows multiple files to be uploaded simultaneously

2. **`watch` Channel**:
```rust
progress_sender: watch::Sender<f64>
```
   - Tokio's `watch` is a **broadcast channel** that always holds the latest value
   - Multiple receivers can subscribe to progress updates
   - Used to update the TUI with transfer progress

3. **Clone Semantics**:
```rust
#[derive(Clone)]
```
   - Cloning `AppState` clones the `Arc` pointers (cheap)
   - Doesn't duplicate the actual data
   - Axum requires `State` to be `Clone` for sharing across handlers

**Architecture Decision:**
- ✅ **Benefit**: Concurrent file uploads with DashMap
- ✅ **Benefit**: Cheap cloning with Arc
- ✅ **Benefit**: Real-time progress updates via watch channel
- ⚠️ **Cost**: DashMap has small memory overhead vs regular HashMap

#### 4.3 Server Modes: `modes.rs`

**Purpose:** Start server in either Local (HTTPS) or Tunnel mode.

```rust
pub async fn start_https(server: ServerInstance, direction: ServerDirection) -> Result<u16>
pub async fn start_tunnel(server: ServerInstance, direction: ServerDirection) -> Result<u16>
```

**Flow for Tunnel Mode:**

1. **Start local HTTP server** on random port
2. **Start Cloudflare tunnel** pointing to that port
3. **Wait for tunnel URL** from Cloudflare's metrics API
4. **Build access URL** with token and keys in fragment
5. **Display QR code** and start TUI

**Learning Points:**

1. **Consuming vs Borrowing**:
```rust
pub async fn start_tunnel(server: ServerInstance, ...) -> Result<u16>
```
   - `server` is moved (consumed) into the function
   - Caller can't use `server` after calling this
   - Necessary because we spawn it on a background task

2. **Selective Cloning Before Consumption**:
```rust
let session = server.session.clone();
let display_name = server.display_name.clone();
let progress_receiver = server.progress_receiver();
// Now we can consume server
let (port, server_handle) = start_local_server(server, Protocol::Http).await?;
```

3. **URL Fragment for Keys**:
```rust
let url = format!(
    "{}/{}/{}#key={}&nonce={}",
    tunnel_url, service, session.token(),
    session.session_key_b64(), session.session_nonce_b64()
);
```
   - Everything after `#` is the **fragment** - never sent to the server
   - Browser JavaScript reads the keys from `window.location.hash`
   - **Zero-knowledge**: Server never sees encryption keys

**Architecture Decision:**
- ✅ **Benefit**: Zero-knowledge - keys never touch the server
- ✅ **Benefit**: Tunnel mode works behind NAT/firewalls
- ✅ **Benefit**: Local mode faster (no tunnel overhead)
- ⚠️ **Cost**: Tunnel mode requires external `cloudflared` binary
- ⚠️ **Cost**: Tunnel adds latency (~100-200ms depending on location)

---

### 5. Transfer Module: `transfer/`

#### 5.1 Manifest: `manifest.rs`

**Purpose:** Create metadata about files being sent.

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct FileEntry {
    pub index: usize,
    pub name: String,
    #[serde(skip)]
    pub full_path: PathBuf,
    pub relative_path: String,
    pub size: u64,
    pub nonce: String,
}
```

**Learning Points:**

1. **Serde Attributes**:
```rust
#[serde(skip)]
pub full_path: PathBuf,
```
   - `#[serde(skip)]` excludes field from JSON serialization
   - Security: Don't leak server's full filesystem paths to client
   - Only `relative_path` is sent to the browser

2. **Per-File Nonces**:
```rust
let nonce = Nonce::new();  // Unique for each file
```
   - Each file gets its own random nonce base
   - Combined with chunk counter for per-chunk unique nonces
   - Prevents nonce reuse even across files

3. **Async File Metadata**:
```rust
let metadata = tokio::fs::metadata(path).await?;
```
   - Tokio's async filesystem operations don't block the thread
   - `await?` waits for the result and propagates errors

**Architecture Decision:**
- ✅ **Benefit**: Per-file nonces provide cryptographic isolation
- ✅ **Benefit**: Relative paths support directory structures
- ✅ **Benefit**: Pre-computed manifest allows progress tracking
- ⚠️ **Cost**: Manifests for huge file lists could be large (but rare)

#### 5.2 Send Handlers: `send.rs`

**Purpose:** Stream encrypted file chunks to the browser.

```rust
pub async fn send_handler(
    Path((token, file_index, chunk_index)): Path<(String, usize, usize)>,
    State(state): State<AppState>,
) -> Result<Response<Body>, AppError>
```

**Learning Points:**

1. **Axum Extractors**:
```rust
Path((token, file_index, chunk_index)): Path<(String, usize, usize)>
```
   - Axum automatically extracts and parses URL path parameters
   - Maps `/send/:token/:file_index/chunk/:chunk_index` to variables
   - Type conversion happens automatically (string to usize)

2. **Token Claiming Logic**:
```rust
if file_index == 0 && chunk_index == 0 {
    if !state.session.claim(&token) {
        return Err(anyhow::anyhow!("Session already claimed").into());
    }
}
```
   - **First chunk claims the session** - single-use tokens
   - Subsequent chunks check `is_active(&token)`
   - Prevents multiple clients from downloading simultaneously

3. **Buffered File Reading**:
```rust
let file = tokio::fs::File::open(&file_entry.full_path).await?;
let mut reader = BufReader::with_capacity(CHUNK_SIZE as usize * 2, file);
reader.seek(SeekFrom::Start(start)).await?;
```
   - `BufReader` reduces syscalls by buffering reads
   - `2 * CHUNK_SIZE` buffer provides read-ahead
   - `seek` allows random-access reading for any chunk

4. **Chunk Calculation**:
```rust
let start = chunk_index as u64 * CHUNK_SIZE;
let end = std::cmp::min(start + CHUNK_SIZE, file_entry.size);
```
   - Last chunk may be smaller than `CHUNK_SIZE`
   - `std::cmp::min` ensures we don't read past EOF

**Architecture Decision:**
- ✅ **Benefit**: Streaming reduces memory usage (no need to load entire file)
- ✅ **Benefit**: Random chunk access allows parallel downloads
- ✅ **Benefit**: Buffered reads improve performance
- ⚠️ **Cost**: Each chunk requires encryption overhead (CPU)

#### 5.3 Receive Handlers: `receive.rs`

**Purpose:** Accept encrypted chunks from browser and decrypt to disk.

```rust
pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<axum::Json<Value>, AppError>
```

**Learning Points:**

1. **Multipart Form Data**:
```rust
async fn parse_chunk_upload(mut multipart: Multipart) -> anyhow::Result<ChunkUpload> {
    while let Some(field) = multipart.next_field().await? {
        match field.name() {
            Some("chunk") => chunk_data = Some(field.bytes().await?.to_vec()),
            Some("chunkIndex") => chunk_index = Some(field.text().await?.parse()?),
            // ...
        }
    }
}
```
   - Browser uploads chunks as multipart form data
   - Rust iterates over fields and extracts each one
   - Text fields are parsed from strings to numbers

2. **Per-File Session Management**:
```rust
let file_id = hash_path(&chunk.relative_path);
let is_new_file = !state.receive_sessions.contains_key(&file_id);
```
   - Hash the path to create a unique file ID
   - Each file gets its own `ReceiveSession` in the `DashMap`
   - Allows concurrent uploads of multiple files

3. **Duplicate Detection**:
```rust
if session.storage.has_chunk(chunk.chunk_index) {
    return Ok(axum::Json(json!({
        "success": true,
        "duplicate": true,
        "chunk": chunk.chunk_index,
    })));
}
```
   - Network retries may send the same chunk twice
   - Gracefully handle duplicates rather than erroring

4. **Finalization**:
```rust
pub async fn finalize_upload(...) -> Result<axum::Json<Value>, AppError> {
    let (_key, session) = state.receive_sessions.remove(&file_id).ok_or(...)?;

    if session.storage.chunk_count() != session.total_chunks {
        return Err(anyhow::anyhow!("Incomplete upload: ...")
    }

    let computed_hash = session.storage.finalize().await?;
}
```
   - Browser calls finalize after all chunks uploaded
   - Verifies all chunks received before finalizing
   - Computes SHA-256 hash for integrity verification

**Architecture Decision:**
- ✅ **Benefit**: Concurrent multi-file uploads with separate sessions
- ✅ **Benefit**: Out-of-order chunk delivery supported
- ✅ **Benefit**: Duplicate chunk handling improves reliability
- ✅ **Benefit**: Final hash verification ensures integrity
- ⚠️ **Cost**: Partial files kept in memory until finalized

#### 5.4 Chunk Storage: `storage.rs`

**Purpose:** Manage writing decrypted chunks to disk with error handling.

```rust
pub struct ChunkStorage {
    file: File,
    path: PathBuf,
    chunks_received: HashSet<usize>,
    disarmed: bool,  // RAII guard
}
```

**Learning Points:**

1. **RAII (Resource Acquisition Is Initialization)**:
```rust
impl Drop for ChunkStorage {
    fn drop(&mut self) {
        if !self.disarmed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
```
   - `Drop` trait is called when value goes out of scope
   - If upload fails mid-way, partially written file is auto-deleted
   - `disarmed = true` prevents deletion on success
   - **"Panic-safe"** - cleanup happens even if code panics

2. **Out-of-Order Writes**:
```rust
pub async fn store_chunk(&mut self, chunk_index: usize, ...) -> Result<()> {
    let offset = (chunk_index as u64) * CHUNK_SIZE;
    self.file.seek(SeekFrom::Start(offset)).await?;
    self.file.write_all(&decrypted).await?;
}
```
   - Chunks may arrive in any order
   - Use `seek` to write to the correct position
   - File grows with "holes" that get filled in

3. **Finalization with Hash**:
```rust
pub async fn finalize(mut self) -> Result<String> {
    self.file.flush().await?;

    self.file.seek(SeekFrom::Start(0)).await?;
    let mut hasher = Sha256::new();
    // Read entire file and hash it

    self.disarmed = true;  // Prevent deletion
    Ok(hex::encode(hasher.finalize()))
}
```
   - `flush()` ensures all data is written to disk
   - Hash entire file from start to verify integrity
   - **Consumes `self`** - can only finalize once
   - Return the hex-encoded SHA-256 hash

**Architecture Decision:**
- ✅ **Benefit**: RAII cleanup prevents orphaned partial files
- ✅ **Benefit**: Out-of-order writes maximize throughput
- ✅ **Benefit**: Final hash verification ensures file integrity
- ✅ **Benefit**: Async I/O prevents blocking
- ⚠️ **Cost**: Files with "holes" may not be space-efficient on all filesystems
- ⚠️ **Cost**: Drop uses sync I/O (but only on error path)

---

### 6. Tunnel Integration: `tunnel.rs`

**Purpose:** Manage Cloudflare tunnel lifecycle.

```rust
pub struct CloudflareTunnel {
    process: Child,
    url: String,
}
```

**Learning Points:**

1. **Process Management**:
```rust
let mut child = Command::new("cloudflared")
    .args(&[...])
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()?;
```
   - Spawns external `cloudflared` process
   - Null stdout/stderr to suppress output
   - Returns `Child` handle for process management

2. **Polling for URL**:
```rust
async fn wait_for_url(metrics_port: u16) -> Result<String> {
    let client = reqwest::Client::new();
    let api_url = format!("http://localhost:{}/quicktunnel", metrics_port);

    for _ in 0..60 {  // 12 second timeout
        match client.get(&api_url).send().await {
            Ok(res) if res.status().is_success() => {
                let json: QuickTunnelResponse = res.json().await?;
                return Ok(format!("https://{}", json.hostname));
            }
            _ => { }
        }
        sleep(Duration::from_millis(200)).await;
    }
    bail!("Timed out")
}
```
   - Cloudflare tunnel exposes metrics API
   - Poll until hostname is ready
   - 200ms retry interval, 12 second max

3. **RAII for Process Cleanup**:
```rust
impl Drop for CloudflareTunnel {
    fn drop(&mut self) {
        let _ = self.process.start_kill();
    }
}
```
   - Tunnel automatically killed when struct is dropped
   - Ensures cleanup even if main process panics
   - `start_kill()` sends SIGTERM (graceful)

**Architecture Decision:**
- ✅ **Benefit**: Works behind NAT/firewalls
- ✅ **Benefit**: Free tier of Cloudflare tunnels
- ✅ **Benefit**: Automatic cleanup via Drop
- ⚠️ **Cost**: Requires `cloudflared` to be installed
- ⚠️ **Cost**: Adds network latency
- ⚠️ **Cost**: Depends on Cloudflare service availability

---

### 7. UI Module: `ui/`

#### TUI: `tui.rs`

**Purpose:** Display progress, QR code, and file info in the terminal.

**Learning Points:**

1. **Terminal Mode Switching**:
```rust
enable_raw_mode()?;
execute!(stdout, EnterAlternateScreen)?;
// ... run UI ...
disable_raw_mode()?;
execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
```
   - Raw mode: terminal doesn't echo keypresses
   - Alternate screen: UI doesn't clobber terminal history
   - Proper cleanup ensures terminal is restored

2. **Reactive UI Loop**:
```rust
loop {
    let progress = *self.progress.borrow();  // Read latest progress

    terminal.draw(|f| { self.render_layout(f, progress); })?;

    if event::poll(Duration::from_millis(100))? {
        // Check for 'q', 'c', or Esc to quit
    }

    if progress >= 100.0 { break; }
}
```
   - Read progress from `watch` channel
   - Redraw UI every 100ms
   - Exit on completion or user keypress

3. **Responsive Layout**:
```rust
fn render_layout(&self, f: &mut Frame, progress: f64) {
    let width = f.size().width;
    match width {
        w if w >= 112 => self.render_wide(f, progress),
        w if w >= 65 => self.render_medium(f, progress),
        _ => self.render_compact(f, progress),
    }
}
```
   - Adapts layout to terminal width
   - Wide: side-by-side, Medium: stacked, Compact: minimal

**Architecture Decision:**
- ✅ **Benefit**: Rich terminal UI improves UX
- ✅ **Benefit**: Responsive layout works on any terminal size
- ✅ **Benefit**: Real-time progress updates
- ⚠️ **Cost**: TUI library adds dependency size
- ⚠️ **Cost**: Raw mode can cause issues if process crashes

---

## Core Concepts

### 1. Ownership and Borrowing

Rust's ownership system prevents memory safety bugs without garbage collection.

**Rules:**
1. Each value has one owner
2. When owner goes out of scope, value is dropped
3. You can borrow values with references (`&T` or `&mut T`)

**Example from `send.rs`:**
```rust
pub async fn send_handler(
    Path((token, file_index, chunk_index)): Path<(String, usize, usize)>,
    State(state): State<AppState>,
) -> Result<Response<Body>, AppError> {
    let file_entry = state.session.get_file(file_index)  // Borrows session
        .ok_or_else(|| anyhow::anyhow!("invalid file index"))?;
    // state.session is still usable here
}
```

### 2. Error Handling with `Result`

Rust has no exceptions. Errors are values.

```rust
pub async fn new(dest_path: PathBuf) -> Result<Self> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&dest_path)
        .await?;  // ? propagates errors up

    Ok(Self { file, ... })
}
```

- `Result<T, E>` is either `Ok(T)` or `Err(E)`
- `?` operator returns early if `Err`, unwraps if `Ok`
- Forces explicit error handling

### 3. Async/Await with Tokio

ArchDrop uses Tokio for async I/O.

**Async Functions:**
```rust
pub async fn store_chunk(&mut self, ...) -> Result<()> {
    self.file.seek(SeekFrom::Start(offset)).await?;
    self.file.write_all(&decrypted).await?;
    Ok(())
}
```

- `async fn` returns a `Future`
- `.await` suspends execution until Future is ready
- Tokio runtime schedules Futures on thread pool

**Why Async?**
- Handle many connections without thread-per-connection
- I/O operations don't block the entire thread

### 4. Traits

Traits define shared behavior (like interfaces).

**Example - `Drop` trait:**
```rust
impl Drop for ChunkStorage {
    fn drop(&mut self) {
        if !self.disarmed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
```

- `Drop` is called when value goes out of scope
- Enables RAII patterns

**Example - `Clone` trait:**
```rust
#[derive(Clone)]
pub struct AppState { ... }
```

- `#[derive(Clone)]` auto-generates cloning code
- Required by Axum for state sharing

### 5. Pattern Matching

Rust's `match` is exhaustive and powerful.

```rust
match field.name() {
    Some("chunk") => chunk_data = Some(field.bytes().await?.to_vec()),
    Some("chunkIndex") => chunk_index = Some(field.text().await?.parse()?),
    Some("totalChunks") => total_chunks = Some(field.text().await?.parse()?),
    _ => {}  // Catch-all for unknown fields
}
```

- Compiler ensures all cases are handled
- Can destructure enums and extract data

---

## Data Flow

### Send Mode Flow

```
┌─────────────┐
│   User CLI  │  archdrop send file.txt
└──────┬──────┘
       │
       ▼
┌─────────────────────┐
│  Create Manifest    │  Scan files, generate per-file nonces
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│ Generate Session    │  Create session token, AES key, nonce
│   Keys & Token      │
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│  Start Web Server   │  Axum HTTP(S) server with routes
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│ Start Tunnel (opt)  │  Cloudflare tunnel if not --local
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│   Display QR Code   │  URL with keys in fragment
└─────────────────────┘

   [User scans QR]
         │
         ▼
┌─────────────────────┐
│  Browser Requests   │  GET /send/:token/manifest
│     Manifest        │
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│  Server Sends JSON  │  List of files with sizes & nonces
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│ Browser Downloads   │  GET /send/:token/:file/:chunk
│  Chunks in Parallel │  (multiple concurrent requests)
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│ Server: Read Chunk  │  Seek to position, read from disk
│  Encrypt & Return   │  Encrypt with AES-GCM, send bytes
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│ Browser: Decrypt    │  Web Crypto API decrypts chunks
│   & Save to Disk    │  File System Access API saves
└─────────────────────┘
```

### Receive Mode Flow

```
┌─────────────┐
│   User CLI  │  archdrop receive ~/Downloads
└──────┬──────┘
       │
       ▼
┌─────────────────────┐
│ Generate Session    │  Create session token, AES key, nonce
│   Keys & Token      │
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│  Start Web Server   │  Axum HTTP(S) server with routes
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│   Display QR Code   │  URL with keys in fragment
└─────────────────────┘

   [User scans QR]
         │
         ▼
┌─────────────────────┐
│ Browser: User       │  File picker UI
│  Selects Files      │
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│ Browser: Encrypt    │  Web Crypto API encrypts chunks
│  Chunks in Parallel │  with per-file nonces
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│ Browser: Upload     │  POST /receive/:token/chunk
│  via Multipart      │  (concurrent uploads)
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│ Server: Decrypt     │  AES-GCM decrypt chunk
│  & Write to Disk    │  Seek to position, write
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│ Browser: Finalize   │  POST /receive/:token/finalize
└──────┬──────────────┘
       │
       ▼
┌─────────────────────┐
│ Server: Compute     │  Read entire file, compute SHA-256
│  File Hash          │  Verify all chunks received
└─────────────────────┘
```

---

## Security Model

### 1. Zero-Knowledge Architecture

**Keys never reach the server:**
```
URL: https://example.com/send/TOKEN#key=BASE64&nonce=BASE64
                                  ^
                                  |
                         Fragment (never sent to server)
```

- Browser JavaScript extracts keys from `window.location.hash`
- Server only sees the TOKEN
- Even if server is compromised, files remain encrypted

### 2. Authentication

**Single-Use Tokens:**
```rust
pub fn claim(&self, token: &str) -> bool {
    self.active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}
```

- Atomic compare-and-swap ensures only first caller succeeds
- UUIDv4 tokens are cryptographically random (128 bits)
- Tokens are valid for single transfer only

### 3. Path Traversal Protection

```rust
pub fn validate_path(path: &str) -> Result<(), PathValidationError> {
    if path.contains('\0') { return Err(...); }
    if path.is_absolute() { return Err(...); }

    for component in path.components() {
        match component {
            Component::ParentDir => return Err(PathValidationError::ContainsParentDir),
            // ...
        }
    }
    Ok(())
}
```

- Rejects absolute paths (`/etc/passwd`)
- Rejects parent directory traversal (`../../../`)
- Rejects null bytes (filesystem bypass)
- All uploaded files stay within destination directory

### 4. Encryption

**AES-256-GCM with Unique Nonces:**
- 256-bit keys (2^256 keyspace)
- Authenticated encryption (integrity + confidentiality)
- Unique nonce per chunk (7-byte base + 4-byte counter)
- 16-byte authentication tag per chunk

**Nonce Structure:**
```
[7 bytes random][4 bytes counter][1 byte flag] = 12 bytes total
```

This ensures:
- No nonce reuse across chunks
- No nonce reuse across files
- Deterministic nonces allow random-access decryption

---

## Key Rust Patterns Used

### 1. Newtype Pattern

Wrap primitive types for type safety:
```rust
pub struct EncryptionKey([u8; 32]);
pub struct Nonce([u8; 7]);
```

**Why?** Prevents mixing up keys and nonces.

### 2. RAII (Resource Acquisition Is Initialization)

Use Drop trait for automatic cleanup:
```rust
impl Drop for ChunkStorage {
    fn drop(&mut self) {
        if !self.disarmed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
```

**Why?** Ensures cleanup even on panics.

### 3. Type State Pattern

```rust
pub struct Session {
    active: Arc<AtomicBool>,
    completed: Arc<AtomicBool>,
}
```

State transitions enforced at runtime:
- Inactive → Active (via `claim()`)
- Active → Completed (via `complete()`)

### 4. Builder Pattern (via Clap)

```rust
#[derive(Parser)]
#[command(name = "archdrop")]
struct Cli { ... }
```

Clap uses builder pattern internally for CLI construction.

### 5. Error Conversion with `From` Trait

```rust
impl<E> From<E> for AppError
where E: Into<anyhow::Error> { ... }
```

Allows using `?` operator with any error type.

### 6. Channels for Communication

**Watch Channel (latest value broadcast):**
```rust
let (progress_sender, progress_receiver) = tokio::sync::watch::channel(0.0);
```

Used for progress updates to TUI.

### 7. Atomic Operations

```rust
self.active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
```

Lock-free synchronization for session claiming.

---

## Common Pitfalls for New Rust Developers

### 1. Borrowing vs Ownership

**Error:**
```rust
let manifest = state.session.get_manifest();  // Returns Option<&Manifest>
do_something(manifest);
do_something_else(manifest);  // ERROR: manifest already moved
```

**Fix:** Clone if needed, or use references:
```rust
let manifest = state.session.get_manifest().cloned();  // Clone the manifest
```

### 2. Async Functions Must Be .await-ed

**Error:**
```rust
let file = tokio::fs::File::open(path);  // Wrong - returns Future
```

**Fix:**
```rust
let file = tokio::fs::File::open(path).await?;  // Await the Future
```

### 3. Mutable References

Only one mutable reference at a time:

**Error:**
```rust
let ref1 = &mut storage;
let ref2 = &mut storage;  // ERROR: can't borrow as mutable twice
```

This prevents data races at compile time.

### 4. Option and Result Handling

**Error:**
```rust
let file = state.session.get_file(index);  // Returns Option<&FileEntry>
file.size;  // ERROR: file is Option, not FileEntry
```

**Fix:**
```rust
let file = state.session.get_file(index).ok_or(...)?;
file.size;  // Now file is &FileEntry
```

---

## Testing Architecture

### Test Organization

```
tests/
├── crypto_tests.rs         # Encryption/decryption tests
├── manifest_tests.rs       # File manifest creation tests
├── session_tests.rs        # Session management tests
└── traverse/              # Path traversal tests
    └── util.rs
```

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test crypto_tests

# With output
cargo test -- --nocapture
```

---

## Performance Considerations

### 1. Chunking Strategy

**CHUNK_SIZE = 1MB** (defined in `lib.rs`)

**Tradeoffs:**
- ✅ Smaller chunks: Better parallelism, lower memory
- ⚠️ Smaller chunks: More overhead (HTTP + crypto per chunk)
- ✅ Larger chunks: Fewer requests, less overhead
- ⚠️ Larger chunks: Higher memory usage, less parallelism

1MB is a good balance for most use cases.

### 2. Buffered I/O

```rust
let mut reader = BufReader::with_capacity(CHUNK_SIZE * 2, file);
```

2MB buffer provides read-ahead for the OS.

### 3. Concurrent Transfers

**Send:** Multiple chunks downloaded in parallel by browser
**Receive:** DashMap allows concurrent file uploads

### 4. Async I/O

All I/O is async (Tokio), allowing many connections without thread-per-connection overhead.

---

## Future Improvements

Areas for potential enhancement:

### 1. Resumable Transfers

Currently, interrupted transfers must restart from beginning.

**Possible Solution:**
- Store chunk metadata in database
- Allow resuming from last successful chunk

### 2. Progress Reporting

Current progress is basic (0-100%).

**Possible Solution:**
- Track bytes transferred
- Calculate transfer rate
- Estimate time remaining

### 3. Multi-Device Pairing

Currently one-to-one transfers only.

**Possible Solution:**
- Allow multiple receivers
- Broadcast chunks to all connected clients

### 4. Compression

Files are not compressed before encryption.

**Possible Solution:**
- Add optional zstd compression
- Compress before encrypting

### 5. Authentication

Currently relies on secret URLs only.

**Possible Solution:**
- Add optional password protection
- Key derivation from password + random salt

---

## Glossary

**AES-GCM**: Advanced Encryption Standard with Galois/Counter Mode - authenticated encryption algorithm

**AEAD**: Authenticated Encryption with Associated Data - encryption that also verifies integrity

**Arc**: Atomic Reference Counted pointer - allows shared ownership across threads

**Async**: Asynchronous programming - allows non-blocking I/O operations

**Axum**: Web framework for Rust built on Tokio

**Borrowing**: Temporarily accessing data without taking ownership

**DashMap**: Concurrent HashMap that allows lock-free access

**Future**: Rust's representation of an asynchronous computation

**Nonce**: "Number used once" - ensures unique encryption per operation

**Ownership**: Rust's system for managing memory without garbage collection

**RAII**: Resource Acquisition Is Initialization - automatic cleanup pattern

**Tokio**: Asynchronous runtime for Rust

**UUID**: Universally Unique Identifier - cryptographically random token

**Watch Channel**: Broadcast channel that holds the latest value

---

## Conclusion

ArchDrop demonstrates several key Rust patterns:

1. **Type Safety**: Newtype pattern prevents mixing keys/nonces
2. **Memory Safety**: Ownership prevents data races
3. **Resource Management**: RAII ensures cleanup
4. **Error Handling**: Result type forces explicit handling
5. **Concurrency**: Async/await with Tokio for scalability
6. **Security**: Zero-knowledge with keys in URL fragments

The architecture prioritizes:
- **Security**: End-to-end encryption, path validation
- **Performance**: Async I/O, concurrent transfers, chunking
- **Reliability**: RAII cleanup, duplicate detection, integrity hashing
- **Usability**: QR codes, responsive TUI, automatic tunneling

By understanding these patterns and the data flow, new developers can effectively navigate and contribute to the codebase.
