# Detailed Analysis: Selected Code Review Issues

## Issue 7: Missing Integrity Verification

### Current Implementation

**Location:** `src/transfer/receive.rs:149-174`

```rust
// Decrypt and Merge chunks into final file
let mut output = tokio::fs::File::create(&dest_path).await?;

let session_key = EncryptionKey::from_base64(&state.session_key)?;
let file_nonce = Nonce::from_base64(&metadata.nonce)?;

// Merge and decrypt chunks sequentially
for i in 0..metadata.total_chunks {
    let chunk_path = format!("{}/{}.chunk", chunk_dir, i);
    let encrypted_chunk = tokio::fs::read(&chunk_path).await?;

    let decrypted = decrypt_chunk_at_position(
        &session_key,
        &file_nonce,
        &encrypted_chunk,
        i as u32,
    )?;

    output.write_all(&decrypted).await?;
}

// Cleanup temp files
tokio::fs::remove_dir_all(&chunk_dir).await.ok();
```

### The Problem

**What happens if:**
1. A chunk file is corrupted on disk (bit flip, partial write)
2. An attacker replaces encrypted chunks with different encrypted data
3. Network transmission corrupted a chunk that passed AES-GCM auth
4. Filesystem returns wrong data (rare but possible)

**Current behavior:** Decryption will likely fail with AES-GCM error, but there's no way to verify the *final assembled file* matches what the sender intended.

### Why AES-GCM Isn't Enough

AES-GCM provides:
- **Per-chunk authentication**: Each chunk's encrypted data has an auth tag
- **Chunk integrity**: You know if a chunk was tampered with

AES-GCM does NOT provide:
- **Cross-chunk integrity**: You can't tell if chunks were reordered
- **File-level integrity**: You can't verify the complete file matches sender's original
- **Metadata verification**: File name, size, chunk count aren't authenticated

### Attack Scenarios

**Scenario 1: Chunk Reordering**
```
Attacker swaps chunk 5 and chunk 10
AES-GCM: ✓ Both chunks decrypt successfully
Result: Corrupted file (wrong data in wrong position)
```

**Scenario 2: Chunk Substitution**
```
Attacker takes chunk 3 from a different file transfer
AES-GCM: ✓ Decrypts successfully (if same key/nonce)
Result: File contains foreign data
```

**Scenario 3: Truncation**
```
Attacker deletes last 10 chunks
Metadata check: ✗ Would fail (good!)
But: If metadata is also modified, attack succeeds
```

### The Fix: File-Level Hashing

**Step 1: Add hash to manifest**

`src/transfer/manifest.rs`:
```rust
use sha2::{Sha256, Digest};

#[derive(Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub full_path: PathBuf,
    pub relative_path: String,
    pub size: u64,
    pub nonce: String,
    pub sha256: String,  // ← Add this
}

impl Manifest {
    pub async fn new(file_paths: Vec<PathBuf>, base: Option<&Path>) -> Result<Self> {
        let mut files = Vec::new();

        for full_path in file_paths {
            // ... existing code ...

            // Calculate hash of original file
            let sha256 = calculate_file_hash(&full_path).await?;

            files.push(FileEntry {
                name,
                full_path: full_path.clone(),
                relative_path,
                size: metadata.len(),
                nonce: Nonce::new().to_base64(),
                sha256,  // ← Store it
            });
        }

        Ok(Manifest { files })
    }
}

async fn calculate_file_hash(path: &Path) -> Result<String> {
    use tokio::io::AsyncReadExt;

    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 { break; }
        hasher.update(&buffer[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}
```

**Step 2: Verify hash after decryption**

`src/transfer/receive.rs`:
```rust
pub async fn finalize_upload(
    Path(token): Path<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<axum::Json<Value>, AppError> {
    // ... existing validation code ...

    // Decrypt and merge chunks
    let mut output = tokio::fs::File::create(&dest_path).await?;
    let mut hasher = Sha256::new();  // ← Add hasher

    for i in 0..metadata.total_chunks {
        let chunk_path = format!("{}/{}.chunk", chunk_dir, i);
        let encrypted_chunk = tokio::fs::read(&chunk_path).await?;

        let decrypted = decrypt_chunk_at_position(
            &session_key,
            &file_nonce,
            &encrypted_chunk,
            i as u32,
        )?;

        hasher.update(&decrypted);  // ← Hash decrypted data
        output.write_all(&decrypted).await?;
    }

    output.flush().await?;

    // Verify integrity
    let computed_hash = hex::encode(hasher.finalize());
    if computed_hash != metadata.expected_hash {
        // Clean up corrupted file
        tokio::fs::remove_file(&dest_path).await.ok();
        tokio::fs::remove_dir_all(&chunk_dir).await.ok();

        return Err(anyhow::anyhow!(
            "File integrity check failed: expected {}, got {}",
            metadata.expected_hash,
            computed_hash
        ).into());
    }

    // Cleanup temp files
    tokio::fs::remove_dir_all(&chunk_dir).await.ok();

    Ok(axum::Json(json!({
        "success": true,
        "path": relative_path,
        "size": metadata.file_size,
        "sha256": computed_hash
    })))
}
```

