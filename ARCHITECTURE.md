# ArchDrop Architecture Documentation

## Overview

ArchDrop is a secure peer-to-peer file transfer CLI tool for Linux that enables direct file transfer between devices without cloud intermediaries. The application implements end-to-end encryption using AES-256-GCM, ensuring files remain private during transit.

**Core Statistics:**
- 1,730 lines of Rust
- 505 lines of JavaScript
- Zero-knowledge architecture (server never sees unencrypted data)

## High-Level Architecture

```
┌─────────────┐                    ┌──────────────┐
│   Sender    │                    │   Receiver   │
│   Device    │                    │    Device    │
└──────┬──────┘                    └──────┬───────┘
       │                                  │
       │ 1. archdrop send file.txt        │
       │                                  │
       ▼                                  │
┌─────────────────────────────────┐      │
│  Local HTTPS / Tunnel Server    │      │
│  - Generates key & nonce        │      │
│  - Creates session token        │      │
│  - Displays QR code with URL    │      │
└─────────────────────────────────┘      │
       │                                  │
       │ 2. URL in QR:                    │
       │    https://host/send/token       │
       │    #key=xxx&nonce=yyy            │
       │                                  │
       │                                  │
       │  3. User scans QR ───────────────┤
       │                                  │
       │  4. Browser fetches manifest     │
       │◄─────────────────────────────────┤
       │                                  │
       │  5. Streams encrypted chunks     │
       ├──────────────────────────────────►
       │                                  │
       │  6. Browser decrypts in-memory   │
       │                                  ▼
       │                          ┌──────────────┐
       │                          │ Decrypted    │
       │                          │ File Saved   │
       │                          └──────────────┘
```

## Directory Structure

```
archdrop-beta/
├── src/
│   ├── main.rs              # CLI entry point, argument parsing
│   ├── lib.rs               # Module exports
│   │
│   ├── crypto/              # Encryption layer
│   │   ├── mod.rs           # Key & Nonce types
│   │   ├── encrypt.rs       # Encryptor creation
│   │   ├── decrypt.rs       # Chunk decryption
│   │   └── stream.rs        # Streaming encrypted file reader
│   │
│   ├── server/              # HTTP server infrastructure
│   │   ├── mod.rs           # Server startup logic
│   │   ├── session.rs       # Session management
│   │   ├── state.rs         # Application state
│   │   ├── modes.rs         # Local HTTPS vs Tunnel
│   │   ├── utils.rs         # TLS cert generation
│   │   └── web.rs           # HTML/JS template serving
│   │
│   ├── transfer/            # File transfer logic
│   │   ├── manifest.rs      # File metadata structure
│   │   ├── chunk.rs         # Chunk upload/metadata
│   │   ├── send.rs          # Streaming file sender
│   │   ├── receive.rs       # Chunk receiver
│   │   └── util.rs          # Error handling, hashing
│   │
│   ├── ui/                  # User interface
│   │   ├── qr.rs            # QR code generation
│   │   ├── tui.rs           # Terminal UI (ratatui)
│   │   └── output.rs        # Console styling
│   │
│   └── tunnel.rs            # Cloudflare tunnel integration
│
└── templates/               # Web UI (embedded in binary)
    ├── upload/              # File upload page
    ├── download/            # File download page
    └── shared/crypto.js     # Browser-side crypto
```

## Core Components

### 1. CLI Layer (main.rs)

**Responsibilities:**
- Parse command-line arguments using clap
- Validate file paths and permissions
- Build file manifest for send mode
- Initialize server with appropriate mode

**Commands:**
```bash
archdrop send <paths...> [--local]
archdrop receive [destination] [--local]
```

**Flags:**
- `--local`: Use self-signed HTTPS (faster, no tunnel)
- Default: Use Cloudflare tunnel (works across networks)

### 2. Cryptography Layer (crypto/)

**Algorithm:** AES-256-GCM with streaming support

**Key Components:**

**EncryptionKey (32 bytes):**
```rust
pub struct EncryptionKey([u8; 32]);

impl EncryptionKey {
    pub fn new() -> Self {
        // Generates cryptographically secure random key
        OsRng.fill_bytes(&mut key)
    }
}
```

**Nonce Structure (12 bytes):**
```
┌─────────────┬───────────────┬────────────┐
│  7 bytes    │   4 bytes     │  1 byte    │
│  Random     │   Counter     │ Last-chunk │
│  Base       │   (BE32)      │   Flag     │
└─────────────┴───────────────┴────────────┘
```

The counter-based nonce allows:
- Stateless decryption of individual chunks
- Deterministic nonce reconstruction at any position
- Support for random-access chunk downloads

**Streaming Encryption (EncryptorBE32):**
```rust
// Server-side: encrypt and stream
let encryptor = EncryptorBE32::<Aes256Gcm>::new(...);
encryptor.encrypt_next(chunk) // Auto-increments counter
```

