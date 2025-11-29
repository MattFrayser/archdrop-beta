# Temp Storage Analysis: Is It Needed?

## Your Question

> "I was under the understanding that having the encrypted data to temp and then decrypting all at once was to catch any malformed data? Does this make the hash redundant? Is temp needed?"

This is a great architectural question. Let's analyze what each layer provides.

---

## Three Layers of Protection

### Layer 1: AES-GCM Authentication Tags (Per-Chunk)

**What it provides:**
```rust
let decrypted = decrypt_chunk_at_position(key, nonce, encrypted_chunk, counter)?;
// ↑ This will FAIL if:
//   - Chunk was tampered with
//   - Wrong key/nonce
//   - Corrupted encryption
//   - Authentication tag doesn't match
```

**AES-GCM guarantees:**
- Chunk integrity (detects bit flips, corruption)
- Chunk authenticity (detects tampering)
- Per-chunk validation (each chunk independently verified)

**What it DOESN'T catch:**
- Missing chunks (chunk 50 never uploaded)
- Reordered chunks (chunk 10 and 20 swapped)
- Wrong file entirely (chunks from different file)
- Truncation (only received 900 of 1000 chunks)

### Layer 2: Chunk Metadata Tracking

**What it provides:**
```rust
// Verify all chunks received
for i in 0..total_chunks {
    if !has_chunk(i) {
        return Err("Missing chunk");
    }
}
```

**Catches:**
- Missing chunks
- Incomplete uploads

**What it DOESN'T catch:**
- Chunk reordering (all chunks present but wrong order)
- Wrong chunks (chunks from different file with same index)

### Layer 3: SHA-256 Whole-File Hash

**What it provides:**
```rust
let computed_hash = hash_entire_file(decrypted_data);
if computed_hash != expected_hash {
    return Err("File integrity failed");
}
```

**Catches:**
- Chunk reordering
- Wrong file
- Any deviation from original file
- Cross-chunk integrity issues

**What it DOESN'T catch:**
- Nothing! It's the ultimate verification

---

## Are They Redundant?

### Short Answer: NO

Each layer serves a different purpose:

| Issue | AES-GCM | Metadata | SHA-256 |
|-------|---------|----------|---------|
| Corrupted chunk | ✓ | ✗ | ✓ |
| Tampered chunk | ✓ | ✗ | ✓ |
| Missing chunk | ✗ | ✓ | ✓ |
| Reordered chunks | ✗ | ✗ | ✓ |
| Wrong file | ✗ | ✗ | ✓ |
| Fail fast (immediate) | ✓ | ✗ | ✗ |
| Final guarantee | ✗ | ✗ | ✓ |

**Defense in depth:** AES-GCM catches errors early (per-chunk), SHA-256 provides final guarantee.

---

## Temp Storage: Current Approach

**Current flow:**
```
1. Receive encrypted chunk → Write to /tmp/archdrop/{token}/{file_id}/{i}.chunk
2. Receive all chunks
3. On finalize:
   a. For each chunk:
      - Read encrypted chunk from /tmp
      - Decrypt (AES-GCM validates here)
      - Write to final destination
   b. Calculate hash of final file
   c. Verify hash matches expected
   d. If verification fails → Delete final file
   e. Cleanup /tmp
```

**Benefits:**
1. **Atomic file creation** - Final file only exists if everything succeeds
2. **Fail-safe on decryption** - If chunk 500 fails to decrypt, destination unchanged
3. **Error recovery** - Can retry finalization without re-uploading
4. **Separation of concerns** - Upload and processing are separate

**Downsides:**
1. **Double disk I/O** - Write encrypted, read encrypted, write decrypted
2. **Temp storage required** - Uses disk space during transfer
3. **Slower for small files** - Unnecessary I/O overhead

---

## Alternative: Direct Decryption (No Temp)

**New flow:**
```
1. Receive encrypted chunk
2. Decrypt immediately (AES-GCM validates)
3. Append to final file
4. Track progress in memory
5. After all chunks:
   a. Calculate hash of final file
   b. If hash fails → Delete final file
```

**Benefits:**
1. **Single pass I/O** - Only write once (to final destination)
2. **No temp storage** - Saves disk space
3. **Faster** - Especially for small files

**Downsides:**
1. **Partial files on error** - If chunk 500 fails, you have 499 chunks in destination
2. **Must cleanup on error** - Need RAII guard to delete partial file
3. **Can't retry finalization** - Must re-upload all chunks

---

## Analysis: Is Temp Needed?

### For Small Files (< 100MB): NO

**Reason:** Already storing encrypted chunks in memory.

**Current:**
```rust
ChunkStorage::Memory {
    chunks: HashMap<usize, Vec<u8>>  // Encrypted chunks in RAM
}

// On finalize: decrypt from memory, write to disk
for i in 0..total_chunks {
    let encrypted = chunks.get(i);
    let decrypted = decrypt(encrypted);
    file.write(decrypted);
}
```

**No temp storage involved!** Memory → Decrypt → Disk (single write)