**Step 3: Add hash to ChunkMetadata**

`src/transfer/chunk.rs`:
```rust
#[derive(Serialize, Deserialize)]
pub struct ChunkMetadata {
    pub relative_path: String,
    pub file_name: String,
    pub total_chunks: usize,
    pub file_size: u64,
    pub completed_chunks: HashSet<usize>,
    pub nonce: String,
    pub expected_hash: String,  // ← Add this
}
```

Browser needs to send this in first chunk:
```javascript
// templates/upload/upload.js
const fileHash = await calculateFileHash(file);

formData.append('expectedHash', fileHash);  // Send with first chunk
```

### Performance Impact

**Cost:** ~1-2% overhead
- Hashing happens during decryption (no extra I/O)
- SHA-256 is fast (~500 MB/s on modern CPUs)
- One extra hash per file (negligible)

**Benefit:** Guaranteed file integrity, prevents silent corruption

---

## Issue 13: Sequential Chunk Processing

### Current Implementation

**Location:** `src/transfer/receive.rs:156-171`

```rust
// Sequential processing
for i in 0..metadata.total_chunks {
    let chunk_path = format!("{}/{}.chunk", chunk_dir, i);
    let encrypted_chunk = tokio::fs::read(&chunk_path).await?;  // Async I/O

    let decrypted = decrypt_chunk_at_position(               // CPU-bound
        &session_key,
        &file_nonce,
        &encrypted_chunk,
        i as u32,
    )?;

    output.write_all(&decrypted).await?;                     // Async I/O
}
```

### Performance Analysis

**Timeline for 1000 chunks (256KB each = 256MB file):**

```
Sequential:
├─ Read chunk 0    [2ms disk I/O]
├─ Decrypt chunk 0 [0.5ms CPU]
├─ Write chunk 0   [2ms disk I/O]
├─ Read chunk 1    [2ms disk I/O]  ← Disk idle during decrypt
├─ Decrypt chunk 1 [0.5ms CPU]     ← CPU idle during I/O
├─ Write chunk 1   [2ms disk I/O]
...
Total: 1000 × 4.5ms = 4.5 seconds
```

**Problem:** CPU is idle during I/O, disk is idle during decryption. Pipeline stalls.

### Parallel Solution

**Strategy:** Pipeline I/O and CPU work using async streams

```rust
use futures::stream::{self, StreamExt, TryStreamExt};
use tokio::io::AsyncWriteExt;

pub async fn finalize_upload(
    Path(token): Path<String>,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<axum::Json<Value>, AppError> {
    // ... existing validation ...

    let session_key = EncryptionKey::from_base64(&state.session_key)?;
    let file_nonce = Nonce::from_base64(&metadata.nonce)?;

    // Create output file
    let mut output = tokio::fs::File::create(&dest_path).await?;

    // Parallel decrypt pipeline
    let decrypted_chunks = stream::iter(0..metadata.total_chunks)
        .map(|i| {
            let chunk_dir = chunk_dir.clone();
            let session_key = session_key.clone();
            let file_nonce = file_nonce.clone();

            async move {
                // Read chunk (async I/O)
                let chunk_path = format!("{}/{}.chunk", chunk_dir, i);
                let encrypted_chunk = tokio::fs::read(&chunk_path).await?;

                // Decrypt (CPU-bound, but in task pool)
                let decrypted = tokio::task::spawn_blocking(move || {
                    decrypt_chunk_at_position(
                        &session_key,
                        &file_nonce,
                        &encrypted_chunk,
                        i as u32,
                    )
                }).await??;

                Ok::<(usize, Vec<u8>), anyhow::Error>((i, decrypted))
            }
        })
        .buffer_unordered(4);  // ← 4 parallel decrypt operations

    // Collect results maintaining order
    let mut chunks: Vec<(usize, Vec<u8>)> = decrypted_chunks
        .try_collect()
        .await?;

    // Sort by chunk index (buffer_unordered doesn't preserve order)
    chunks.sort_by_key(|(i, _)| *i);

    // Write sequentially (must be sequential for file integrity)
    for (_, decrypted) in chunks {
        output.write_all(&decrypted).await?;
    }

    output.flush().await?;

    // ... cleanup ...
}
```

### Performance Gain

**Timeline for 1000 chunks with 4-way parallelism:**

```
Parallel (4 workers):
├─ Worker 1: Read 0, Decrypt 0, Read 4, Decrypt 4, ...
├─ Worker 2: Read 1, Decrypt 1, Read 5, Decrypt 5, ...
├─ Worker 3: Read 2, Decrypt 2, Read 6, Decrypt 6, ...
└─ Worker 4: Read 3, Decrypt 3, Read 7, Decrypt 7, ...

Total: ~1000/4 × 4.5ms + overhead = 1.2 seconds
```

