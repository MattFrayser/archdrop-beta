# ArchDrop Architecture Documentation

## Table of Contents

1. [System Overview](#system-overview)
2. [Design Philosophy](#design-philosophy)
3. [Architecture Patterns](#architecture-patterns)
4. [Component Architecture](#component-architecture)
5. [Network Architecture](#network-architecture)
6. [Data Flow](#data-flow)
7. [Security Architecture](#security-architecture)
8. [Performance Architecture](#performance-architecture)
9. [Deployment Architecture](#deployment-architecture)

---

## System Overview

ArchDrop is a secure peer-to-peer file transfer system designed for Linux that enables encrypted file transfers between devices without requiring cloud intermediaries.

### Core Principles

1. **Zero-Knowledge Architecture**: Server never has access to encryption keys
2. **End-to-End Encryption**: All data encrypted on sender, decrypted on receiver
3. **Single-Use Sessions**: Each transfer creates a unique, non-reusable session
4. **Progressive Enhancement**: Works locally (fast) or via tunnel (accessible)

### System Context

```
┌─────────────────────────────────────────────────────────────────┐
│                         User's Device                           │
│                                                                 │
│  ┌──────────────┐         ┌─────────────┐                     │
│  │  Terminal    │────────▶│  ArchDrop   │                     │
│  │  (User CLI)  │         │   Binary    │                     │
│  └──────────────┘         └──────┬──────┘                     │
│                                   │                             │
│                                   │ Spawns                      │
│                           ┌───────▼────────┐                   │
│                           │  Axum Server   │                   │
│                           │  (HTTP/HTTPS)  │                   │
│                           └───────┬────────┘                   │
│                                   │                             │
└───────────────────────────────────┼─────────────────────────────┘
                                    │
                                    │ Optional
                     ┌──────────────▼──────────────┐
                     │  Cloudflare Tunnel Process  │
                     │      (cloudflared)          │
                     └──────────────┬──────────────┘
                                    │
════════════════════════════════════╪════════════════════════════
                               Internet/LAN
════════════════════════════════════╪════════════════════════════
                                    │
                            ┌───────▼────────┐
                            │   Web Browser  │
                            │  (JavaScript)  │
                            │                │
                            │ - Web Crypto   │
                            │ - File API     │
                            │ - Fetch API    │
                            └────────────────┘
```

---

## Design Philosophy

### 1. Security by Design

**Principle**: Security is not bolted on; it's fundamental to the architecture.

**Implementation**:
- Encryption keys generated on-device, never transmitted to server
- Keys embedded in URL fragment (`#key=...`) which browsers don't send to servers
- Single-use session tokens prevent replay attacks
- Path traversal validation prevents directory escape

**Trade-offs**:
- ✅ Maximum security and privacy
- ⚠️ Cannot recover lost URLs (no server-side key storage)
- ⚠️ Requires modern browser with Web Crypto API

### 2. Progressive Enhancement

**Principle**: Start with best case (local network), degrade gracefully to tunneled.

**Implementation**:
```
Best Case:     Local HTTPS (--local flag)
  ↓ Advantages: Fast, low latency, no external dependencies
  ↓ Requires:   Devices on same network, cert acceptance

Fallback:      Cloudflare Tunnel (default)
  ↓ Advantages: Works anywhere, NAT traversal
  ↓ Requires:   Internet connection, cloudflared binary
```

**Trade-offs**:
- ✅ Flexibility for different network scenarios
- ✅ User can choose speed vs accessibility
- ⚠️ Tunnel mode requires external dependency

### 3. Fail-Safe Operations

**Principle**: Failures should be safe and obvious, not silent.

**Implementation**:
- RAII pattern: `ChunkStorage` automatically deletes partial files on error
- Atomic session claiming: Only one client can claim a session
- Explicit error propagation: Rust's `Result` type forces error handling
- Health checks: Server validates readiness before displaying URL

**Trade-offs**:
- ✅ No corrupted partial files left on disk
- ✅ Clear error messages to users
- ⚠️ No automatic retry logic (user must restart)

### 4. Minimize Dependencies

**Principle**: Fewer dependencies mean smaller attack surface and faster builds.

**Implementation**:
- Client is pure HTML/JS (no framework)
- Server uses Axum (minimal web framework)
- Crypto uses standard libraries (`aes-gcm`, not custom crypto)
- Optional: Cloudflare tunnel is external process, not library

**Trade-offs**:
- ✅ Fast compile times
- ✅ Smaller binary size
- ✅ Easier to audit
- ⚠️ Client code is more verbose without framework

---

## Architecture Patterns

### 1. Zero-Knowledge Pattern

**Problem**: How to enable encrypted transfers without trusting the server?

**Solution**: Split secret into two parts transmitted via different channels:

```
URL Structure:
https://example.com/send/TOKEN#key=KEY&nonce=NONCE
                         │             │
                         └─────────────┴──────────────┐
                                                       │
Server sees:             TOKEN                        │
Browser sees:            TOKEN + KEY + NONCE ─────────┘
```

**Flow**:
1. Server generates encryption key and session token
2. Server constructs URL with key in fragment
3. User shares URL (QR code, copy-paste)
4. Browser extracts key from `window.location.hash`
5. Browser encrypts/decrypts data with key
6. Server only sees encrypted bytes

**Why This Works**:
- HTTP specification: Fragment is never sent to server
- Browser: Fragment is accessible to JavaScript
- Result: End-to-end encryption maintained

### 2. Single-Use Session Pattern

**Problem**: How to prevent unauthorized access without user accounts?

**Solution**: Atomic compare-and-swap on session activation:

```rust
pub fn claim(&self, token: &str) -> bool {
    self.active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}
```

**Flow**:
1. Session starts inactive (`active = false`)
2. First client calls `claim(token)`
3. Compare-exchange succeeds: `false → true`
4. Second client calls `claim(token)`
5. Compare-exchange fails: already `true`

**Properties**:
- Lock-free: No mutex contention
- Atomic: Race conditions impossible
- Single-use: Only first caller succeeds

### 3. Chunked Streaming Pattern

**Problem**: How to transfer large files without loading into memory?

**Solution**: Split files into fixed-size chunks, encrypt and transfer independently:

```
File (1GB)
    │
    ├─ Chunk 0 (1MB) ──▶ Encrypt ──▶ Transfer ──▶ Decrypt ──▶ Write to position 0
    ├─ Chunk 1 (1MB) ──▶ Encrypt ──▶ Transfer ──▶ Decrypt ──▶ Write to position 1MB
    ├─ Chunk 2 (1MB) ──▶ Encrypt ──▶ Transfer ──▶ Decrypt ──▶ Write to position 2MB
    │       ...
    └─ Chunk 999 (1MB) ─▶ Encrypt ──▶ Transfer ──▶ Decrypt ──▶ Write to position 999MB
```

**Benefits**:
- Memory usage: O(CHUNK_SIZE) not O(FILE_SIZE)
- Parallelism: Multiple chunks transfer simultaneously
- Resumability: Can identify missing chunks (not yet implemented)
- Progress tracking: Count chunks transferred

**Chunk Size Selection** (1MB):
- Smaller: More parallelism, more HTTP overhead
- Larger: Less overhead, higher memory usage
- 1MB: Good balance for most networks

### 4. RAII Cleanup Pattern

**Problem**: How to ensure cleanup happens even on errors/panics?

**Solution**: Use Rust's `Drop` trait for automatic cleanup:

```rust
pub struct ChunkStorage {
    file: File,
    path: PathBuf,
    disarmed: bool,
}

impl Drop for ChunkStorage {
    fn drop(&mut self) {
        if !self.disarmed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
```

**Flow**:
```
Success Path:
1. Create ChunkStorage
2. Write chunks
3. Call finalize() → sets disarmed = true
4. Drop called → no deletion

Error Path:
1. Create ChunkStorage
2. Write chunks
3. Error occurs, panic, or early return
4. Drop called → disarmed still false → file deleted
```

**Why This Matters**:
- No orphaned partial files
- Works even with panics
- Compiler enforces cleanup

### 5. Actor-Based Concurrency Pattern

**Problem**: How to handle concurrent file uploads safely?

**Solution**: Each file gets isolated state, stored in concurrent map:

```rust
pub struct AppState {
    receive_sessions: Arc<DashMap<String, ReceiveSession>>,
}

pub struct ReceiveSession {
    storage: ChunkStorage,
    total_chunks: usize,
    nonce: String,
    // ...
}
```

**Flow**:
```
File A                              File B
  │                                   │
  ├─ Chunk 0 ──┐                     ├─ Chunk 0 ──┐
  ├─ Chunk 1 ──┼─▶ Session A         ├─ Chunk 1 ──┼─▶ Session B
  ├─ Chunk 2 ──┘   (isolated)        ├─ Chunk 2 ──┘   (isolated)
  │                                   │
  No interaction between sessions
```

**Benefits**:
- Files don't interfere with each other
- Lock-free concurrent access via `DashMap`
- Each file can finalize independently

---

## Component Architecture

### High-Level Component Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                          CLI Layer                              │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  main.rs: Command parsing, file validation, mode routing │  │
│  └───────────────────────┬──────────────────────────────────┘  │
└──────────────────────────┼─────────────────────────────────────┘
                           │
┌──────────────────────────▼─────────────────────────────────────┐
│                       Server Layer                              │
│  ┌────────────┬────────────┬────────────┬────────────────┐    │
│  │ Session    │  State     │  Modes     │  Web Templates │    │
│  │ Management │ Management │ (Local/    │  (HTML/JS)     │    │
│  │            │            │  Tunnel)   │                │    │
│  └────────────┴────────────┴────────────┴────────────────┘    │
└──────────────────────────┬─────────────────────────────────────┘
                           │
┌──────────────────────────▼─────────────────────────────────────┐
│                      Transfer Layer                             │
│  ┌────────────┬────────────┬────────────┬────────────────┐    │
│  │  Manifest  │    Send    │  Receive   │    Storage     │    │
│  │ (Metadata) │ (Stream)   │  (Upload)  │ (Chunk Mgmt)   │    │
│  └────────────┴────────────┴────────────┴────────────────┘    │
└──────────────────────────┬─────────────────────────────────────┘
                           │
┌──────────────────────────▼─────────────────────────────────────┐
│                       Crypto Layer                              │
│  ┌────────────────────────────────────────────────────────┐    │
│  │  AES-256-GCM Encryption/Decryption                     │    │
│  │  Types: EncryptionKey, Nonce                           │    │
│  └────────────────────────────────────────────────────────┘    │
└──────────────────────────┬─────────────────────────────────────┘
                           │
┌──────────────────────────▼─────────────────────────────────────┐
│                      Support Layers                             │
│  ┌────────────┬────────────┬────────────┐                      │
│  │     UI     │   Tunnel   │   Utils    │                      │
│  │ (TUI, QR)  │(Cloudflare)│ (Helpers)  │                      │
│  └────────────┴────────────┴────────────┘                      │
└─────────────────────────────────────────────────────────────────┘
```

### Component Responsibilities

#### CLI Layer (`main.rs`)
- **Responsibility**: User interface, argument parsing, input validation
- **Inputs**: Command-line arguments
- **Outputs**: Server configuration, file lists
- **Dependencies**: Clap (CLI parsing), WalkDir (directory traversal)

#### Server Layer (`server/`)

**Session Module** (`session.rs`):
- **Responsibility**: Session lifecycle, token management, atomic claiming
- **State**: Token, active flag, completion flag, encryption key/cipher
- **Concurrency**: Thread-safe via `Arc<AtomicBool>`

**State Module** (`state.rs`):
- **Responsibility**: Shared application state for Axum handlers
- **State**: Session, progress channel, receive sessions map
- **Concurrency**: `Clone` for sharing across handlers, `DashMap` for concurrent file uploads

**Modes Module** (`modes.rs`):
- **Responsibility**: Server initialization (local HTTPS vs tunnel)
- **Outputs**: Running server, QR code displayed
- **Control Flow**: Spawns server on background task, blocks on TUI

**Web Module** (`web.rs`):
- **Responsibility**: Serve HTML/JS templates to browser
- **Templates**: download.html, upload.html, shared.js, styles.css

#### Transfer Layer (`transfer/`)

**Manifest Module** (`manifest.rs`):
- **Responsibility**: File metadata collection
- **Data**: File paths, sizes, per-file nonces
- **Output**: JSON-serializable manifest sent to browser

**Send Module** (`send.rs`):
- **Responsibility**: Stream encrypted chunks to browser
- **Endpoints**:
  - `GET /send/:token/manifest` - File list
  - `GET /send/:token/:file/:chunk` - Encrypted chunk
  - `POST /send/:token/complete` - Mark transfer complete

**Receive Module** (`receive.rs`):
- **Responsibility**: Accept encrypted chunks from browser
- **Endpoints**:
  - `POST /receive/:token/chunk` - Upload chunk
  - `POST /receive/:token/finalize` - Finalize file
  - `POST /receive/:token/complete` - Mark transfer complete

**Storage Module** (`storage.rs`):
- **Responsibility**: Chunk decryption, disk writing, cleanup
- **Features**: Out-of-order writes, RAII cleanup, SHA-256 hashing

#### Crypto Layer (`crypto.rs`, `types.rs`)

**Types**:
- `EncryptionKey`: 32-byte AES-256 key
- `Nonce`: 7-byte nonce base (combined with counter)

**Functions**:
- `encrypt_chunk_at_position()`: Encrypts chunk with position-based nonce
- `decrypt_chunk_at_position()`: Decrypts chunk with position-based nonce

#### Support Layers

**UI Module** (`ui/`):
- TUI: Terminal interface with progress bar, QR code
- QR: QR code generation for URL sharing
- Output: Spinner, success/error messages

**Tunnel Module** (`tunnel.rs`):
- Spawn `cloudflared` process
- Poll metrics API for tunnel URL
- RAII cleanup on drop

---

## Network Architecture

### Local Mode (--local)

```
┌─────────────────┐                    ┌─────────────────┐
│   User Device   │                    │   Other Device  │
│                 │                    │                 │
│  ┌───────────┐  │  Same Network     │  ┌───────────┐  │
│  │ ArchDrop  │  │◀────────────────▶│  │  Browser  │  │
│  │  Server   │  │                    │  │           │  │
│  └───────────┘  │                    │  └───────────┘  │
│   127.0.0.1:X   │                    │                 │
│   (Self-signed  │                    │                 │
│    HTTPS cert)  │                    │                 │
└─────────────────┘                    └─────────────────┘

Protocol: HTTPS (self-signed certificate)
Port: Random available port (OS-assigned)
Discovery: QR code or manual URL sharing
Latency: <1ms (LAN)
Bandwidth: Full LAN speed (typically 100Mbps-10Gbps)
```

**Advantages**:
- Minimal latency
- Full LAN bandwidth
- No external dependencies
- Complete privacy (traffic never leaves network)

**Disadvantages**:
- Devices must be on same network
- Browser shows certificate warning (must accept)
- Not accessible from internet

### Tunnel Mode (default)

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│   User Device   │     │  Cloudflare CDN  │     │   Other Device  │
│                 │     │                  │     │                 │
│  ┌───────────┐  │     │  ┌────────────┐ │     │  ┌───────────┐  │
│  │ ArchDrop  │◀─┼─────┼─▶│   Tunnel   │◀┼─────┼─▶│  Browser  │  │
│  │  Server   │  │     │  │   Edge     │ │     │  │           │  │
│  └───────────┘  │     │  └────────────┘ │     │  └───────────┘  │
│  localhost:X    │     │                  │     │                 │
│       ▲         │     │  Public HTTPS    │     │                 │
│       │         │     │  with valid cert │     │                 │
│  ┌────┴──────┐  │     │                  │     │                 │
│  │cloudflared│  │     │                  │     │                 │
│  └───────────┘  │     │                  │     │                 │
└─────────────────┘     └──────────────────┘     └─────────────────┘

Protocol: HTTP (local), HTTPS (tunnel)
Port: Local random port + Cloudflare edge
Discovery: QR code with tunnel URL
Latency: ~50-200ms (depends on CF edge location)
Bandwidth: Internet connection limited
```

**Advantages**:
- Works behind NAT/firewalls
- No port forwarding needed
- Valid HTTPS certificate (no warnings)
- Accessible from anywhere

**Disadvantages**:
- Requires `cloudflared` installation
- Added latency (~100-200ms)
- Bandwidth limited by internet connection
- Depends on Cloudflare service

### Protocol Selection Decision Tree

```
Start
  │
  ├─ Same network? ────Yes──▶ Use --local flag ──▶ Fast transfer
  │                                │
  │                                └─ Must accept cert warning
  │
  └─ Different networks? ───Yes──▶ Use tunnel mode ──▶ Accessible
                                   │
                                   └─ Requires cloudflared
```

---

## Data Flow

### Send Mode - Detailed Flow

```
1. CLI Invocation
   ┌────────────────────────────────────────┐
   │ $ archdrop send file.txt --local       │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
2. File Discovery & Validation
   ┌────────────────────────────────────────┐
   │ - Check file exists                    │
   │ - Traverse directories recursively     │
   │ - Collect file metadata (size, path)  │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
3. Manifest Creation
   ┌────────────────────────────────────────┐
   │ - Generate per-file nonces             │
   │ - Create FileEntry for each file       │
   │ - Calculate relative paths             │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
4. Session Initialization
   ┌────────────────────────────────────────┐
   │ - Generate session key (32 bytes)     │
   │ - Generate session nonce (7 bytes)    │
   │ - Generate session token (UUID v4)    │
   │ - Create AES-256-GCM cipher           │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
5. Server Startup
   ┌────────────────────────────────────────┐
   │ - Bind to random port (0.0.0.0:0)     │
   │ - Configure Axum routes:              │
   │   * /health                            │
   │   * /send/:token/manifest             │
   │   * /send/:token/:file/chunk/:chunk   │
   │   * /send/:token/complete             │
   │ - Start HTTPS server (self-signed)    │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
6. Optional Tunnel Setup
   ┌────────────────────────────────────────┐
   │ - Spawn cloudflared process            │
   │ - Wait for tunnel URL from metrics     │
   │ - Construct public URL                 │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
7. URL Construction & Display
   ┌────────────────────────────────────────┐
   │ URL: https://....#key=X&nonce=Y        │
   │ - Display QR code in TUI               │
   │ - Show file name and progress bar      │
   └─────────────────┬──────────────────────┘
                     │
         ┌───────────┴───────────┐
         │   User scans QR code  │
         └───────────┬───────────┘
                     │
                     ▼
8. Browser Requests Manifest
   ┌────────────────────────────────────────┐
   │ GET /send/:token/manifest              │
   │ - Server validates token               │
   │ - Returns JSON manifest                │
   │   { files: [                           │
   │     { index, name, size, nonce }       │
   │   ]}                                   │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
9. Browser Initiates Parallel Downloads
   ┌────────────────────────────────────────┐
   │ For each file:                         │
   │   Calculate chunk count                │
   │   Request chunks in parallel           │
   │                                        │
   │ GET /send/:token/0/chunk/0             │
   │ GET /send/:token/0/chunk/1             │
   │ GET /send/:token/0/chunk/2             │
   │        ... (concurrent) ...            │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
10. Server Processes Chunk Request
    ┌────────────────────────────────────────┐
    │ - Claim session (first chunk only)     │
    │ - Validate file_index and chunk_index  │
    │ - Calculate byte range:                │
    │   start = chunk_index * 1MB            │
    │   end = min(start + 1MB, file_size)    │
    │ - Open file with buffered reader       │
    │ - Seek to start position               │
    │ - Read chunk into buffer               │
    │ - Encrypt with AES-GCM:                │
    │   nonce = file_nonce + counter         │
    │ - Return encrypted bytes               │
    └─────────────────┬──────────────────────┘
                      │
                      ▼
11. Browser Decrypts & Assembles
    ┌────────────────────────────────────────┐
    │ For each chunk received:               │
    │ - Extract keys from URL fragment       │
    │ - Decrypt with Web Crypto API          │
    │ - Append to file blob                  │
    │ - Update progress bar                  │
    │                                        │
    │ When all chunks received:              │
    │ - Trigger browser download             │
    │ - POST /send/:token/complete           │
    └─────────────────┬──────────────────────┘
                      │
                      ▼
12. Transfer Complete
    ┌────────────────────────────────────────┐
    │ - Server marks session complete        │
    │ - Update TUI to 100%                   │
    │ - TUI exits                            │
    │ - Server shuts down gracefully         │
    └────────────────────────────────────────┘
```

### Receive Mode - Detailed Flow

```
1. CLI Invocation
   ┌────────────────────────────────────────┐
   │ $ archdrop receive ~/Downloads --local │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
2. Destination Validation
   ┌────────────────────────────────────────┐
   │ - Check if path exists                 │
   │ - Create directory if needed           │
   │ - Verify write permissions             │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
3. Session Initialization
   ┌────────────────────────────────────────┐
   │ - Generate session key (32 bytes)     │
   │ - Generate session nonce (7 bytes)    │
   │ - Generate session token (UUID v4)    │
   │ - Create AES-256-GCM cipher           │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
4. Server Startup
   ┌────────────────────────────────────────┐
   │ - Bind to random port (0.0.0.0:0)     │
   │ - Configure Axum routes:              │
   │   * /health                            │
   │   * /receive/:token/chunk             │
   │   * /receive/:token/finalize          │
   │   * /receive/:token/complete          │
   │ - Start HTTPS server (self-signed)    │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
5. URL Construction & Display
   ┌────────────────────────────────────────┐
   │ URL: https://....#key=X&nonce=Y        │
   │ - Display QR code in TUI               │
   │ - Show destination path                │
   └─────────────────┬──────────────────────┘
                     │
         ┌───────────┴───────────┐
         │   User scans QR code  │
         │   Selects files       │
         └───────────┬───────────┘
                     │
                     ▼
6. Browser Encrypts & Uploads
   ┌────────────────────────────────────────┐
   │ For each file:                         │
   │ - Generate per-file nonce              │
   │ - Split into chunks                    │
   │ - Encrypt each chunk with Web Crypto   │
   │ - Upload chunks in parallel            │
   │                                        │
   │ POST /receive/:token/chunk             │
   │ Multipart form data:                   │
   │ - chunk: encrypted bytes               │
   │ - fileName: "file.txt"                 │
   │ - relativePath: "dir/file.txt"         │
   │ - chunkIndex: 0                        │
   │ - totalChunks: 100                     │
   │ - fileSize: 104857600                  │
   │ - nonce: "base64..."                   │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
7. Server Receives & Decrypts Chunk
   ┌────────────────────────────────────────┐
   │ - Validate session token               │
   │ - Parse multipart form data            │
   │ - Hash relativePath for file ID        │
   │ - Get or create ReceiveSession         │
   │ - Check for duplicate chunk            │
   │ - Decrypt chunk with AES-GCM           │
   │ - Seek to position in file             │
   │ - Write decrypted data                 │
   │ - Track chunk as received              │
   │ - Return success with progress         │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
8. Browser Finalizes File
   ┌────────────────────────────────────────┐
   │ POST /receive/:token/finalize          │
   │ Body: { relativePath: "..." }          │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
9. Server Finalizes File
   ┌────────────────────────────────────────┐
   │ - Verify all chunks received           │
   │ - Flush file to disk                   │
   │ - Compute SHA-256 hash                 │
   │ - Disarm RAII guard (prevent deletion) │
   │ - Remove from sessions map             │
   │ - Return hash to browser               │
   └─────────────────┬──────────────────────┘
                     │
                     ▼
10. All Files Complete
    ┌────────────────────────────────────────┐
    │ POST /receive/:token/complete          │
    │ - Server marks session complete        │
    │ - Update TUI to 100%                   │
    │ - TUI exits                            │
    │ - Server shuts down gracefully         │
    └────────────────────────────────────────┘
```

---

## Security Architecture

See [SECURITY.md](SECURITY.md) for detailed security analysis.

### Defense in Depth

```
┌─────────────────────────────────────────────────────────────────┐
│                       Layer 1: Network                          │
│  - Optional HTTPS (local mode)                                  │
│  - Cloudflare tunnel encryption (tunnel mode)                   │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────┐
│                    Layer 2: Authentication                      │
│  - Single-use session tokens (UUID v4)                          │
│  - Atomic session claiming                                      │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────┐
│                    Layer 3: Encryption                          │
│  - AES-256-GCM authenticated encryption                         │
│  - Unique nonces per chunk                                      │
│  - Zero-knowledge key distribution                              │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────┐
│                  Layer 4: Input Validation                      │
│  - Path traversal prevention                                    │
│  - File index bounds checking                                   │
│  - Multipart form validation                                    │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────┐
│                    Layer 5: Integrity                           │
│  - Per-chunk authentication tags (GCM)                          │
│  - Final file SHA-256 hashing                                   │
│  - Duplicate chunk detection                                    │
└─────────────────────────────────────────────────────────────────┘
```

---

## Performance Architecture

### Concurrency Model

**Tokio Green Threads**:
- Async runtime with work-stealing scheduler
- Thousands of concurrent connections without OS threads
- Non-blocking I/O operations

**Parallelism Opportunities**:

1. **Send Mode**: Browser fetches multiple chunks concurrently
   ```
   Chunk 0 ──┐
   Chunk 1 ──┼──▶ Server handles concurrently (Tokio tasks)
   Chunk 2 ──┘
   ```

2. **Receive Mode**: Browser uploads multiple files concurrently
   ```
   File A ──┐
   File B ──┼──▶ DashMap allows lock-free concurrent access
   File C ──┘
   ```

### Memory Management

**Streaming Architecture**:
- Files never fully loaded into memory
- Buffered I/O with 2MB read buffers
- Each chunk independently encrypted/decrypted

**Memory Profile**:
```
Send Mode:
  - File handle: O(1)
  - Read buffer: 2MB
  - Encryption buffer: 1MB
  - Network buffer: ~1MB
  Total per request: ~4MB

Receive Mode:
  - Per file storage: O(chunk_set) = O(total_chunks * 8 bytes)
  - Write buffer: 1MB per concurrent chunk
  - File handle: O(concurrent_files)
  Total: ~(concurrent_chunks * 1MB) + metadata
```

### Disk I/O Optimization

**Sequential Writes (Send)**:
- Buffered reads with 2MB buffer
- OS page cache benefits sequential access
- Minimal seeks if chunks requested in order

**Random Writes (Receive)**:
- Chunks may arrive out of order
- Use `seek()` to write at correct position
- Modern SSDs handle random writes well
- HDDs may show degraded performance

### Network Optimization

**HTTP/2 Benefits** (when using tunnel):
- Multiplexing: Multiple chunks over single connection
- Header compression: Smaller overhead
- Server push: Not currently used (potential future optimization)

**Chunk Size Impact**:
```
Small chunks (64KB):
  ✅ Fine-grained parallelism
  ✅ Lower memory usage
  ⚠️ More HTTP overhead
  ⚠️ More encryption overhead

Large chunks (10MB):
  ✅ Less HTTP overhead
  ✅ Less encryption overhead
  ⚠️ Higher memory usage
  ⚠️ Coarse-grained parallelism

Chosen (1MB):
  ✓ Good balance for most networks
  ✓ Reasonable memory usage
  ✓ Sufficient parallelism
```

---

## Deployment Architecture

### Single Binary Deployment

**Build**:
```bash
cargo build --release
```

**Binary Size**:
- ~10-15MB (with dependencies compiled in)
- Static linking of Rust dependencies
- Dynamic linking of system libraries (libc, libssl)

**Deployment**:
```bash
sudo cp target/release/archdrop /usr/local/bin/
```

**No Configuration Files**:
- All configuration via CLI flags
- No persistent state
- Ephemeral sessions (process lifetime only)

### System Requirements

**Minimum**:
- Linux kernel 3.2+ (Rust std library requirement)
- 50MB disk space
- 100MB RAM
- Network interface

**Recommended**:
- Linux kernel 4.0+
- 100MB disk space
- 512MB RAM
- Gigabit network (for local mode)

**Optional**:
- `cloudflared` binary (for tunnel mode)
- Valid SSL certificate (for production local mode)

### Scaling Characteristics

**Vertical Scaling**:
- CPU: Encryption is CPU-bound
  - Single file: ~1 core
  - Multiple files: Up to N cores
- Memory: O(concurrent_chunks * 1MB)
- Disk: I/O bound for large files

**Horizontal Scaling**:
- Not applicable (single ephemeral session per process)
- Each transfer is independent
- No shared state between transfers

**Limitations**:
- One transfer per process instance
- No load balancing (single-use URLs)
- No session persistence

---

## Future Architecture Considerations

### Potential Enhancements

1. **Resumable Transfers**
   - Track completed chunks in persistent storage
   - Allow reconnection with same token
   - Browser can query missing chunks

2. **Multi-Recipient Broadcasting**
   - Allow multiple clients to claim session
   - Broadcast chunks to all connected clients
   - Useful for team file sharing

3. **Compression Integration**
   - Add zstd compression before encryption
   - Trade-off: CPU for bandwidth
   - Optional flag: `--compress`

4. **Progress Persistence**
   - SQLite database for chunk tracking
   - Survive process restarts
   - Query API for transfer status

5. **Web Assembly Crypto**
   - Move encryption to WebAssembly for performance
   - Faster than JavaScript Web Crypto API
   - Better control over memory layout

### Architectural Debt

1. **No Graceful Degradation**
   - Transfer fails completely on single chunk error
   - Could implement automatic retry logic
   - Could support partial file recovery

2. **Limited Error Reporting**
   - Browser doesn't know specific failure reasons
   - Generic 500 errors returned
   - Could add detailed error codes

3. **No Metrics/Observability**
   - No logging of transfer statistics
   - No Prometheus metrics
   - Useful for debugging performance issues

4. **Synchronous Cleanup**
   - `Drop` trait uses blocking I/O
   - Could use async cleanup task
   - Low priority (only affects error paths)

---

## Conclusion

ArchDrop's architecture prioritizes:

1. **Security**: Zero-knowledge design, end-to-end encryption
2. **Simplicity**: Single binary, minimal dependencies
3. **Performance**: Streaming, parallelism, async I/O
4. **Reliability**: RAII cleanup, atomic operations, error propagation

The architecture makes deliberate trade-offs:
- Ephemeral sessions (simplicity) vs. resumable transfers (complexity)
- Single-use tokens (security) vs. multi-recipient (convenience)
- Static binary (portability) vs. plugins (extensibility)

These decisions reflect the project's goal: a **secure, simple, fast** file transfer tool for personal use, not an enterprise-grade file sharing platform.
