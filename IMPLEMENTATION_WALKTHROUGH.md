# Implementation Walkthrough: How The Code Works

## Table of Contents
1. [ChunkStorage: The Core Abstraction](#chunkstorage-the-core-abstraction)
2. [PartialFileGuard: RAII Cleanup](#partialfileguard-raii-cleanup)
3. [Receive Handler: Upload Flow](#receive-handler-upload-flow)
4. [Send Chunk Handler: Download Flow](#send-chunk-handler-download-flow)
5. [Manifest Hash Calculation](#manifest-hash-calculation)
6. [Browser Download: Client-Side](#browser-download-client-side)
7. [Rust Concepts Explained](#rust-concepts-explained)

---

## ChunkStorage: The Core Abstraction

### The Enum Definition

```rust
pub enum ChunkStorage {
    Memory {
        chunks: HashMap<usize, Vec<u8>>,
    },
    DirectWrite {
        output_file: File,
        hasher: Sha256,
        chunks_received: HashSet<usize>,
        guard: PartialFileGuard,
    },
}
```

**What is an enum in Rust?**
- Unlike C/Java enums (just numbers), Rust enums can hold data
- Each variant is a different "shape" with different fields
- Only one variant exists at a time

**Memory variant:**
- For files < 100MB
- `HashMap<usize, Vec<u8>>`: Key = chunk index, Value = encrypted bytes
- Simple: just store chunks in RAM, decrypt later

**DirectWrite variant:**
- For files ≥ 100MB
- `output_file`: The destination file being written
- `hasher`: SHA-256 state updated as chunks arrive
- `chunks_received`: Track which chunks we got (no duplicates)
- `guard`: Cleanup helper (deletes file on error)

### Creating Storage: `new()`

```rust
pub async fn new(file_size: u64, dest_path: PathBuf) -> Result<Self> {
    if file_size < MEMORY_THRESHOLD {
        Ok(ChunkStorage::Memory {
            chunks: HashMap::new(),
        })
    } else {
        // Create parent directory if needed
        if let Some(parent) = dest_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let output_file = File::create(&dest_path).await?;
        let guard = PartialFileGuard::new(dest_path);

        Ok(ChunkStorage::DirectWrite {
            output_file,
            hasher: Sha256::new(),
            chunks_received: HashSet::new(),
            guard,
        })
    }
}
```

**Step-by-step:**

1. **Check file size:**
   ```rust
   if file_size < MEMORY_THRESHOLD {
   ```
   - `MEMORY_THRESHOLD` is `100 * 1024 * 1024` (100MB)
   - This decides which variant to create

2. **Small file path:**
   ```rust
   Ok(ChunkStorage::Memory {
       chunks: HashMap::new(),
   })
   ```
   - Create empty HashMap
   - Wrap in `Ok()` because return type is `Result`
   - That's it! Nothing written to disk yet

3. **Large file path:**
   ```rust
   if let Some(parent) = dest_path.parent() {
       tokio::fs::create_dir_all(parent).await?;
   }
   ```
   - `dest_path.parent()` returns `Option<&Path>` (might not have parent)
   - `if let Some(parent)` unpacks the Option if it exists
   - `create_dir_all` makes all parent directories (like `mkdir -p`)
   - `.await?` means: wait for async operation, propagate errors with `?`

4. **Create the file:**
   ```rust
   let output_file = File::create(&dest_path).await?;
   ```
   - Opens file for writing, creates if doesn't exist
   - Async operation (non-blocking I/O)
   - `?` propagates errors up (early return if file creation fails)

5. **Create guard:**
   ```rust
   let guard = PartialFileGuard::new(dest_path);
   ```
   - Stores the path for cleanup
   - Will delete file if guard is dropped without being disarmed

6. **Return DirectWrite variant:**
   ```rust
   Ok(ChunkStorage::DirectWrite {
       output_file,
       hasher: Sha256::new(),      // Fresh SHA-256 state
       chunks_received: HashSet::new(),  // Empty set
       guard,
   })
   ```

### Storing Chunks: `store_chunk()`

```rust
pub async fn store_chunk(
    &mut self,
    chunk_index: usize,
    encrypted_data: Vec<u8>,
    key: &EncryptionKey,
    nonce: &Nonce,
) -> Result<()> {
    match self {
        ChunkStorage::Memory { chunks } => {
            chunks.insert(chunk_index, encrypted_data);
            Ok(())
        }
        ChunkStorage::DirectWrite {
            output_file,
            hasher,
            chunks_received,
            ..
        } => {
            // Decrypt chunk
            let decrypted = decrypt_chunk_at_position(
                key,
                nonce,
                &encrypted_data,
                chunk_index as u32,
            )?;

            // Update hash
            hasher.update(&decrypted);

            // Write to file
            output_file.write_all(&decrypted).await?;

            // Track chunk
            chunks_received.insert(chunk_index);

            Ok(())
        }
    }
}
```

**How `match` works:**
- Pattern matching on enum variants
- Destructures fields (pulls them out)
- `..` means "ignore other fields"

**Memory path:**
```rust
ChunkStorage::Memory { chunks } => {
    chunks.insert(chunk_index, encrypted_data);
    Ok(())
}
```
- Just insert into HashMap
- Keep data encrypted (decrypt later)
- Fast: no I/O

**DirectWrite path:**
```rust
ChunkStorage::DirectWrite {
    output_file,
    hasher,
    chunks_received,
    ..  // Ignore 'guard' field
} => {
```

1. **Decrypt immediately:**
   ```rust
   let decrypted = decrypt_chunk_at_position(
       key,
       nonce,
       &encrypted_data,
       chunk_index as u32,
   )?;
   ```
   - Stateless decryption (can decrypt chunk N without chunks 0..N-1)
   - Nonce = `[7-byte base][4-byte counter][1-byte flag]`
   - Counter = `chunk_index`
   - AES-GCM validates integrity (fails if corrupted)
   - `?` propagates decryption errors

2. **Update streaming hash:**
   ```rust
   hasher.update(&decrypted);
   ```
   - SHA-256 is stateful (maintains running state)
   - Can call `update()` multiple times
   - No need to keep all data in memory

3. **Write to disk:**
   ```rust
   output_file.write_all(&decrypted).await?;
   ```
   - Async write (doesn't block)
   - `write_all` ensures all bytes written (vs `write` which might do partial)
   - Appends to file (file position advances automatically)

4. **Track chunk received:**
   ```rust
   chunks_received.insert(chunk_index);
   ```
   - HashSet prevents duplicates
   - Used to verify all chunks received

### Finalizing: `finalize()`

```rust
pub async fn finalize(
    mut self,
    dest_path: &Path,
    key: &EncryptionKey,
    nonce: &Nonce,
    total_chunks: usize,
) -> Result<String> {
    match self {
        ChunkStorage::Memory { chunks } => {
            // ... decrypt and write ...
        }
        ChunkStorage::DirectWrite {
            mut output_file,
            hasher,
            mut guard,
            ..
        } => {
            // ... verify hash ...
        }
    }
}
```

**Important: consumes `self`**
- `mut self` (not `&mut self`)
- Takes ownership (can't use ChunkStorage after this)
- Allows moving fields out of enum

**Memory path:**
```rust
ChunkStorage::Memory { chunks } => {
    let mut output = File::create(dest_path).await?;
    let mut hasher = Sha256::new();

    // Decrypt in order
    for i in 0..total_chunks {
        let encrypted = chunks
            .get(&i)
            .ok_or_else(|| anyhow::anyhow!("Missing chunk {}", i))?;

        let decrypted = decrypt_chunk_at_position(key, nonce, encrypted, i as u32)?;

        hasher.update(&decrypted);
        output.write_all(&decrypted).await?;
    }

    output.flush().await?;

    Ok(hex::encode(hasher.finalize()))
}
```

**Step by step:**

1. **Create output file:**
   ```rust
   let mut output = File::create(dest_path).await?;
   ```
   - Now we write to disk (delayed until finalize)

2. **Loop through chunks in order:**
   ```rust
   for i in 0..total_chunks {
   ```
   - Must process sequentially (chunk 0, then 1, then 2...)
   - File data must be in correct order

3. **Get chunk from HashMap:**
   ```rust
   let encrypted = chunks
       .get(&i)
       .ok_or_else(|| anyhow::anyhow!("Missing chunk {}", i))?;
   ```
   - `get(&i)` returns `Option<&Vec<u8>>`
   - `ok_or_else()` converts `None` to `Err`
   - Closure creates error message if missing
   - `?` propagates error

4. **Decrypt, hash, write:**
   ```rust
   let decrypted = decrypt_chunk_at_position(key, nonce, encrypted, i as u32)?;
   hasher.update(&decrypted);
   output.write_all(&decrypted).await?;
   ```
   - Same as DirectWrite path, just delayed

5. **Flush and return hash:**
   ```rust
   output.flush().await?;
   Ok(hex::encode(hasher.finalize()))
   ```
   - `flush()` ensures all buffered data written to disk
   - `hasher.finalize()` consumes hasher, returns `[u8; 32]`
   - `hex::encode()` converts to hex string: "a1b2c3..."

**DirectWrite path:**
```rust
ChunkStorage::DirectWrite {
    mut output_file,
    hasher,
    mut guard,
    ..
} => {
    output_file.flush().await?;
    drop(output_file);  // Close file

    let hash = hex::encode(hasher.finalize());

    guard.disarm();  // Success! Don't delete file

    Ok(hash)
}
```

**Why simpler?**
- File already written (chunks were written as they arrived)
- Hash already calculated (updated with each chunk)
- Just need to finalize and disarm guard

**Steps:**

1. **Flush and close:**
   ```rust
   output_file.flush().await?;
   drop(output_file);
   ```
   - Ensure all data written
   - `drop()` explicitly closes file (releases file handle)

2. **Finalize hash:**
   ```rust
   let hash = hex::encode(hasher.finalize());
   ```
   - `finalize()` consumes hasher, can't use it again
   - Returns final 256-bit hash

3. **Disarm guard:**
   ```rust
   guard.disarm();
   ```
   - Tells guard not to delete file
   - File is valid, keep it
   - If we didn't disarm, Drop would delete the file

---

## PartialFileGuard: RAII Cleanup

### The Structure

```rust
pub struct PartialFileGuard {
    path: Option<PathBuf>,
}
```

**Why `Option<PathBuf>`?**
- `Some(path)` = guard is armed (will delete file)
- `None` = guard is disarmed (won't delete)

### Creation

```rust
impl PartialFileGuard {
    pub fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    pub fn disarm(&mut self) {
        self.path = None;
    }
}
```

**Simple:**
- `new()` creates armed guard
- `disarm()` sets path to `None`

### The Magic: Drop Trait

```rust
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

**What is `Drop`?**
- Trait that runs when value goes out of scope
- Like C++ destructor or Python `__del__`
- Automatically called by Rust

**When does drop run?**
```rust
{
    let guard = PartialFileGuard::new(path);
    // ... do work ...
} // <-- guard.drop() called here automatically
```

**How it works:**

1. **Check if armed:**
   ```rust
   if let Some(path) = self.path.take() {
   ```
   - `take()` moves value out of Option, leaves `None`
   - If path was `None` (disarmed), this doesn't run

2. **Spawn cleanup thread:**
   ```rust
   std::thread::spawn(move || {
       let _ = std::fs::remove_file(&path);
   });
   ```
   - `spawn()` creates new OS thread
   - `move` gives ownership of `path` to thread
   - Non-blocking (don't wait for delete)
   - `let _ =` ignores result (delete might fail, that's okay)

**Why spawn a thread?**
- `Drop` must be synchronous (can't be async)
- File deletion is I/O (might block)
- Spawn thread to avoid blocking

**Example usage:**
```rust
let guard = PartialFileGuard::new(dest_path.clone());
let mut file = File::create(&dest_path).await?;

// Write data...
file.write_all(data).await?;

// Success! Disarm guard
guard.disarm();

// ... end of scope, guard drops but does nothing (disarmed)
```

**Error scenario:**
```rust
let guard = PartialFileGuard::new(dest_path.clone());
let mut file = File::create(&dest_path).await?;

file.write_all(data).await?;  // <-- Error here!

// Never reach disarm(), error propagates via ?
// ... end of scope, guard drops and DELETES file
```

---

## Receive Handler: Upload Flow

### The Handler Function

```rust
pub async fn receive_handler(
    Path(token): Path<String>,
    State(state): State<AppState>,
    multipart: Multipart,
) -> Result<Json<Value>, AppError> {
```

**Function signature explained:**

1. **`async fn`:**
   - Function can `await` asynchronous operations
   - Returns `Future` that must be awaited

2. **Parameters are "extractors":**
   ```rust
   Path(token): Path<String>,
   ```
   - Axum automatically extracts route parameter
   - URL `/receive/abc123/chunk` → `token = "abc123"`

   ```rust
   State(state): State<AppState>,
   ```
   - Extracts shared application state
   - Cloned automatically (AppState has Arc<...> inside)

   ```rust
   multipart: Multipart,
   ```
   - Parses `multipart/form-data` request body
   - Contains chunk data + metadata

3. **Return type:**
   ```rust
   Result<Json<Value>, AppError>
   ```
   - `Ok(Json(value))` = HTTP 200 with JSON body
   - `Err(AppError)` = HTTP error (converted to error response)

### Step 1: Validate Token

```rust
if !state.session.is_valid(&token).await {
    return Err(anyhow::anyhow!("Invalid token").into());
}
```

**What happens:**
- `state.session.is_valid()` checks:
  - Token matches expected UUID
  - Session not marked as used
- Returns `bool`
- `anyhow::anyhow!()` creates error with message
- `.into()` converts to `AppError`

### Step 2: Parse Chunk Upload

```rust
let chunk = parse_chunk_upload(multipart).await?;
```

**This function extracts:**
- `chunk_data`: Encrypted bytes
- `relative_path`: File path
- `chunk_index`: Which chunk this is
- `total_chunks`: How many total
- `file_size`: Total file size
- `nonce`: Per-file nonce (only in first chunk)

### Step 3: Get or Create Session

```rust
let file_id = hash_path(&chunk.relative_path);
let mut sessions = state.upload_sessions.write().await;

let session = sessions.entry(file_id.clone()).or_insert_with(|| {
    // ... create session ...
});
```

**Breaking it down:**

1. **Hash the path:**
   ```rust
   let file_id = hash_path(&chunk.relative_path);
   ```
   - SHA-256 of path (deterministic ID)
   - Multiple files can be uploaded simultaneously

2. **Lock sessions map:**
   ```rust
   let mut sessions = state.upload_sessions.write().await;
   ```
   - `upload_sessions` is `Arc<RwLock<HashMap<...>>>`
   - `write().await` acquires exclusive lock (async)
   - Returns `RwLockWriteGuard` (RAII lock)
   - Lock released when guard dropped

3. **Entry API:**
   ```rust
   sessions.entry(file_id.clone())
   ```
   - Efficient way to "get or insert"
   - Returns `Entry` enum (Occupied or Vacant)

4. **Or insert with closure:**
   ```rust
   .or_insert_with(|| {
       // This only runs if key doesn't exist
       UploadSession { ... }
   })
   ```
   - Closure called lazily (only if needed)
   - Returns mutable reference to value

**Inside the closure:**
```rust
let destination = state.session.get_destination().unwrap().clone();
let dest_path = destination.join(&chunk.relative_path);

let storage = ChunkStorage::new(chunk.file_size, dest_path)
    .await
    .expect("Failed to create storage");

UploadSession {
    storage,
    total_chunks: chunk.total_chunks,
    nonce: chunk.nonce.clone().unwrap_or_default(),
    relative_path: chunk.relative_path.clone(),
    file_size: chunk.file_size,
}
```

- Gets destination from session
- Joins paths (handles subdirectories)
- Creates ChunkStorage (memory or disk based on size)
- Returns new UploadSession

### Step 4: Check for Duplicates

```rust
if session.storage.has_chunk(chunk.chunk_index) {
    return Ok(Json(json!({
        "success": true,
        "duplicate": true,
        "chunk": chunk.chunk_index,
    })));
}
```

**Idempotent uploads:**
- Browser might retry chunk (network issue)
- Server checks if already have it
- Return success immediately (no re-processing)

### Step 5: Store Chunk

```rust
let session_key = EncryptionKey::from_base64(&state.session_key)?;
let file_nonce = Nonce::from_base64(&session.nonce)?;

session
    .storage
    .store_chunk(chunk.chunk_index, chunk.data, &session_key, &file_nonce)
    .await?;
```

**What happens:**
- Parse encryption key from base64
- Parse nonce from base64
- Call `storage.store_chunk()`
  - Small files: Insert into HashMap
  - Large files: Decrypt and write to disk
- `await?` = wait for completion, propagate errors

### Step 6: Return Success

```rust
Ok(Json(json!({
    "success": true,
    "chunk": chunk.chunk_index,
    "total": session.total_chunks,
    "received": session.storage.chunks_count(),
})))
```

- `json!` macro creates JSON value
- Wrapped in `Json()` (Axum response type)
- Browser gets response with progress info

---

## Send Chunk Handler: Download Flow

### The Handler

```rust
pub async fn send_chunk_handler(
    Path((token, file_index, chunk_index)): Path<(String, usize, usize)>,
    State(state): State<AppState>,
) -> Result<Response<Body>, AppError> {
```

**Extracting multiple path parameters:**
```rust
Path((token, file_index, chunk_index)): Path<(String, usize, usize)>
```
- URL: `/send/abc123/0/42`
- Extracts: `token="abc123"`, `file_index=0`, `chunk_index=42`
- Tuple destructuring

### Step 1: Validate and Get File

```rust
if !state.session.is_valid(&token).await {
    return Err(anyhow::anyhow!("Invalid token").into());
}

let file_entry = state
    .session
    .get_file(file_index)
    .ok_or_else(|| anyhow::anyhow!("Invalid file index"))?;
```

- Validate session token
- Get file metadata from manifest
- `ok_or_else()` converts `Option` to `Result`

### Step 2: Calculate Chunk Boundaries

```rust
const CHUNK_SIZE: u64 = 64 * 1024;

let start = chunk_index as u64 * CHUNK_SIZE;
let end = std::cmp::min(start + CHUNK_SIZE, file_entry.size);

if start >= file_entry.size {
    return Err(anyhow::anyhow!("Chunk index out of bounds").into());
}
```

**Example:**
```
File size: 200KB
Chunk size: 64KB

Chunk 0: start=0, end=65536
Chunk 1: start=65536, end=131072
Chunk 2: start=131072, end=196608
Chunk 3: start=196608, end=204800 (last chunk, smaller)
```

**`std::cmp::min()`:**
- Last chunk might be smaller than 64KB
- `min(start + 64KB, file_size)` ensures we don't read past end

### Step 3: Read Chunk from Disk

```rust
let mut file = tokio::fs::File::open(&file_entry.full_path).await?;
file.seek(SeekFrom::Start(start)).await?;

let chunk_len = (end - start) as usize;
let mut buffer = vec![0u8; chunk_len];
file.read_exact(&mut buffer).await?;
```

**Step by step:**

1. **Open file:**
   ```rust
   let mut file = tokio::fs::File::open(&file_entry.full_path).await?;
   ```
   - Async file open
   - Read-only mode

2. **Seek to position:**
   ```rust
   file.seek(SeekFrom::Start(start)).await?;
   ```
   - Move file cursor to byte offset
   - `SeekFrom::Start(n)` = absolute position
   - Async operation

3. **Read exact amount:**
   ```rust
   let mut buffer = vec![0u8; chunk_len];
   file.read_exact(&mut buffer).await?;
   ```
   - `vec![0u8; chunk_len]` allocates buffer (zeroed)
   - `read_exact()` reads EXACTLY chunk_len bytes
   - Fails if can't read enough (EOF, error)

### Step 4: Encrypt Chunk

```rust
let session_key = EncryptionKey::from_base64(state.session.session_key())?;
let file_nonce = Nonce::from_base64(&file_entry.nonce)?;

let encrypted = encrypt_chunk_at_position(
    &session_key,
    &file_nonce,
    &buffer,
    chunk_index as u32,
)?;
```

**Stateless encryption:**
- Don't need to encrypt chunks 0..N-1 first
- Nonce constructed from base + counter
- Counter = chunk_index
- Each chunk independently encryptable

### Step 5: Return Response

```rust
Ok(Response::builder()
    .header(header::CONTENT_TYPE, "application/octet-stream")
    .body(Body::from(encrypted))?)
```

- Build HTTP response
- Set content type (binary data)
- Body contains encrypted bytes
- `?` converts error to AppError

---

## Manifest Hash Calculation

### The Function

```rust
async fn calculate_file_hash(path: &Path) -> Result<String> {
    const CHUNK_SIZE: usize = 64 * 1024;

    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; CHUNK_SIZE];

    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}
```

**Why loop?**
- File might be huge (TB)
- Can't load entire file into memory
- Read in chunks, update hash incrementally

**How it works:**

1. **Read chunk:**
   ```rust
   let n = file.read(&mut buffer).await?;
   ```
   - Reads up to CHUNK_SIZE bytes
   - Returns number of bytes actually read
   - Might be less than buffer size (last chunk, EOF)

2. **Check EOF:**
   ```rust
   if n == 0 {
       break;
   }
   ```
   - Zero bytes read = end of file
   - Exit loop

3. **Update hash:**
   ```rust
   hasher.update(&buffer[..n]);
   ```
   - `&buffer[..n]` = slice of first n bytes
   - Don't include unused buffer space
   - SHA-256 state updated

4. **Finalize:**
   ```rust
   Ok(hex::encode(hasher.finalize()))
   ```
   - After loop, `finalize()` produces final hash
   - Convert bytes to hex string

---

## Browser Download: Client-Side

### Small File Download

```javascript
async function downloadSmallFile(token, fileEntry, key, nonceBase, totalChunks) {
    const decryptedChunks = [];

    for (let i = 0; i < totalChunks; i++) {
        const encrypted = await downloadChunkWithRetry(token, fileEntry.index, i);

        const nonce = generateNonce(nonceBase, i);
        const decrypted = await crypto.subtle.decrypt(
            { name: 'AES-GCM', iv: nonce },
            key,
            encrypted
        );

        decryptedChunks.push(new Uint8Array(decrypted));
    }

    // Verify hash
    const blob = new Blob(decryptedChunks);
    const arrayBuffer = await blob.arrayBuffer();
    const hashBuffer = await crypto.subtle.digest('SHA-256', arrayBuffer);
    const computedHash = arrayBufferToHex(hashBuffer);

    if (computedHash !== fileEntry.sha256) {
        throw new Error('Integrity check failed!');
    }

    // Trigger download
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = fileEntry.name;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
}
```

**Step by step:**

1. **Download all chunks:**
   - Loop through all chunks
   - `downloadChunkWithRetry()` handles network errors
   - Decrypt each chunk with Web Crypto API
   - Store in array (all in memory)

2. **Create blob:**
   ```javascript
   const blob = new Blob(decryptedChunks);
   ```
   - Blob = immutable binary data
   - Browser stores in memory

3. **Verify hash:**
   ```javascript
   const hashBuffer = await crypto.subtle.digest('SHA-256', arrayBuffer);
   ```
   - Web Crypto API (native browser crypto)
   - Fast (hardware accelerated)
   - Returns ArrayBuffer with hash bytes

4. **Trigger download:**
   ```javascript
   const url = URL.createObjectURL(blob);
   ```
   - Creates `blob://` URL (browser internal)
   - Create link, click it, remove it
   - Browser saves to Downloads folder

### Large File Download (File System Access API)

```javascript
async function downloadLargeFile(token, fileEntry, key, nonceBase, totalChunks) {
    const fileHandle = await window.showSaveFilePicker({
        suggestedName: fileEntry.name,
    });

    const writable = await fileHandle.createWritable();

    try {
        const decryptedChunks = [];

        for (let i = 0; i < totalChunks; i++) {
            const encrypted = await downloadChunkWithRetry(token, fileEntry.index, i);

            const nonce = generateNonce(nonceBase, i);
            const decrypted = await crypto.subtle.decrypt(
                { name: 'AES-GCM', iv: nonce },
                key,
                encrypted
            );

            const decryptedArray = new Uint8Array(decrypted);

            // Write to disk immediately
            await writable.write(decryptedArray);

            // Keep for hash verification
            decryptedChunks.push(decryptedArray);
        }

        await writable.close();

        // Verify hash
        const blob = new Blob(decryptedChunks);
        const arrayBuffer = await blob.arrayBuffer();
        const hashBuffer = await crypto.subtle.digest('SHA-256', arrayBuffer);
        const computedHash = arrayBufferToHex(hashBuffer);

        if (computedHash !== fileEntry.sha256) {
            throw new Error('Integrity check failed!');
        }
    } catch (error) {
        await writable.abort();  // Delete partial file
        throw error;
    }
}
```

**Key differences:**

1. **Ask where to save:**
   ```javascript
   const fileHandle = await window.showSaveFilePicker({
       suggestedName: fileEntry.name,
   });
   ```
   - Shows native "Save As" dialog
   - User picks location
   - Returns FileSystemFileHandle

2. **Create writable stream:**
   ```javascript
   const writable = await fileHandle.createWritable();
   ```
   - Stream that writes directly to disk
   - Not buffered in memory

3. **Write chunks as they arrive:**
   ```javascript
   await writable.write(decryptedArray);
   ```
   - Each chunk written immediately
   - Memory usage stays constant (only current chunk)
   - Can handle TB files

4. **Still collect for hash:**
   ```javascript
   decryptedChunks.push(decryptedArray);
   ```
   - Need all data for hash verification
   - This is a limitation: must hold decrypted data
   - Could be improved with streaming hash

5. **Cleanup on error:**
   ```javascript
   await writable.abort();
   ```
   - Deletes partial file
   - Browser handles cleanup

### Retry Logic

```javascript
async function downloadChunkWithRetry(token, fileIndex, chunkIndex, maxRetries = 3) {
    for (let attempt = 0; attempt < maxRetries; attempt++) {
        try {
            const response = await fetch(`/send/${token}/${fileIndex}/chunk/${chunkIndex}`);
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}`);
            }
            return await response.arrayBuffer();
        } catch (e) {
            if (attempt === maxRetries - 1) {
                throw new Error(`Failed after ${maxRetries} attempts: ${e.message}`);
            }
            const delay = 1000 * Math.pow(2, attempt);  // 1s, 2s, 4s
            await new Promise(r => setTimeout(r, delay));
        }
    }
}
```

**Exponential backoff:**
- Attempt 0: Wait 1 second (2^0 * 1000ms)
- Attempt 1: Wait 2 seconds (2^1 * 1000ms)
- Attempt 2: Wait 4 seconds (2^2 * 1000ms)
- Prevents hammering server

---

## Rust Concepts Explained

### 1. Ownership and Borrowing

```rust
pub async fn finalize(
    mut self,              // Takes ownership (consumes)
    dest_path: &Path,      // Borrows (doesn't take ownership)
    key: &EncryptionKey,   // Borrows
```

**Ownership:**
- `self` (not `&self`) = function takes ownership
- Can't use `self` after calling this function
- Allows moving fields out of struct

**Borrowing:**
- `&Path` = immutable reference
- Function can read, can't modify
- Original owner still valid after call

### 2. Result and Option

```rust
pub async fn new(...) -> Result<Self> {
    if file_size < THRESHOLD {
        Ok(ChunkStorage::Memory { ... })
    } else {
        let file = File::create(&path).await?;
        Ok(ChunkStorage::DirectWrite { ... })
    }
}
```

**Result<T, E>:**
- `Ok(value)` = success with value
- `Err(error)` = failure with error
- Must handle both cases

**The `?` operator:**
```rust
let file = File::create(&path).await?;
```
Expands to:
```rust
let file = match File::create(&path).await {
    Ok(f) => f,
    Err(e) => return Err(e.into()),
};
```
- If error, early return
- Automatically converts error types

**Option<T>:**
```rust
pub struct PartialFileGuard {
    path: Option<PathBuf>,
}
```
- `Some(value)` = has value
- `None` = no value
- Rust's null safety (no null pointers)

### 3. Async/Await

```rust
pub async fn store_chunk(...) -> Result<()> {
    let decrypted = decrypt_chunk(...)?;

    output_file.write_all(&decrypted).await?;
    //                                ^^^^^
    //                             Suspension point
}
```

**How async works:**
- `async fn` returns `Future<Output = Result<()>>`
- `.await` suspends execution (non-blocking)
- Other tasks can run while waiting for I/O
- No OS threads blocked

### 4. Pattern Matching

```rust
match self {
    ChunkStorage::Memory { chunks } => {
        // chunks is HashMap
    }
    ChunkStorage::DirectWrite { output_file, hasher, .. } => {
        // output_file is File, hasher is Sha256
    }
}
```

**Exhaustive:**
- Must handle all enum variants
- Compiler error if missing case
- Prevents bugs

**Destructuring:**
- Pulls fields out of variant
- Can rename, ignore with `..`

### 5. Traits

**Drop trait:**
```rust
impl Drop for PartialFileGuard {
    fn drop(&mut self) {
        // Called when value goes out of scope
    }
}
```

**Like interfaces:**
- Define behavior
- Types implement traits
- Generic code uses trait bounds

### 6. Smart Pointers

**Arc (Atomic Reference Counted):**
```rust
pub struct AppState {
    pub upload_sessions: Arc<RwLock<HashMap<...>>>,
}
```
- Thread-safe shared ownership
- Cloning increments count
- Dropped when count reaches zero

**RwLock (Read-Write Lock):**
```rust
let mut sessions = state.upload_sessions.write().await;
```
- Multiple readers OR one writer
- `.write()` = exclusive access
- `.read()` = shared access
- Async aware (doesn't block thread)

### 7. Lifetimes (Implicit)

```rust
pub fn get_file(&self, index: usize) -> Option<&FileEntry> {
    //           ^^^^^                          ^^^^^^^^^^
    //           Input lifetime                 Output lifetime
}
```

**Elided (implicit) lifetime:**
```rust
// Compiler sees this as:
pub fn get_file<'a>(&'a self, index: usize) -> Option<&'a FileEntry>
```
- Output reference lives as long as `self`
- Can't outlive the struct it came from
- Prevents dangling pointers

---

## Summary

**ChunkStorage:**
- Enum with two variants (Memory, DirectWrite)
- Automatic threshold-based switching
- Memory: store encrypted, decrypt on finalize
- DirectWrite: decrypt immediately, write to disk

**PartialFileGuard:**
- RAII cleanup using Drop trait
- Deletes file if dropped while armed
- Disarm on success

**Receive Handler:**
- Parse chunk from multipart
- Get or create upload session
- Store chunk (memory or disk)
- Return success with progress

**Send Chunk Handler:**
- Calculate chunk boundaries
- Read from disk at offset
- Encrypt chunk
- Return encrypted bytes

**Browser:**
- Small files: download to memory, verify hash, blob download
- Large files: File System Access API, stream to disk, verify hash
- Retry logic with exponential backoff

**Rust Concepts:**
- Ownership prevents memory errors
- Result/Option for error handling
- Async/await for non-blocking I/O
- Pattern matching for control flow
- Traits for behavior (Drop)
- Smart pointers for sharing (Arc, RwLock)