**Speedup:** 3.7x faster

### BUT: Memory Trade-off

**Problem:** You're buffering all chunks in memory before writing!

For 1000 chunks × 256KB = 256MB in memory. This violates your design goal of constant memory usage.

### Better Solution: Ordered Pipeline

```rust
use futures::stream::{self, StreamExt};
use tokio::sync::mpsc;

pub async fn finalize_upload(...) -> Result<...> {
    // ... validation ...

    let (tx, mut rx) = mpsc::channel::<(usize, Vec<u8>)>(8);  // Bounded buffer

    // Spawn decrypt workers
    let decrypt_task = tokio::spawn({
        let chunk_dir = chunk_dir.clone();
        let session_key = session_key.clone();
        let file_nonce = file_nonce.clone();

        async move {
            stream::iter(0..metadata.total_chunks)
                .map(|i| {
                    let chunk_dir = chunk_dir.clone();
                    let session_key = session_key.clone();
                    let file_nonce = file_nonce.clone();
                    let tx = tx.clone();

                    async move {
                        let chunk_path = format!("{}/{}.chunk", chunk_dir, i);
                        let encrypted_chunk = tokio::fs::read(&chunk_path).await?;

                        let decrypted = tokio::task::spawn_blocking(move || {
                            decrypt_chunk_at_position(&session_key, &file_nonce, &encrypted_chunk, i as u32)
                        }).await??;

                        tx.send((i, decrypted)).await.ok();
                        Ok::<(), anyhow::Error>(())
                    }
                })
                .buffer_unordered(4)
                .try_collect::<()>()
                .await
        }
    });

    // Write chunks in order as they arrive
    let mut output = tokio::fs::File::create(&dest_path).await?;
    let mut next_chunk = 0;
    let mut buffer: HashMap<usize, Vec<u8>> = HashMap::new();

    while let Some((idx, data)) = rx.recv().await {
        buffer.insert(idx, data);

        // Write chunks in order
        while let Some(data) = buffer.remove(&next_chunk) {
            output.write_all(&data).await?;
            next_chunk += 1;
        }
    }

    decrypt_task.await??;
    output.flush().await?;

    // ... cleanup ...
}
```

**Memory usage:** Bounded to 8 chunks max (~2MB) instead of all chunks (256MB)

### YAGNI Consideration

**Your use case:** Quick file transfers, not large files

**Question:** Is 4.5s vs 1.2s for 256MB worth the complexity?

**Recommendation:**
- Keep sequential for v1 (simpler, easier to debug)
- Add parallel as optional feature flag for large files
- Document this as a known optimization opportunity

---

## Issue 14: Unnecessary Disk I/O in Receive Mode

### Current Implementation

**Flow:**
```
Browser → Server: Encrypted chunk (256KB)
                ↓
        tokio::fs::write("/tmp/archdrop/{token}/{file_id}/{i}.chunk")
                ↓
        [Chunk sits on disk]
                ↓
        [All chunks uploaded]
                ↓
        For each chunk:
            tokio::fs::read("/tmp/archdrop/...")
            decrypt()
            write to final file
                ↓
        tokio::fs::remove_dir_all("/tmp/archdrop/{token}")
```

**I/O operations per chunk:**
1. Write encrypted chunk to temp
2. Read encrypted chunk from temp
3. Write decrypted chunk to final file
4. Delete temp chunk

**Total:** 4 disk operations per chunk

### Why This Is Wasteful

**For a 10MB file (40 chunks):**
```
Current:
- 40 temp writes   (10MB)
- 40 temp reads    (10MB)
- 40 final writes  (10MB)
- 40 temp deletes
= 30MB disk I/O + 40 delete ops

Optimal:
- 40 final writes  (10MB)
= 10MB disk I/O
```

**Waste:** 3x the necessary I/O

**SSD wear:** Writing + deleting encrypted chunks serves no purpose for small files

### Root Cause

The design assumes:
1. Files might be huge (> available RAM)
2. Chunks might arrive out of order (need buffering)
3. Upload might fail midway (need persistence)

**Reality for your use case:**
- Most transfers are < 100MB (fits in RAM easily)
- HTTP uploads are reliable (browser handles retries)
- Session is single-use (no resume needed)

### Solution: Hybrid Storage Strategy