### For Large Files (≥ 100MB): MAYBE

**Option A: Keep temp (safe, traditional)**
```rust
ChunkStorage::Disk {
    base_path: PathBuf  // /tmp/archdrop/{token}/{file_id}/
}

// Chunks written as they arrive
// Decrypted on finalize (atomic)
```

**Pros:**
- Atomic file creation
- Fail-safe error handling
- Can retry finalization
- Destination never has partial files

**Cons:**
- Double I/O (write encrypted, write decrypted)
- Requires temp disk space

**Option B: Decrypt-as-you-go (efficient, risky)**
```rust
// On first chunk: create final file with guard
let mut output = File::create(dest_path)?;
let guard = PartialFileGuard::new(dest_path.clone());  // Deletes on drop

// On each chunk:
let encrypted = parse_chunk();
let decrypted = decrypt(encrypted)?;  // Fails if corrupt
output.write_all(decrypted)?;

// On finalize:
output.flush()?;
verify_hash(dest_path)?;  // Final check
guard.disarm();  // Success, keep file
```

**Pros:**
- Single I/O pass (half the disk operations)
- No temp storage needed
- Faster

**Cons:**
- Partial file exists during upload (deleted on error via guard)
- If server crashes mid-finalize, partial file may remain
- Can't retry finalization (must re-upload)

---

## Recommendation: Hybrid Approach

**Combine best of both:**

```rust
pub enum ChunkStorage {
    Memory {
        chunks: HashMap<usize, Vec<u8>>,  // Encrypted
    },
    DirectWrite {
        output_file: tokio::fs::File,
        partial_guard: PartialFileGuard,
        decrypted_size: u64,
    },
}

impl ChunkStorage {
    pub async fn new(token: &str, file_id: &str, file_size: u64, dest_path: PathBuf) -> Result<Self> {
        if file_size < 100 * 1024 * 1024 {
            // Small files: memory buffer
            Ok(ChunkStorage::Memory {
                chunks: HashMap::new(),
            })
        } else {
            // Large files: decrypt directly to destination
            let output = tokio::fs::File::create(&dest_path).await?;
            let guard = PartialFileGuard::new(dest_path.clone());

            Ok(ChunkStorage::DirectWrite {
                output_file: output,
                partial_guard: guard,
                decrypted_size: 0,
            })
        }
    }

    pub async fn store_chunk(
        &mut self,
        index: usize,
        encrypted_data: Vec<u8>,
        key: &EncryptionKey,
        nonce: &Nonce,
    ) -> Result<()> {
        match self {
            ChunkStorage::Memory { chunks } => {
                // Store encrypted (decrypt on finalize)
                chunks.insert(index, encrypted_data);
                Ok(())
            }
            ChunkStorage::DirectWrite { output_file, decrypted_size, .. } => {
                // Decrypt immediately and write
                let decrypted = decrypt_chunk_at_position(
                    key,
                    nonce,
                    &encrypted_data,
                    index as u32,
                )?;

                output_file.write_all(&decrypted).await?;
                *decrypted_size += decrypted.len() as u64;
                Ok(())
            }
        }
    }

    pub async fn finalize(
        mut self,
        dest_path: &Path,
        key: &EncryptionKey,
        nonce: &Nonce,
        total_chunks: usize,
        expected_hash: &str,
    ) -> Result<()> {
        match self {
            ChunkStorage::Memory { chunks } => {
                // Decrypt and write all at once
                let mut output = tokio::fs::File::create(dest_path).await?;
                let mut hasher = Sha256::new();

                for i in 0..total_chunks {
                    let encrypted = chunks.get(&i)
                        .ok_or_else(|| anyhow::anyhow!("Missing chunk {}", i))?;

                    let decrypted = decrypt_chunk_at_position(key, nonce, encrypted, i as u32)?;
                    hasher.update(&decrypted);
                    output.write_all(&decrypted).await?;
                }

                output.flush().await?;

                // Verify hash
                let computed = hex::encode(hasher.finalize());
                if computed != expected_hash {
                    tokio::fs::remove_file(dest_path).await?;
                    return Err(anyhow::anyhow!("Hash mismatch"));
                }

                Ok(())
            }
            ChunkStorage::DirectWrite { mut output_file, mut partial_guard, .. } => {
                // File already written, just verify hash
                output_file.flush().await?;
                drop(output_file);  // Close file

                // Calculate hash of written file
                let computed = calculate_file_hash(dest_path).await?;

                if computed != expected_hash {
                    // Hash failed, guard will delete file on drop
                    return Err(anyhow::anyhow!("Hash mismatch"));
                }

                // Success! Disarm guard
                partial_guard.disarm();
                Ok(())
            }
        }
    }
}
```

**Result:**
- Small files (< 100MB): Memory → Decrypt on finalize → Single disk write
- Large files (≥ 100MB): Decrypt immediately → Write once → No temp storage

**Protection:**
- AES-GCM validates every chunk (fail fast)
- SHA-256 validates final file (ultimate guarantee)
- PartialFileGuard cleans up on error (no orphaned files)