**Stateless Decryption:**
```rust
// Decrypt chunk N without decrypting chunks 0..N-1
decrypt_chunk_at_position(key, nonce_base, data, counter: u32)
```

### 3. Server Infrastructure (server/)

**Two Operating Modes:**

**Local HTTPS Mode:**
- Generates self-signed certificate using rcgen
- Binds to local IP on random port
- URL format: `https://192.168.x.x:port/send/token#key=...`
- Faster (no tunnel overhead)
- Requires manual certificate acceptance

**Tunnel Mode:**
- Spawns cloudflared subprocess
- Proxies to local HTTP server
- URL format: `https://random.trycloudflare.com/send/token#key=...`
- Works across networks/firewalls
- Slightly slower due to tunnel latency

**Session Management:**

```rust
pub struct Session {
    token: String,              // UUID v4
    manifest: Option<Manifest>, // Send mode: file list
    destination: Option<PathBuf>, // Receive mode: save path
    session_key: String,        // Base64-encoded AES key
    used: Arc<Mutex<bool>>,    // Single-use flag
}
```

**Session Lifecycle:**
1. Created on server startup
2. Token embedded in QR code URL
3. Validated on every HTTP request
4. Marked as used after successful transfer
5. Server terminates after completion

**Routes:**

Send Mode:
```
GET  /send/{token}/manifest        # File list with metadata
GET  /send/{token}/{index}/data    # Stream encrypted file
GET  /send/{token}                 # Serve download HTML
```

Receive Mode:
```
POST /receive/{token}/chunk        # Upload encrypted chunk
POST /receive/{token}/status       # Check completed chunks
POST /receive/{token}/finalize     # Decrypt and save files
GET  /receive/{token}              # Serve upload HTML
```

### 4. Transfer Layer (transfer/)

**Send Flow:**

1. **Manifest Creation** (manifest.rs)
   - Walks directory tree
   - Generates per-file nonce
   - Stores original paths and sizes

2. **Streaming Transfer** (send.rs)
   - Opens file asynchronously
   - Reads 64KB chunks
   - Encrypts via EncryptorBE32
   - Frames encrypted data: `[4-byte length][encrypted data]`
   - Streams via Axum body stream

3. **Browser Processing** (download.js)
   - Parses framed chunks
   - Decrypts using Web Crypto API
   - Concatenates decrypted chunks
   - Triggers download via Blob URL

**Receive Flow:**

1. **Browser Upload** (upload.js)
   - Chunks file into 256KB pieces
   - Generates per-file nonce
   - Encrypts each chunk with incremented counter
   - Uploads as multipart/form-data

2. **Server Storage** (receive.rs, chunk.rs)
   - Stores encrypted chunks in `/tmp/archdrop/{token}/{file_hash}/`
   - Tracks metadata: completed chunks, total size, nonce
   - No decryption until finalization

3. **Finalization**
   - Verifies all chunks received
   - Validates path (prevents traversal)
   - Decrypts chunks sequentially
   - Writes to destination
   - Cleans up temp files

**Path Security:**
```rust
// Prevent path traversal attacks
let canonical_dest = dest_path.canonicalize()?;
let canonical_base = destination.canonicalize()?;

if !canonical_dest.starts_with(&canonical_base) {
    return Err("Path traversal detected");
}
```

### 5. User Interface (ui/)

**Terminal UI (tui.rs):**
- Built with ratatui (terminal UI framework)
- Displays QR code in terminal
- Shows transfer progress
- Real-time progress updates via tokio::sync::watch channel

**QR Code (qr.rs):**
```rust
QrCode::new(url.as_bytes())
    .render::<unicode::Dense1x2>()
    .dark_color(Dense1x2::Light)  // Inverted for terminals
    .light_color(Dense1x2::Dark)
```

### 6. Browser-Side Crypto (templates/shared/crypto.js)

**URL Fragment Parsing:**
```javascript
const fragment = window.location.hash.substring(1)
const params = new URLSearchParams(fragment)
const key = params.get('key')    // Never sent to server
const nonce = params.get('nonce')
```

**Web Crypto API:**
```javascript
const cryptoKey = await crypto.subtle.importKey(
    'raw', keyData, { name: 'AES-GCM' }, false, ['decrypt']
)

const decrypted = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv: nonce },
    cryptoKey,
    encryptedChunk
)
```

## Data Flow

### Send Mode Data Flow

```
1. CLI: archdrop send file.txt
   └─► Manifest creation (paths, sizes, per-file nonces)

2. Server startup
   ├─► Generate session key (32 bytes random)
   ├─► Generate session token (UUID v4)
   └─► Display QR: https://host/send/{token}#key={key}&nonce={base_nonce}

3. Browser opens URL
   ├─► Extract key/nonce from fragment
   ├─► GET /send/{token}/manifest
   └─► For each file:
       ├─► GET /send/{token}/{index}/data
       │   └─► Server streams: [frame][frame][frame]...
       ├─► Browser parses frames
       ├─► Decrypts each chunk
       └─► Triggers download

4. Server shutdown after transfer
```