```rust
// src/transfer/storage.rs

const MEMORY_THRESHOLD: u64 = 50 * 1024 * 1024;  // 50MB

pub enum ChunkStorage {
    Memory {
        chunks: HashMap<usize, Vec<u8>>,
        metadata: ChunkMetadata,
    },
    Disk {
        base_path: PathBuf,
    },
}

impl ChunkStorage {
    pub async fn new(token: &str, file_id: &str, file_size: u64) -> Result<Self> {
        if file_size <= MEMORY_THRESHOLD {
            Ok(ChunkStorage::Memory {
                chunks: HashMap::new(),
                metadata: ChunkMetadata::default(),
            })
        } else {
            let base_path = PathBuf::from(format!("/tmp/archdrop/{}/{}", token, file_id));
            tokio::fs::create_dir_all(&base_path).await?;
            Ok(ChunkStorage::Disk { base_path })
        }
    }

    pub async fn store_chunk(&mut self, index: usize, data: Vec<u8>) -> Result<()> {
        match self {
            ChunkStorage::Memory { chunks, metadata } => {
                chunks.insert(index, data);
                metadata.completed_chunks.insert(index);
                Ok(())
            }
            ChunkStorage::Disk { base_path } => {
                let chunk_path = base_path.join(format!("{}.chunk", index));
                tokio::fs::write(&chunk_path, data).await?;
                Ok(())
            }
        }
    }

    pub async fn finalize(
        self,
        output_path: &Path,
        key: &EncryptionKey,
        nonce: &Nonce,
        total_chunks: usize,
    ) -> Result<()> {
        let mut output = tokio::fs::File::create(output_path).await?;

        match self {
            ChunkStorage::Memory { chunks, .. } => {
                // Decrypt directly from memory
                for i in 0..total_chunks {
                    let encrypted = chunks.get(&i)
                        .ok_or_else(|| anyhow::anyhow!("Missing chunk {}", i))?;

                    let decrypted = decrypt_chunk_at_position(key, nonce, encrypted, i as u32)?;
                    output.write_all(&decrypted).await?;
                }
            }
            ChunkStorage::Disk { base_path } => {
                // Existing disk-based logic
                for i in 0..total_chunks {
                    let chunk_path = base_path.join(format!("{}.chunk", i));
                    let encrypted = tokio::fs::read(&chunk_path).await?;
                    let decrypted = decrypt_chunk_at_position(key, nonce, &encrypted, i as u32)?;
                    output.write_all(&decrypted).await?;
                }

                tokio::fs::remove_dir_all(&base_path).await.ok();
            }
        }

        output.flush().await?;
        Ok(())
    }
}
```

**Usage in receive handler:**

```rust
// src/transfer/receive.rs

use crate::transfer::storage::ChunkStorage;

// Store per-session storage (not per-chunk)
pub struct ReceiveSession {
    storage: Arc<Mutex<ChunkStorage>>,
    // ...
}

pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<axum::Json<Value>, AppError> {
    let chunk = parse_chunk_upload(multipart).await?;

    // Get or create storage
    let mut storage = state.get_storage(&token, &chunk).await?;

    // Store chunk (automatically chooses memory or disk)
    storage.store_chunk(chunk.chunk_index, chunk.data).await?;

    Ok(axum::Json(json!({
        "success": true,
        "chunk": chunk.chunk_index
    })))
}
```

### Performance Impact

**For 10MB file:**
```
Before: 30MB I/O
After:  10MB I/O
Speedup: 3x faster
```

**For 1GB file:**
```
Before: 3GB I/O (uses disk)
After:  3GB I/O (uses disk)
Speedup: None (but no regression)
```

### YAGNI Analysis

**Arguments for simplicity:**
- Current code is easier to understand
- Disk-based approach handles all file sizes
- Optimization is premature

**Arguments for hybrid:**
- 90% of transfers likely < 50MB
- 3x speedup is significant for UX
- Memory usage is bounded and predictable
- Code complexity is moderate

**Recommendation:** Document current approach, add hybrid as future enhancement if users complain about speed.

---

## Issue 15: Metadata Rewritten on Every Chunk

### Current Implementation

**Location:** `src/transfer/chunk.rs:102-115`

```rust
pub async fn update_chunk_metadata(
    token: &str,
    file_id: &str,
    metadata: &mut ChunkMetadata,
    chunk_index: usize,
) -> anyhow::Result<()> {
    metadata.completed_chunks.insert(chunk_index);  // In-memory update

    let metadata_path = format!("/tmp/archdrop/{}/{}/metadata.json", token, file_id);
    let json = serde_json::to_string_pretty(metadata)?;  // Serialize entire struct
    tokio::fs::write(&metadata_path, json).await?;       // Write entire file

    Ok(())
}
```

**Called from:** `src/transfer/receive.rs:35`

```rust
pub async fn receive_handler(...) -> Result<...> {
    // ... parse chunk ...

    let mut metadata = load_or_create_metadata(&token, &chunk).await?;
    save_encrypted_chunk(&token, &file_id, chunk.chunk_index, &chunk.data).await?;
    update_chunk_metadata(&token, &file_id, &mut metadata, chunk.chunk_index).await?;
    //                                                                          ^^^^^^
    //                                                     Rewrite entire file every chunk

    Ok(...)
}
```

### The Problem

**For a 256MB file (1000 chunks):**

```
Chunk 0:   Write metadata.json (100 bytes)
Chunk 1:   Write metadata.json (104 bytes)  ← 1 more chunk listed
Chunk 2:   Write metadata.json (108 bytes)
...
Chunk 999: Write metadata.json (4KB)        ← 1000 chunks listed

Total: 1000 metadata writes
```