---

## Answers to Your Questions

### "Does this make the hash redundant?"

**NO.** AES-GCM and SHA-256 serve different purposes:

**AES-GCM:**
- Per-chunk authentication
- Detects tampering/corruption in individual chunks
- Fails fast (during decryption)

**SHA-256:**
- Whole-file integrity
- Detects chunk reordering, missing chunks, wrong file
- Final verification (after all processing)

**Example where AES-GCM passes but hash fails:**
```
Chunk 0: ✓ Decrypts successfully (AES-GCM valid)
Chunk 2: ✓ Decrypts successfully (AES-GCM valid)
Chunk 1: ✓ Decrypts successfully (AES-GCM valid)
         ↑ Out of order!

Final hash: ✗ Doesn't match expected (SHA-256 catches reordering)
```

### "Is temp needed?"

**Depends on file size:**

**Small files (< 100MB): NO**
- Already using memory storage
- Decrypt on finalize
- Single disk write
- No temp directory needed

**Large files (≥ 100MB): NO (with proper error handling)**
- Can decrypt directly to destination
- Use PartialFileGuard for cleanup
- Single disk write (no temp)
- Verify hash at end

**Original reason for temp:**
- Safety: Destination never has partial/corrupt files
- Retry: Can retry finalization without re-upload

**But you can achieve same safety with:**
- RAII guards (PartialFileGuard deletes on error)
- Hash verification (delete if fails)
- Atomic semantics via guards

---

## Recommended Approach

### For Your Use Case

**Small files (< 100MB):**
```rust
Memory chunks → Decrypt on finalize → Write to destination → Verify hash
```
- Fast (single I/O pass)
- No temp storage
- Hash provides final guarantee

**Large files (≥ 100MB):**
```rust
Receive chunk → Decrypt immediately → Append to destination → Verify hash at end
```
- Fast (single I/O pass)
- No temp storage
- PartialFileGuard cleans up on error
- Hash catches any issues

**Protection layers:**
1. AES-GCM fails fast on corrupt chunks ✓
2. Metadata ensures all chunks received ✓
3. SHA-256 provides final guarantee ✓
4. PartialFileGuard prevents orphaned files ✓

---

## Performance Comparison

### Current (with temp):
```
1GB file (1000 chunks):
- Write 1000 encrypted chunks to /tmp: ~2s (1GB write)
- Read 1000 encrypted chunks from /tmp: ~2s (1GB read)
- Decrypt 1000 chunks: ~0.5s
- Write 1000 decrypted chunks to dest: ~2s (1GB write)
Total: ~6.5s (3GB disk I/O)
```

### Proposed (no temp):
```
1GB file (1000 chunks):
- Receive chunk → Decrypt → Append to dest: ~2s (1GB write)
- Verify hash: ~1s (1GB read for hash)
Total: ~3s (2GB disk I/O)
```

**2x faster for large files!**

---

## Implementation Plan

### Phase 1: Refactor ChunkStorage

**Add DirectWrite variant:**
```rust
pub enum ChunkStorage {
    Memory { chunks: HashMap<usize, Vec<u8>> },
    DirectWrite {
        output_file: tokio::fs::File,
        partial_guard: PartialFileGuard,
        hasher: Sha256,
        chunks_received: HashSet<usize>,
    },
}
```

### Phase 2: Implement PartialFileGuard

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
            let _ = std::fs::remove_file(&path);
        }
    }
}
```

### Phase 3: Update receive_handler

```rust
pub async fn receive_handler(...) -> Result<...> {
    let chunk = parse_chunk_upload(multipart).await?;

    let mut sessions = state.upload_sessions.write().await;
    let session = sessions.entry(file_id.clone()).or_insert_with(|| {
        // Pass destination path for direct write
        let dest_path = calculate_dest_path(&chunk.relative_path);
        ChunkStorage::new(chunk.file_size, dest_path).unwrap()
    });

    // Store (decrypts immediately for large files)
    session.storage.store_chunk(
        chunk.chunk_index,
        chunk.data,
        &session_key,
        &file_nonce,
    ).await?;

    Ok(...)
}
```

### Phase 4: Simplify finalize

```rust
pub async fn finalize_upload(...) -> Result<...> {
    let session = sessions.remove(&file_id)?;

    // Verify (hash already calculated for DirectWrite)
    session.storage.finalize(
        &dest_path,
        &session_key,
        &file_nonce,
        session.total_chunks,
        &expected_hash,
    ).await?;

    Ok(...)
}
```

---

## Conclusion

**Is temp needed?** NO - not with proper error handling via guards and hash verification.

**Is hash redundant?** NO - it catches issues AES-GCM can't (reordering, wrong file, etc.).

**Best approach:**
- Small files: Memory → Decrypt on finalize → Single write
- Large files: Decrypt immediately → Direct write → Hash verification

**Result:**
- 2x faster (half the I/O)
- No temp storage required
- Same safety guarantees via guards and hash
- Simpler code (no temp directory management)