### Receive Mode Data Flow

```
1. CLI: archdrop receive ./downloads
   └─► Create destination directory

2. Server startup
   ├─► Generate session key
   ├─► Generate session token
   └─► Display QR: https://host/receive/{token}#key={key}

3. Browser opens upload page
   ├─► Extract key from fragment
   ├─► User selects files
   └─► For each file:
       ├─► Generate file nonce
       ├─► Chunk into 256KB pieces
       ├─► Encrypt each chunk
       │   └─► Nonce = [7-byte base][4-byte counter][0]
       ├─► POST /receive/{token}/chunk (for each)
       └─► POST /receive/{token}/finalize

4. Server finalization
   ├─► Verify all chunks received
   ├─► Validate path (no traversal)
   ├─► For each chunk (sequential):
   │   ├─► Read encrypted chunk from /tmp
   │   ├─► Decrypt using counter-based nonce
   │   └─► Append to output file
   ├─► Clean up /tmp/archdrop/{token}
   └─► Mark session as used

5. Server shutdown
```

## Security Model

### Zero-Knowledge Architecture

**Key Principle:** Server never has access to both encrypted data AND decryption keys.

**Separation of Concerns:**
- Encryption keys travel in URL fragment (not sent to server)
- Server handles encrypted data only
- Browser performs all decryption client-side

### Threat Model

**Protected Against:**
- Network eavesdropping (AES-256-GCM encryption)
- Server compromise (zero-knowledge design)
- Path traversal attacks (canonical path validation)
- Concurrent session access (single-use sessions)

**Not Protected Against:**
- Browser history leakage (keys in fragment may persist)
- Token guessing (UUIDs provide ~122 bits entropy)
- Denial of service (no rate limiting)
- Malicious receiver (sender trusts receiver's device)

### Encryption Guarantees

**AES-256-GCM provides:**
- Confidentiality (256-bit key strength)
- Integrity (GMAC authentication tag)
- Authenticated encryption (AEAD)

**Nonce Safety:**
- 7-byte random base = 2^56 possible values
- 4-byte counter = 2^32 chunks per file
- Collision probability negligible for use case

## Performance Characteristics

### Memory Usage

**Send Mode:**
- 64KB buffer per file stream
- Manifest held in memory (minimal for typical use)
- Streaming reduces memory footprint

**Receive Mode:**
- Chunks buffered to disk immediately
- Sequential decryption (no full file in memory)
- Browser chunks at 256KB (configurable)

### Throughput

**Bottlenecks:**
- Encryption/decryption: ~1-2 GB/s (AES-NI)
- Network: Limited by tunnel or local bandwidth
- Disk I/O: Write-heavy in receive mode

**Optimization Opportunities:**
- Parallel chunk processing
- Larger chunk sizes for large files
- Direct memory-to-memory transfer (skip disk)

## Technology Stack

### Backend (Rust)
- **axum 0.7**: Web framework
- **tokio 1**: Async runtime
- **aes-gcm 0.10**: Encryption (stream mode)
- **rcgen 0.12**: Self-signed certificates
- **uuid 1.6**: Session tokens
- **ratatui 0.26**: Terminal UI
- **qrcode 0.13**: QR generation

### Frontend (JavaScript)
- Web Crypto API: Browser-native encryption
- Fetch API: HTTP requests
- Blob/File APIs: File handling

### External Dependencies
- **cloudflared** (optional): Tunnel creation

## Testing

**Test Coverage:**
```
tests/
├── decryption_tests.rs    # Crypto correctness
├── hash_tests.rs          # Path hashing
└── integration_test.rs    # End-to-end flows
```

**Critical Test Scenarios:**
- Nonce counter correctness
- Chunk encryption/decryption round-trip
- Path traversal prevention
- Session validation

## Deployment

**Single Binary:**
- All web assets embedded via include_str!
- No external dependencies (except cloudflared for tunnel)
- Statically linked (portable)

**Runtime Requirements:**
- Linux (tested)
- Network access (local or internet)
- Write access to /tmp

## Known Limitations

1. **Single session per instance**: Server handles one transfer at a time
2. **No resume support**: Failed transfers must restart
3. **Temp directory hardcoded**: Uses /tmp/archdrop
4. **No compression**: Files sent as-is
5. **Browser required**: Receiver needs modern browser with Web Crypto API

## Future Considerations

1. **Multiple sessions**: Concurrent transfers
2. **Session persistence**: Survive server restarts
3. **Compression**: Reduce bandwidth for compressible files
4. **Progress persistence**: Resume interrupted uploads
5. **Alternative auth**: Non-QR methods for desktop-to-desktop
6. **Configurable temp path**: Support different filesystems
7. **Rate limiting**: DoS protection