**Disk operations:**
```
Metadata writes: 1000
Data written:    ~2MB (100 bytes + 104 + 108 + ... + 4000)
```

**Why this is bad:**
1. **SSD wear:** 1000 writes to same file (write amplification)
2. **Performance:** Each write has syscall overhead
3. **Atomic write concerns:** Partial write could corrupt metadata
4. **Lock contention:** If parallel uploads, metadata becomes bottleneck

### Visualization

```
metadata.json evolution:

Chunk 0:
{
  "relative_path": "test.bin",
  "total_chunks": 1000,
  "completed_chunks": [0],
  "file_size": 256000000
}

Chunk 1:
{
  "relative_path": "test.bin",
  "total_chunks": 1000,
  "completed_chunks": [0, 1],  ← Only this line changed
  "file_size": 256000000
}

Chunk 2:
{
  "relative_path": "test.bin",
  "total_chunks": 1000,
  "completed_chunks": [0, 1, 2],  ← Only this line changed
  "file_size": 256000000
}

... 997 more rewrites ...
```

### Solution 1: Batch Updates

**Strategy:** Only persist metadata every N chunks

```rust
const METADATA_BATCH_SIZE: usize = 10;

pub async fn update_chunk_metadata(
    token: &str,
    file_id: &str,
    metadata: &mut ChunkMetadata,
    chunk_index: usize,
) -> anyhow::Result<()> {
    metadata.completed_chunks.insert(chunk_index);

    // Only persist every 10th chunk or on completion
    if metadata.completed_chunks.len() % METADATA_BATCH_SIZE == 0
        || metadata.completed_chunks.len() == metadata.total_chunks {
        let metadata_path = format!("/tmp/archdrop/{}/{}/metadata.json", token, file_id);
        let json = serde_json::to_string_pretty(metadata)?;
        tokio::fs::write(&metadata_path, json).await?;
    }

    Ok(())
}
```

**Result:** 1000 chunks → 100 metadata writes (10x reduction)

**Risk:** If server crashes, lose up to 10 chunks of progress

### Solution 2: Append-Only Log

**Strategy:** Append chunk number to log file, reconstruct on read

```rust
pub async fn update_chunk_metadata(
    token: &str,
    file_id: &str,
    metadata: &mut ChunkMetadata,
    chunk_index: usize,
) -> anyhow::Result<()> {
    metadata.completed_chunks.insert(chunk_index);

    let log_path = format!("/tmp/archdrop/{}/{}/chunks.log", token, file_id);

    // Append single line
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .await?;

    file.write_all(format!("{}\n", chunk_index).as_bytes()).await?;

    Ok(())
}

pub async fn load_metadata(token: &str, file_id: &str) -> anyhow::Result<ChunkMetadata> {
    let log_path = format!("/tmp/archdrop/{}/{}/chunks.log", token, file_id);

    if !Path::new(&log_path).exists() {
        return Ok(ChunkMetadata::default());
    }

    let content = tokio::fs::read_to_string(&log_path).await?;
    let completed_chunks: HashSet<usize> = content
        .lines()
        .filter_map(|line| line.parse().ok())
        .collect();

    Ok(ChunkMetadata {
        completed_chunks,
        // ... other fields from separate metadata file ...
    })
}
```

**Benefits:**
- Single append per chunk (no rewrite)
- Atomic (single line write)
- Easy to debug (text file)

**Drawbacks:**
- Must parse entire log to get state
- Log grows linearly with chunks

### Solution 3: In-Memory Only (YAGNI)

**Key insight:** Why persist at all?

**Current usage of completed_chunks:**
1. Resume capability → You don't support resume
2. Status endpoint → Could query filesystem instead
3. Validation → Done at finalize time

**Alternative:** Keep in memory, verify at finalize

```rust
// Remove update_chunk_metadata entirely

pub async fn receive_handler(...) -> Result<...> {
    let chunk = parse_chunk_upload(multipart).await?;

    // Just save the chunk
    save_encrypted_chunk(&token, &file_id, chunk.chunk_index, &chunk.data).await?;

    // No metadata update!

    Ok(axum::Json(json!({
        "success": true,
        "chunk": chunk.chunk_index
    })))
}

pub async fn chunk_status(...) -> Result<...> {
    // Check filesystem instead of metadata
    let chunk_dir = format!("/tmp/archdrop/{}/{}", token, file_id);

    let mut completed = Vec::new();
    let mut entries = tokio::fs::read_dir(&chunk_dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        if let Some(name) = entry.file_name().to_str() {
            if name.ends_with(".chunk") {
                if let Some(idx) = name.strip_suffix(".chunk").and_then(|s| s.parse().ok()) {
                    completed.push(idx);
                }
            }
        }
    }

    completed.sort();

    Ok(axum::Json(json!({
        "completed_chunks": completed,
        "total_chunks": query.total_chunks
    })))
}
```

**Benefits:**
- Zero metadata writes
- Simpler code
- Filesystem is source of truth

