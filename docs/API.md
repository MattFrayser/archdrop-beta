# ArchDrop API Documentation

Complete HTTP API reference for the ArchDrop file transfer server.

## Table of Contents

1. [Overview](#overview)
2. [Authentication](#authentication)
3. [Send Mode API](#send-mode-api)
4. [Receive Mode API](#receive-mode-api)
5. [Error Handling](#error-handling)
6. [Examples](#examples)

---

## Overview

ArchDrop exposes a minimal HTTP API that browsers use to transfer files. The API is designed around two modes:

- **Send Mode**: Server streams encrypted file chunks to browser
- **Receive Mode**: Browser uploads encrypted chunks to server

### Base URL Structure

```
Local Mode:  https://127.0.0.1:{PORT}
Tunnel Mode: https://{RANDOM}.trycloudflare.com
```

### URL Fragment Pattern

All transfer URLs include encryption keys in the fragment (after `#`):

```
{BASE_URL}/{MODE}/{TOKEN}#key={KEY}&nonce={NONCE}
```

**Example**:
```
https://127.0.0.1:54321/send/a1b2c3d4#key=YWJjZGVmZw&nonce=MTIzNDU2Nw
                        └────┬────┘     └─────┬─────┘  └─────┬─────┘
                          Token          Key (Base64)   Nonce (Base64)
```

**Important**: The fragment (`#...`) is never sent to the server, providing zero-knowledge encryption.

---

## Authentication

### Session Tokens

Each transfer session has a unique UUID v4 token:

```
Token Format: {UUID}
Example:      "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
```

**Properties**:
- 128-bit random UUID v4
- Single-use (atomic claiming mechanism)
- Ephemeral (valid only during server process lifetime)
- No expiration time (session ends on completion or server shutdown)

### Token Claiming

The first request to access a session "claims" it atomically:

```rust
// Send Mode: Claim on first chunk request
GET /send/:token/0/chunk/0
  → If unclaimed: Claim succeeds, transfer begins
  → If claimed:   Claim fails, 500 error returned

// Receive Mode: Claim on first chunk upload
POST /receive/:token/chunk
  → If unclaimed: Claim succeeds, upload begins
  → If claimed:   Claim fails, 500 error returned
```

After claiming, subsequent requests must use the same token and session must be active.

---

## Send Mode API

Send mode serves encrypted file chunks to the browser for download.

### 1. Health Check

**Endpoint**: `GET /health`

**Description**: Server readiness check.

**Request**:
```http
GET /health HTTP/1.1
Host: 127.0.0.1:54321
```

**Response**:
```http
HTTP/1.1 200 OK
Content-Type: text/plain
Content-Length: 2

OK
```

**Status Codes**:
- `200 OK`: Server is ready

**Notes**:
- Used internally during server startup
- No authentication required
- Useful for monitoring/health checks

---

### 2. Get Manifest

**Endpoint**: `GET /send/:token/manifest`

**Description**: Retrieve list of files available for download.

**Request**:
```http
GET /send/a1b2c3d4-e5f6-7890-abcd-ef1234567890/manifest HTTP/1.1
Host: 127.0.0.1:54321
```

**Response**:
```http
HTTP/1.1 200 OK
Content-Type: application/json

{
  "files": [
    {
      "index": 0,
      "name": "document.pdf",
      "relative_path": "documents/document.pdf",
      "size": 1048576,
      "nonce": "YWJjZGVm"
    },
    {
      "index": 1,
      "name": "image.jpg",
      "relative_path": "images/image.jpg",
      "size": 524288,
      "nonce": "Z2hpamts"
    }
  ]
}
```

**Response Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `files` | array | List of file entries |
| `files[].index` | number | File index (0-based) |
| `files[].name` | string | File name only |
| `files[].relative_path` | string | Relative path from base directory |
| `files[].size` | number | File size in bytes |
| `files[].nonce` | string | Per-file nonce (Base64, 7 bytes) |

**Status Codes**:
- `200 OK`: Manifest returned successfully
- `500 Internal Server Error`: Invalid token or session error

**Notes**:
- Manifest does not include full filesystem paths (security)
- Each file has a unique nonce for encryption
- `full_path` field exists in Rust but is skipped during serialization

---

### 3. Download Chunk

**Endpoint**: `GET /send/:token/:file_index/chunk/:chunk_index`

**Description**: Download a specific encrypted chunk of a file.

**Path Parameters**:

| Parameter | Type | Description |
|-----------|------|-------------|
| `token` | string | Session token (UUID) |
| `file_index` | number | File index from manifest (0-based) |
| `chunk_index` | number | Chunk index within file (0-based) |

**Request**:
```http
GET /send/a1b2c3d4-e5f6-7890-abcd-ef1234567890/0/chunk/5 HTTP/1.1
Host: 127.0.0.1:54321
```

**Response**:
```http
HTTP/1.1 200 OK
Content-Type: application/octet-stream
Content-Length: 1048592

<encrypted binary data>
```

**Response Body**:
- Raw encrypted bytes (AES-256-GCM ciphertext + 16-byte auth tag)
- Chunk size: up to 1MB plaintext + 16 bytes overhead
- Last chunk may be smaller

**Chunk Calculation**:
```
Chunk size: 1MB (1048576 bytes)

Chunk 0: bytes 0..1048576
Chunk 1: bytes 1048576..2097152
Chunk N: bytes N*1048576..min((N+1)*1048576, file_size)
```

**Status Codes**:
- `200 OK`: Chunk returned successfully
- `500 Internal Server Error`:
  - Invalid token
  - Invalid file_index or chunk_index
  - Session not active
  - Session already claimed by another client
  - File I/O error
  - Encryption error

**Special Behavior**:
- **First chunk** (`file_index=0`, `chunk_index=0`): Claims the session
- **Subsequent chunks**: Validates session is active

**Notes**:
- Chunks can be requested in any order (random access)
- Browser typically requests multiple chunks in parallel
- Server uses buffered I/O with 2MB buffer for performance

---

### 4. Complete Download

**Endpoint**: `POST /send/:token/complete`

**Description**: Mark the transfer as complete.

**Request**:
```http
POST /send/a1b2c3d4-e5f6-7890-abcd-ef1234567890/complete HTTP/1.1
Host: 127.0.0.1:54321
```

**Response**:
```http
HTTP/1.1 200 OK
Content-Type: application/json

{
  "success": true,
  "message": "Download complete"
}
```

**Status Codes**:
- `200 OK`: Transfer marked complete
- `500 Internal Server Error`: Invalid or inactive token

**Side Effects**:
- Server marks session as completed
- Progress channel updated to 100%
- TUI exits
- Server shuts down gracefully

---

### 5. Serve Download Page

**Endpoint**: `GET /send/:token`

**Description**: Serve the HTML/JS download client.

**Request**:
```http
GET /send/a1b2c3d4-e5f6-7890-abcd-ef1234567890 HTTP/1.1
Host: 127.0.0.1:54321
```

**Response**:
```http
HTTP/1.1 200 OK
Content-Type: text/html

<!DOCTYPE html>
<html>
  <head>...</head>
  <body>
    <!-- Download UI -->
    <script src="/download.js"></script>
    <script src="/shared.js"></script>
  </body>
</html>
```

**Status Codes**:
- `200 OK`: Page served successfully

**Associated Static Assets**:
- `GET /download.js`: Client-side download logic
- `GET /shared.js`: Shared crypto utilities
- `GET /styles.css`: Styling

---

## Receive Mode API

Receive mode accepts encrypted file chunks uploaded from the browser.

### 1. Upload Chunk

**Endpoint**: `POST /receive/:token/chunk`

**Description**: Upload an encrypted file chunk.

**Request**:
```http
POST /receive/a1b2c3d4-e5f6-7890-abcd-ef1234567890/chunk HTTP/1.1
Host: 127.0.0.1:54321
Content-Type: multipart/form-data; boundary=----WebKitFormBoundary

------WebKitFormBoundary
Content-Disposition: form-data; name="chunk"; filename="blob"
Content-Type: application/octet-stream

<encrypted binary data>
------WebKitFormBoundary
Content-Disposition: form-data; name="fileName"

document.pdf
------WebKitFormBoundary
Content-Disposition: form-data; name="relativePath"

documents/document.pdf
------WebKitFormBoundary
Content-Disposition: form-data; name="chunkIndex"

0
------WebKitFormBoundary
Content-Disposition: form-data; name="totalChunks"

10
------WebKitFormBoundary
Content-Disposition: form-data; name="fileSize"

10485760
------WebKitFormBoundary
Content-Disposition: form-data; name="nonce"

YWJjZGVm
------WebKitFormBoundary--
```

**Multipart Form Fields**:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `chunk` | binary | Yes | Encrypted chunk data |
| `fileName` | string | Yes | File name only |
| `relativePath` | string | Yes | Relative path for file placement |
| `chunkIndex` | number | Yes | Chunk index (0-based) |
| `totalChunks` | number | Yes | Total expected chunks for this file |
| `fileSize` | number | Yes | Total file size in bytes |
| `nonce` | string | Conditional | Per-file nonce (Base64), required for chunk 0 |

**Response**:
```http
HTTP/1.1 200 OK
Content-Type: application/json

{
  "success": true,
  "chunk": 0,
  "total": 10,
  "received": 1
}
```

**Response Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `success` | boolean | Always `true` on success |
| `chunk` | number | Chunk index that was stored |
| `total` | number | Total expected chunks |
| `received` | number | Number of chunks received so far |
| `duplicate` | boolean | (Optional) `true` if chunk was already received |

**Status Codes**:
- `200 OK`: Chunk accepted (may be duplicate)
- `500 Internal Server Error`:
  - Invalid token
  - Malformed multipart data
  - Missing required fields
  - Invalid path (traversal attempt)
  - Decryption failure
  - I/O error

**Duplicate Handling**:

If a chunk is uploaded twice (e.g., network retry):

```json
{
  "success": true,
  "duplicate": true,
  "chunk": 5,
  "received": 6,
  "total": 10
}
```

**Session Creation**:
- First chunk of a new file creates a `ReceiveSession`
- File identified by `hash(relativePath)`
- Session tracks storage, nonce, total chunks, file size

**Decryption Process**:
1. Extract encrypted chunk from multipart data
2. Retrieve cipher from session
3. Decrypt using AES-GCM with per-file nonce + chunk counter
4. Verify authentication tag
5. Seek to chunk position in file
6. Write decrypted data

**Notes**:
- Chunks can arrive in any order
- Server uses `HashSet` to track received chunks
- Duplicate detection prevents double-writes
- Path validation prevents directory traversal

---

### 2. Finalize File

**Endpoint**: `POST /receive/:token/finalize`

**Description**: Finalize a file after all chunks uploaded.

**Request**:
```http
POST /receive/a1b2c3d4-e5f6-7890-abcd-ef1234567890/finalize HTTP/1.1
Host: 127.0.0.1:54321
Content-Type: multipart/form-data; boundary=----WebKitFormBoundary

------WebKitFormBoundary
Content-Disposition: form-data; name="relativePath"

documents/document.pdf
------WebKitFormBoundary--
```

**Multipart Form Fields**:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `relativePath` | string | Yes | Path of file to finalize |

**Response**:
```http
HTTP/1.1 200 OK
Content-Type: application/json

{
  "success": true,
  "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
}
```

**Response Fields**:

| Field | Type | Description |
|-------|------|-------------|
| `success` | boolean | Always `true` on success |
| `sha256` | string | SHA-256 hash of complete file (hex) |

**Status Codes**:
- `200 OK`: File finalized successfully
- `500 Internal Server Error`:
  - Invalid token
  - File session not found
  - Incomplete upload (missing chunks)
  - I/O error during hashing

**Finalization Process**:
1. Verify session exists for file
2. Check all chunks received: `received_count == total_chunks`
3. Flush file to disk
4. Compute SHA-256 hash of entire file
5. Disarm RAII guard (prevent file deletion)
6. Remove session from map
7. Return hash for client verification

**Important**:
- Must be called after all chunks uploaded
- Failure to finalize leaves partial file that will be deleted
- Hash can be used by client to verify integrity

---

### 3. Complete Transfer

**Endpoint**: `POST /receive/:token/complete`

**Description**: Mark all file uploads as complete.

**Request**:
```http
POST /receive/a1b2c3d4-e5f6-7890-abcd-ef1234567890/complete HTTP/1.1
Host: 127.0.0.1:54321
```

**Response**:
```http
HTTP/1.1 200 OK
Content-Type: application/json

{
  "success": true,
  "message": "Transfer complete"
}
```

**Status Codes**:
- `200 OK`: Transfer marked complete
- `500 Internal Server Error`: Invalid token

**Side Effects**:
- Server marks session as completed
- Progress channel updated to 100%
- TUI exits
- Server shuts down gracefully

---

### 4. Serve Upload Page

**Endpoint**: `GET /receive/:token`

**Description**: Serve the HTML/JS upload client.

**Request**:
```http
GET /receive/a1b2c3d4-e5f6-7890-abcd-ef1234567890 HTTP/1.1
Host: 127.0.0.1:54321
```

**Response**:
```http
HTTP/1.1 200 OK
Content-Type: text/html

<!DOCTYPE html>
<html>
  <head>...</head>
  <body>
    <!-- Upload UI -->
    <script src="/upload.js"></script>
    <script src="/shared.js"></script>
  </body>
</html>
```

**Associated Static Assets**:
- `GET /upload.js`: Client-side upload logic
- `GET /shared.js`: Shared crypto utilities
- `GET /styles.css`: Styling

---

## Error Handling

### Error Response Format

All endpoints return generic 500 errors on failure:

```http
HTTP/1.1 500 Internal Server Error
Content-Length: 0
```

**Rationale**:
- Security: Don't leak internal error details
- Simplicity: Single error code simplifies client logic

**Error Logging**:
- Detailed errors logged to server stderr
- Use `eprintln!` or `tracing` (if enabled)
- Client receives generic error

### Common Error Scenarios

**Invalid Token**:
```
Cause:   Token doesn't match session token
Example: GET /send/wrong-token/manifest
Result:  500 Internal Server Error
```

**Session Not Active**:
```
Cause:   Session hasn't been claimed or is already completed
Example: GET /send/:token/0/chunk/0 (second client)
Result:  500 Internal Server Error
```

**Invalid Indices**:
```
Cause:   file_index or chunk_index out of bounds
Example: GET /send/:token/999/chunk/0
Result:  500 Internal Server Error
```

**Path Traversal Attempt**:
```
Cause:   relativePath contains ".." or absolute path
Example: POST /receive/:token/chunk (path="../../../etc/passwd")
Result:  500 Internal Server Error
```

**Decryption Failure**:
```
Cause:   Wrong key, corrupted data, or wrong nonce
Result:  500 Internal Server Error
Log:     "Decryption failed at counter X: ..."
```

**I/O Errors**:
```
Cause:   File not found, permission denied, disk full
Result:  500 Internal Server Error
```

### Client-Side Error Handling

Browser clients should:

1. **Retry Transient Errors**: Network issues, temporary server unavailability
2. **Abort on Persistent Errors**: Invalid token, decryption failure
3. **Show User-Friendly Messages**: Generic error rather than technical details

**Example Retry Logic** (JavaScript):
```javascript
async function downloadChunk(fileIndex, chunkIndex, maxRetries = 3) {
  for (let attempt = 0; attempt < maxRetries; attempt++) {
    try {
      const response = await fetch(`/send/${token}/${fileIndex}/chunk/${chunkIndex}`);
      if (!response.ok) throw new Error('HTTP error');
      return await response.arrayBuffer();
    } catch (error) {
      if (attempt === maxRetries - 1) throw error;
      await sleep(1000 * Math.pow(2, attempt)); // Exponential backoff
    }
  }
}
```

---

## Examples

### Complete Send Mode Flow

```bash
# 1. Server started
archdrop send file.txt --local
# Output: https://127.0.0.1:54321/send/abc123#key=XYZ&nonce=ABC
```

```javascript
// 2. Browser extracts keys from URL fragment
const fragment = window.location.hash.substring(1);
const params = new URLSearchParams(fragment);
const key = await importKey(params.get('key'));
const nonce = params.get('nonce');

// 3. Fetch manifest
const response = await fetch('/send/abc123/manifest');
const manifest = await response.json();

// 4. Download chunks in parallel
for (const file of manifest.files) {
  const totalChunks = Math.ceil(file.size / CHUNK_SIZE);
  const promises = [];

  for (let i = 0; i < totalChunks; i++) {
    promises.push(downloadAndDecryptChunk(file.index, i, key, file.nonce));
  }

  const chunks = await Promise.all(promises);
  const blob = new Blob(chunks);
  saveFile(blob, file.name);
}

// 5. Notify completion
await fetch('/send/abc123/complete', { method: 'POST' });
```

### Complete Receive Mode Flow

```bash
# 1. Server started
archdrop receive ~/Downloads --local
# Output: https://127.0.0.1:54321/receive/def456#key=XYZ&nonce=ABC
```

```javascript
// 2. Browser extracts keys
const fragment = window.location.hash.substring(1);
const params = new URLSearchParams(fragment);
const key = await importKey(params.get('key'));

// 3. User selects files
const input = document.createElement('input');
input.type = 'file';
input.multiple = true;
const files = await selectFiles(input);

// 4. Upload each file
for (const file of files) {
  const fileNonce = generateNonce(); // Random per-file nonce
  const totalChunks = Math.ceil(file.size / CHUNK_SIZE);

  // Upload chunks in parallel
  const promises = [];
  for (let i = 0; i < totalChunks; i++) {
    promises.push(encryptAndUploadChunk(file, i, totalChunks, key, fileNonce));
  }
  await Promise.all(promises);

  // Finalize file
  await fetch('/receive/def456/finalize', {
    method: 'POST',
    body: createFormData({ relativePath: file.name })
  });
}

// 5. Complete transfer
await fetch('/receive/def456/complete', { method: 'POST' });
```

### Curl Examples

**Health Check**:
```bash
curl -k https://127.0.0.1:54321/health
# OK
```

**Get Manifest**:
```bash
TOKEN="a1b2c3d4-e5f6-7890-abcd-ef1234567890"
curl -k "https://127.0.0.1:54321/send/${TOKEN}/manifest"
# {"files":[{"index":0,"name":"file.txt",...}]}
```

**Download Chunk**:
```bash
TOKEN="a1b2c3d4-e5f6-7890-abcd-ef1234567890"
curl -k "https://127.0.0.1:54321/send/${TOKEN}/0/chunk/0" -o chunk.enc
# Saves encrypted chunk to chunk.enc
```

**Upload Chunk** (complex multipart):
```bash
TOKEN="a1b2c3d4-e5f6-7890-abcd-ef1234567890"
curl -k "https://127.0.0.1:54321/receive/${TOKEN}/chunk" \
  -F "chunk=@chunk.enc" \
  -F "fileName=file.txt" \
  -F "relativePath=file.txt" \
  -F "chunkIndex=0" \
  -F "totalChunks=1" \
  -F "fileSize=1024" \
  -F "nonce=YWJjZGVm"
# {"success":true,"chunk":0,"total":1,"received":1}
```

---

## Rate Limiting

**Current Implementation**: None

ArchDrop does not implement rate limiting because:
1. Single-use sessions naturally limit abuse
2. Ephemeral servers (process lifetime only)
3. Not designed for public/untrusted networks

**Recommendation for Production**:
- Add rate limiting middleware (e.g., `tower-http`)
- Limit requests per IP address
- Implement connection limits

---

## CORS Policy

**Current Implementation**: No CORS headers

Browser same-origin policy applies:
- Browser must navigate directly to the URL
- Cannot call API from different origin (e.g., malicious site)

**Security Benefit**: Prevents cross-site request forgery (CSRF)

---

## Versioning

**Current Version**: None (v0.1.0 implied)

API is currently unversioned. Future versions may:
- Use URL path versioning: `/v2/send/:token/...`
- Use custom header: `X-API-Version: 2`
- Maintain backward compatibility

---

## Performance Considerations

### Optimal Client Behavior

**Parallel Requests**:
- Send Mode: Download 4-8 chunks in parallel
- Receive Mode: Upload 4-8 chunks in parallel
- Adjust based on network conditions

**Request Ordering**:
- Sequential: Maximizes cache efficiency
- Random: Better load distribution
- Adaptive: Start sequential, fan out to parallel

**Backpressure**:
- Monitor network buffer sizes
- Slow down if upload queue is full
- Avoid memory exhaustion from queued chunks

### Server Performance

**Benchmarks** (approximate, hardware-dependent):
- Throughput: ~500-800 Mbps (local mode, single file)
- Latency: <10ms per chunk (local mode)
- Memory: ~10MB base + ~4MB per concurrent request
- CPU: ~20% per core for encryption

**Bottlenecks**:
- Encryption: CPU-bound
- Disk I/O: For HDDs (random writes in receive mode)
- Network: For tunnel mode (limited by upstream bandwidth)

---

## Security Considerations

1. **TLS**: Always use HTTPS in production
2. **Certificate Validation**: Local mode uses self-signed certs (users must accept)
3. **Token Secrecy**: Tokens provide authentication; keep URLs private
4. **Fragment Security**: Keys in fragment never reach server logs
5. **Input Validation**: All paths validated; no directory traversal possible

See [SECURITY.md](SECURITY.md) for comprehensive security analysis.

---

## Troubleshooting API Issues

**Problem**: 500 errors on all requests

**Solutions**:
1. Check token matches exactly (including dashes)
2. Verify session not already completed
3. Check server logs for detailed error
4. Ensure files haven't been moved/deleted

**Problem**: Decryption failures

**Solutions**:
1. Verify key and nonce extracted from URL fragment
2. Check chunk counter matches chunk index
3. Ensure no data corruption during transfer
4. Verify file nonce used (not session nonce)

**Problem**: Slow performance

**Solutions**:
1. Increase parallel chunk requests
2. Use local mode instead of tunnel
3. Check disk I/O (especially HDDs)
4. Monitor CPU usage (encryption bottleneck)

For more troubleshooting, see [TROUBLESHOOTING.md](TROUBLESHOOTING.md).