**Drawbacks:**
- Status check requires directory scan (slower)
- No persistent metadata (but you don't need it)

### Recommendation

**For your use case (single-use sessions, no resume):**

Use **Solution 3 (In-Memory Only)** - it's the YAGNI approach.

**If you add resume support later:**

Use **Solution 2 (Append-Only Log)** - it's crash-safe and efficient.

**Avoid:**

Current approach (rewrite entire file) - it's the worst of all worlds.

---

## Issue 16: Memory Allocation in Hot Path

### Current Implementation

**Location:** `src/crypto/stream.rs:56-59`

```rust
let len = encrypted.len() as u32;
let mut framed = len.to_be_bytes().to_vec();  // ← Allocates Vec with 4 bytes
framed.extend_from_slice(&encrypted);          // ← Reallocates Vec to 4 + encrypted.len()

Some(Ok(framed))
```

### What Happens Under the Hood

**Step by step:**

```rust
// Step 1: encrypted is ~65KB after AES-GCM (includes auth tag)
let encrypted: Vec<u8> = self.encryptor.encrypt_next(chunk)?;
// encrypted.len() = 65536 + 16 (GCM tag) = 65552 bytes

// Step 2: Convert length to bytes
let len = 65552_u32;
let len_bytes: [u8; 4] = len.to_be_bytes();  // Stack allocation, fine

// Step 3: Convert to Vec (ALLOCATION #1)
let mut framed = len_bytes.to_vec();
// Allocates: 4 bytes
// Capacity: 4 bytes

// Step 4: Extend with encrypted data (ALLOCATION #2)
framed.extend_from_slice(&encrypted);
// Needs:    4 + 65552 = 65556 bytes
// Capacity: 4 bytes
// Result:   Reallocate to ~131KB (Vec growth strategy doubles capacity)
//           Copy 4 bytes from old buffer
//           Copy 65552 bytes from encrypted
```

### Allocator Behavior

**Rust's Vec growth strategy:**
```rust
// Simplified Vec::extend_from_slice
if self.capacity() < self.len() + additional {
    // Double capacity (or use exact size if huge)
    let new_cap = max(self.capacity() * 2, self.len() + additional);
    self.realloc(new_cap);  // ← Expensive: malloc + memcpy + free
}
```

**For our case:**
```
Initial capacity: 4 bytes
Required:         65556 bytes
New capacity:     131072 bytes (next power of 2)

Operations:
1. malloc(131KB)
2. memcpy(4 bytes, old → new)
3. memcpy(65KB, encrypted → new)
4. free(old 4 byte buffer)
```

### Performance Impact

**Per chunk:**
- 1 malloc
- 1 free
- 2 memcpy operations
- ~65KB wasted capacity (allocated 131KB, use 65KB)

**For 1000 chunks:**
- 1000 malloc/free pairs
- 2000 memcpy operations
- Memory fragmentation

**Benchmark (rough estimate):**
```
malloc/free:  ~100ns each = 200ns
memcpy 65KB:  ~50ns @ 20GB/s = 50ns
Total:        ~250ns overhead per chunk

For 1000 chunks: 250μs overhead
```

**Is this significant?** No, not really. Decryption takes ~500μs per chunk, so this is <1% overhead.

**But:** It's sloppy and violates SIMPLE principle (unnecessary complexity).

### The Fix

**Preallocate correct size:**

```rust
let len = encrypted.len() as u32;
let mut framed = Vec::with_capacity(4 + encrypted.len());  // ← Allocate once
framed.extend_from_slice(&len.to_be_bytes());
framed.extend_from_slice(&encrypted);

Some(Ok(framed))
```

**What changes:**
```
Operations:
1. malloc(65556 bytes)          ← Single allocation
2. memcpy(4 bytes, stack → heap)
3. memcpy(65552 bytes, encrypted → heap)
4. free(when framed is dropped)

No reallocation!
```

### Even Better: Reuse Buffer

**Problem:** Still allocating new Vec every chunk

**Solution:** Reuse buffer across chunks

```rust
pub struct EncryptedFileStream {
    file: File,
    encryptor: EncryptorBE32<Aes256Gcm>,
    buffer: [u8; 65536],
    frame_buffer: Vec<u8>,  // ← Add reusable buffer
    bytes_sent: u64,
    total_size: u64,
    progress_sender: watch::Sender<f64>,
}

impl EncryptedFileStream {
    pub fn new(...) -> Self {
        Self {
            file,
            encryptor,
            buffer: [0u8; 65536],
            frame_buffer: Vec::with_capacity(65536 + 16 + 4),  // ← Preallocate
            bytes_sent: 0,
            total_size,
            progress_sender,
        }
    }

    pub async fn read_next_chunk(&mut self) -> Option<Result<Vec<u8>, std::io::Error>> {
        match self.file.read(&mut self.buffer).await {
            Ok(0) => None,
            Ok(n) => {
                let chunk = &self.buffer[..n];

                let encrypted = match self.encryptor.encrypt_next(chunk) {
                    Ok(enc) => enc,
                    Err(e) => return Some(Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("Encryption failed: {:?}", e),
                    )))
                };

                // Reuse frame_buffer
                self.frame_buffer.clear();  // ← Reset length, keep capacity
                let len = encrypted.len() as u32;
                self.frame_buffer.extend_from_slice(&len.to_be_bytes());
                self.frame_buffer.extend_from_slice(&encrypted);

                // Must clone because we're returning and keeping self
                let framed = self.frame_buffer.clone();

                self.bytes_sent += n as u64;
                let progress = (self.bytes_sent as f64 / self.total_size as f64) * 100.0;
                let _ = self.progress_sender.send(progress);

                Some(Ok(framed))
            }
            Err(e) => Some(Err(e)),
        }
    }
}
```

**Wait, we're still cloning!**

Yes, because the stream needs to own the buffer while yielding it to Axum.

**Can we avoid the clone?**

Not easily. Axum's Body::from_stream takes ownership of each chunk.

**Alternative:** Use bytes::Bytes (reference-counted)

```rust
use bytes::{Bytes, BytesMut};

pub async fn read_next_chunk(&mut self) -> Option<Result<Bytes, std::io::Error>> {
    match self.file.read(&mut self.buffer).await {
        Ok(0) => None,
        Ok(n) => {
            let encrypted = /* ... */;

            let mut framed = BytesMut::with_capacity(4 + encrypted.len());
            framed.extend_from_slice(&(encrypted.len() as u32).to_be_bytes());
            framed.extend_from_slice(&encrypted);

            Some(Ok(framed.freeze()))  // ← Zero-copy conversion to Bytes
        }
        Err(e) => Some(Err(e)),
    }
}
```

**Benefits:**
- BytesMut → Bytes is zero-copy (just refcount increment)
- Axum can efficiently work with Bytes
- No clone needed

### Recommendation

**Priority 1:** Use `Vec::with_capacity` - trivial fix, eliminates reallocation

**Priority 2:** Switch to bytes::Bytes - better integration with Axum, but requires dependency

**YAGNI check:** Is this worth it?
- Saves ~250μs per 1000 chunks = negligible
- But improves code clarity
- Follows best practices

**Verdict:** Do it, it's a 2-line fix.

---

## Issue 21: No Cleanup on Error

### Current Implementation

**Location:** `src/transfer/receive.rs:83-181`

```rust
pub async fn finalize_upload(...) -> Result<axum::Json<Value>, AppError> {
    // ... validation ...

    // Create destination file
    let mut output = tokio::fs::File::create(&dest_path).await?;
    //                                                       ^^^
    //                              If this fails, no cleanup

    // Decrypt and write chunks
    for i in 0..metadata.total_chunks {
        let chunk_path = format!("{}/{}.chunk", chunk_dir, i);
        let encrypted_chunk = tokio::fs::read(&chunk_path).await?;
        //                                                      ^^^
        //                                    If read fails, partial file remains

        let decrypted = decrypt_chunk_at_position(...)?;
        //                                            ^^^
        //                       If decryption fails, partial file remains

        output.write_all(&decrypted).await?;
        //                                ^^^
        //                  If write fails (disk full), partial file remains
    }

    // Cleanup ONLY on success
    tokio::fs::remove_dir_all(&chunk_dir).await.ok();

    Ok(...)
}
```

### What Goes Wrong

**Scenario 1: Disk full**

```
1. User uploads 1GB file
2. Server stores encrypted chunks in /tmp (1GB used)
3. Finalization starts decrypting
4. After 500MB written, disk is full
5. write_all() returns error
6. Function returns early via ?
7. Result:
   - Partial file (500MB) left in destination
   - Encrypted chunks (1GB) left in /tmp
   - Total waste: 1.5GB
```

**Scenario 2: Decryption failure**

```
1. Chunk file corrupted (bit flip on disk)
2. AES-GCM authentication fails
3. decrypt_chunk_at_position() returns error
4. Function returns early via ?
5. Result:
   - Partial file left in destination
   - All encrypted chunks left in /tmp
```

**Scenario 3: Server crash**

```
1. Power outage during finalization
2. Process killed
3. Result:
   - Partial file left in destination
   - All encrypted chunks left in /tmp
   - No cleanup ever happens
```

### Why This Violates RAII

**RAII = Resource Acquisition Is Initialization**

Rust's core principle: Resources should be tied to object lifetimes.

**Current code violates this:**
```rust
// Acquire resources manually
let output = File::create(&dest_path).await?;
let chunk_dir = "/tmp/...";

// ... use resources ...

// Cleanup manually
tokio::fs::remove_dir_all(&chunk_dir).await.ok();
//                                              ^^^
//                                    Only if we get here!
```

**RAII approach:**
```rust
// Acquire resource
let _guard = TempDirGuard::new(chunk_dir);

// ... use resources ...

// Cleanup automatic (Drop trait)
// Runs even if function panics or returns early
```

### Solution: Cleanup Guards

**Step 1: TempDir Guard**

```rust
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct TempDirGuard {
    path: Option<PathBuf>,  // Option to allow disarming
}

impl TempDirGuard {
    pub fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    /// Disarm the guard (don't cleanup on drop)
    pub fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            // Spawn blocking to avoid blocking the async executor
            std::thread::spawn(move || {
                let _ = std::fs::remove_dir_all(&path);
            });
        }
    }
}
```

**Step 2: Partial File Guard**

```rust
pub struct PartialFileGuard {
    path: Option<PathBuf>,
}

impl PartialFileGuard {
    pub fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    pub fn disarm(&mut self) {
        self.path = None;
    }
}

impl Drop for PartialFileGuard {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            std::thread::spawn(move || {
                let _ = std::fs::remove_file(&path);
            });
        }
    }
}
```

**Step 3: Use Guards**

```rust
pub async fn finalize_upload(...) -> Result<axum::Json<Value>, AppError> {
    // ... validation ...

    // Setup cleanup guards
    let mut temp_guard = TempDirGuard::new(PathBuf::from(&chunk_dir));

    let mut output = tokio::fs::File::create(&dest_path).await?;
    let mut file_guard = PartialFileGuard::new(dest_path.clone());

    // Decrypt and write chunks
    for i in 0..metadata.total_chunks {
        let chunk_path = format!("{}/{}.chunk", chunk_dir, i);
        let encrypted_chunk = tokio::fs::read(&chunk_path).await?;

        let decrypted = decrypt_chunk_at_position(
            &session_key,
            &file_nonce,
            &encrypted_chunk,
            i as u32,
        )?;

        output.write_all(&decrypted).await?;
    }

    output.flush().await?;

    // Success! Disarm guards
    temp_guard.disarm();   // Keep temp dir cleaned up
    file_guard.disarm();   // Keep final file

    Ok(axum::Json(json!({
        "success": true,
        "path": relative_path,
        "size": metadata.file_size
    })))
}

// If function returns early via ?, guards auto-cleanup in Drop
```

### Error Flow Comparison

**Before:**
```
Error at chunk 500 →
    ? operator returns error →
        Function exits →
            Temp files remain ❌
            Partial file remains ❌
```

**After:**
```
Error at chunk 500 →
    ? operator returns error →
        Function exits →
            Drop runs on temp_guard →
                Temp dir deleted ✓
            Drop runs on file_guard →
                Partial file deleted ✓
```

### Advanced: Atomic File Creation

**Problem:** Even with guards, there's a window where partial file exists

**Solution:** Write to temp, then atomic rename

```rust
pub async fn finalize_upload(...) -> Result<...> {
    // ... validation ...

    let temp_guard = TempDirGuard::new(PathBuf::from(&chunk_dir));

    // Write to temporary location
    let temp_output = dest_path.with_extension(".tmp");
    let mut temp_guard_file = PartialFileGuard::new(temp_output.clone());

    let mut output = tokio::fs::File::create(&temp_output).await?;

    // Decrypt chunks...
    for i in 0..metadata.total_chunks {
        // ... decrypt and write ...
    }

    output.flush().await?;
    drop(output);  // Close file

    // Atomic rename (succeeds or fails completely)
    tokio::fs::rename(&temp_output, &dest_path).await?;

    // Success! Disarm guards
    temp_guard_file.disarm();  // Temp file was renamed, don't delete
    temp_guard.disarm();

    Ok(...)
}
```

**Benefits:**
- Destination file appears atomically (all-or-nothing)
- No partial files visible to user
- Crash-safe

**Trade-off:**
- Rename may fail if crossing filesystem boundaries
- Requires extra disk space for temp file

### Recommendation

**Minimum (P0):** Implement TempDirGuard - prevents /tmp accumulation

**Better (P1):** Implement both guards - prevents partial files

**Best (P2):** Add atomic rename - provides crash safety

---

## Summary Table

| Issue | Impact | Complexity | YAGNI? | Recommendation |
|-------|--------|------------|--------|----------------|
| 7: Missing Integrity | Security | Medium | No | Fix (P0) |
| 13: Sequential Processing | Performance | High | Yes | Document, defer |
| 14: Unnecessary Disk I/O | Performance | Medium | Yes | Defer to v2 |
| 15: Metadata Rewrites | Performance | Low | Yes | Use in-memory approach |
| 16: Memory Allocation | Performance | Low | Yes | Fix (trivial) |
| 21: No Cleanup on Error | Reliability | Low | No | Fix (P1) |

**Priority order for fixes:**
1. Issue 7 (integrity) - Security critical
2. Issue 21 (cleanup) - Reliability critical
3. Issue 16 (allocation) - Easy win
4. Issue 15 (metadata) - Simple refactor
5. Issue 14 (disk I/O) - Only if performance matters
6. Issue 13 (parallel) - Only for large files
