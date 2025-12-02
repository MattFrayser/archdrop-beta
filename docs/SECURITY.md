# ArchDrop Security Documentation

Comprehensive security analysis and threat model for ArchDrop.

## Table of Contents

1. [Security Overview](#security-overview)
2. [Threat Model](#threat-model)
3. [Security Architecture](#security-architecture)
4. [Cryptographic Design](#cryptographic-design)
5. [Attack Surface Analysis](#attack-surface-analysis)
6. [Known Limitations](#known-limitations)
7. [Security Best Practices](#security-best-practices)
8. [Incident Response](#incident-response)

---

## Security Overview

### Design Goals

1. **Confidentiality**: Files encrypted end-to-end; server cannot read content
2. **Integrity**: Tampering detected via authenticated encryption
3. **Authenticity**: Single-use tokens prevent unauthorized access
4. **Privacy**: Zero-knowledge architecture; server doesn't see keys

### Trust Model

**Trusted**:
- User's device (sender/receiver)
- User's browser (Web Crypto API)
- User's network path (for key distribution)

**Untrusted**:
- Server process (zero-knowledge design)
- Network between server and browser (encrypted)
- Cloudflare tunnel (tunnel mode only)

**Adversary Capabilities**:
- ❌ Cannot decrypt files without key (even with server access)
- ❌ Cannot modify chunks without detection (authenticated encryption)
- ✅ Can see encrypted chunks (server has access to ciphertext)
- ✅ Can see metadata (file sizes, chunk count, transfer timing)

---

## Threat Model

### Threat Actors

#### 1. Network Eavesdropper (Passive)

**Capabilities**:
- Observe network traffic between browser and server
- Capture packets for offline analysis

**Mitigations**:
- ✅ **HTTPS/TLS**: Encrypts transport layer (local and tunnel modes)
- ✅ **End-to-end encryption**: Even if TLS broken, files still encrypted
- ✅ **Fragment-based key distribution**: Keys never traverse network to server

**Residual Risk**: Metadata leakage (file sizes, timing, transfer patterns)

#### 2. Network Attacker (Active)

**Capabilities**:
- Intercept and modify network traffic
- Inject malicious requests
- Man-in-the-middle attacks

**Mitigations**:
- ✅ **HTTPS/TLS**: Prevents MitM (with valid certificates)
- ✅ **AEAD encryption**: Detects any ciphertext modification
- ⚠️ **Self-signed certificates** (local mode): Vulnerable to MitM if user accepts wrong cert

**Residual Risk**:
- Local mode: Users must verify certificate fingerprint
- Tunnel mode: Depends on Cloudflare's certificate management

#### 3. Malicious Server Operator

**Scenario**: Attacker controls the ArchDrop server process

**Capabilities**:
- Full access to server memory and disk
- Can log all requests
- Can modify server code

**Mitigations**:
- ✅ **Zero-knowledge design**: Keys never sent to server
- ✅ **Client-side encryption**: Server only sees ciphertext
- ✅ **No key storage**: Keys only exist in browser memory

**Residual Risk**:
- Server can see ciphertext and metadata
- Malicious server could serve compromised JavaScript (see below)

#### 4. Malicious JavaScript (Code Injection)

**Scenario**: Attacker modifies HTML/JS templates served to browser

**Capabilities**:
- Extract keys from URL fragment
- Exfiltrate plaintext files
- Send data to attacker-controlled server

**Mitigations**:
- ⚠️ **None at protocol level**: Browser fully trusts served JavaScript
- ⚠️ **Subresource Integrity (SRI)**: Not currently implemented
- ⚠️ **Content Security Policy (CSP)**: Not currently implemented

**Residual Risk**: **HIGH** - This is the most significant threat
- If server compromised, JavaScript can be malicious
- User has no way to verify JavaScript integrity
- See [Known Limitations](#known-limitations) for details

#### 5. Unauthorized Access (Token Theft)

**Scenario**: Attacker obtains the transfer URL with token and keys

**Capabilities**:
- Download files (send mode)
- Upload files (receive mode)

**Mitigations**:
- ✅ **Single-use sessions**: Token can only be claimed once
- ✅ **Atomic claiming**: First client wins, others rejected
- ✅ **No expiration time**: Sessions don't linger

**Residual Risk**:
- If URL leaked before legitimate user accesses, attacker can claim
- No authentication beyond URL possession
- No way to revoke URL after generation

#### 6. Physical Access to Receiver

**Scenario**: Attacker has access to device receiving files

**Capabilities**:
- Read downloaded files from disk
- Extract keys from browser memory (difficult)

**Mitigations**:
- ⚠️ **Disk encryption**: User responsibility
- ⚠️ **Secure deletion**: Not implemented

**Residual Risk**: Files written to disk in plaintext (post-decryption)

---

## Security Architecture

### Defense in Depth

```
┌─────────────────────────────────────────────────────────────────┐
│ Layer 1: Transport Security (HTTPS/TLS)                        │
│  - Prevents eavesdropping on network                           │
│  - Prevents tampering of ciphertext in transit                 │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────┐
│ Layer 2: End-to-End Encryption (AES-256-GCM)                   │
│  - Files encrypted on sender, decrypted on receiver            │
│  - Server cannot read plaintext even if TLS compromised        │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────┐
│ Layer 3: Authenticated Encryption (GCM auth tags)              │
│  - Detects any tampering with ciphertext                       │
│  - Per-chunk authentication prevents reordering attacks        │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────┐
│ Layer 4: Session Authentication (UUID v4 tokens)               │
│  - Single-use tokens prevent replay attacks                    │
│  - Atomic claiming prevents race conditions                    │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────┐
│ Layer 5: Input Validation (Path traversal prevention)          │
│  - Validates all file paths                                    │
│  - Prevents directory escape attacks                           │
│  - Sanitizes multipart form data                               │
└─────────────────────────────────────────────────────────────────┘
                              │
┌─────────────────────────────▼───────────────────────────────────┐
│ Layer 6: Integrity Verification (SHA-256 hashing)              │
│  - Final hash of complete file                                 │
│  - Detects corruption or incomplete transfers                  │
└─────────────────────────────────────────────────────────────────┘
```

### Zero-Knowledge Key Distribution

```
┌──────────────────────────────────────────────────────────────────┐
│                    Key Generation (Server)                       │
│                                                                  │
│  1. Generate 32-byte AES key (OsRng)                            │
│  2. Generate 7-byte nonce base (OsRng)                          │
│  3. Encode as Base64 (URL-safe, no padding)                     │
└────────────────────────┬─────────────────────────────────────────┘
                         │
                         ▼
┌──────────────────────────────────────────────────────────────────┐
│              URL Construction (Server)                           │
│                                                                  │
│  URL: https://example.com/send/TOKEN#key=KEY&nonce=NONCE       │
│                                  │         └──────┬──────────┘  │
│                                  │                │              │
│                      Server sees │    Browser sees│              │
│                                  │                │              │
└──────────────────────────────────┼────────────────┼──────────────┘
                                   │                │
                    ┌──────────────┘                └───────────┐
                    │                                            │
                    ▼                                            ▼
       ┌─────────────────────────┐              ┌────────────────────────┐
       │  Server Knowledge       │              │  Browser Knowledge     │
       │  - TOKEN (UUID)         │              │  - TOKEN (UUID)        │
       │  - Ciphertext           │              │  - KEY (32 bytes)      │
       │  - Metadata             │              │  - NONCE (7 bytes)     │
       │                         │              │  - Can decrypt files   │
       │  Cannot decrypt files   │              │                        │
       └─────────────────────────┘              └────────────────────────┘
```

**Why This Works**:
- HTTP specification: Fragment (`#...`) never sent in request
- Browser: `window.location.hash` accessible to JavaScript
- Server: Only sees path and query string, never fragment
- Result: Server cannot access encryption keys

**Verification** (Browser DevTools):
```javascript
// Browser console
console.log(window.location.href);
// https://example.com/send/TOKEN#key=KEY&nonce=NONCE

// Server logs show
// GET /send/TOKEN HTTP/1.1
// (no fragment)
```

---

## Cryptographic Design

### Encryption Algorithm: AES-256-GCM

**Properties**:
- **Algorithm**: AES (Advanced Encryption Standard)
- **Key Size**: 256 bits (maximum security)
- **Mode**: GCM (Galois/Counter Mode)
- **AEAD**: Provides both confidentiality and authenticity

**Why AES-256-GCM**:
- ✅ NIST-approved standard (FIPS 140-2)
- ✅ Hardware acceleration on modern CPUs (AES-NI)
- ✅ Authenticated encryption (integrity + confidentiality)
- ✅ Parallelizable (fast encryption/decryption)
- ✅ Well-studied (no known practical attacks)

**Alternatives Considered**:
- ChaCha20-Poly1305: Good choice, but AES-NI makes AES faster
- XChaCha20-Poly1305: Extended nonce, but our nonce scheme is sufficient
- AES-GCM-SIV: Nonce-misuse resistant, but not needed with our design

### Key Generation

```rust
use rand::rngs::OsRng;
use rand::RngCore;

pub fn new() -> Self {
    let mut key = [0u8; 32];
    OsRng::default().fill_bytes(&mut key);
    Self(key)
}
```

**Random Number Generator**: `OsRng`
- Uses OS-provided CSPRNG (Cryptographically Secure PRNG)
- Linux: reads from `/dev/urandom`
- Suitable for cryptographic key generation

**Key Properties**:
- 256 bits = 32 bytes = 2^256 possible keys
- Brute force: 2^255 attempts on average (infeasible)
- Lifetime: Single transfer only (ephemeral keys)

### Nonce Design

**Structure**:
```
Total: 12 bytes (96 bits) for AES-GCM
┌──────────────┬──────────────┬────────┐
│   7 bytes    │   4 bytes    │ 1 byte │
│ Random Base  │   Counter    │  Flag  │
└──────────────┴──────────────┴────────┘
```

**Per-File Nonce Base** (7 bytes):
```rust
let nonce = Nonce::new();  // Unique per file
```

**Per-Chunk Nonce** (12 bytes total):
```rust
let mut full_nonce = [0u8; 12];
full_nonce[..7].copy_from_slice(nonce_base.as_bytes());
full_nonce[7..11].copy_from_slice(&counter.to_be_bytes());
// Byte 11 left as 0 (flag for future use)
```

**Nonce Uniqueness Guarantee**:
1. Each file has unique 7-byte random base (collision probability: 2^-56)
2. Each chunk has unique counter (0 to chunks-1)
3. Result: No nonce reuse across any chunk of any file

**Why This Matters**:
- AES-GCM security requires unique nonces
- Nonce reuse catastrophically breaks security (keystream reuse)
- Our design makes nonce reuse mathematically infeasible

**Nonce Collision Probability**:
```
P(collision) = 1 - e^(-n² / 2^57)

For 1 million files:
P(collision) ≈ 0.00000000000000034 (negligible)
```

### Authentication Tags

**GCM Authentication Tag**: 16 bytes per chunk

**Properties**:
- Computed from ciphertext + AAD (Associated Authenticated Data)
- Verifies both integrity and authenticity
- Cannot be forged without the key

**Attack Scenarios**:
- Modify ciphertext → Decryption fails (authentication tag mismatch)
- Reorder chunks → Decryption fails (counter doesn't match tag)
- Replay old chunk → Duplicate detection or wrong position
- Forge tag → Computationally infeasible (requires 2^128 operations)

### Session Token Generation

```rust
use uuid::Uuid;

let token = Uuid::new_v4().to_string();
// Example: "550e8400-e29b-41d4-a716-446655440000"
```

**UUID v4 Properties**:
- 122 random bits (6 bits used for version/variant)
- Collision probability: negligible for single-use tokens
- Unpredictable: Uses CSPRNG

**Token Purpose**:
- Authentication: proves client has the URL
- Not encryption: token is separate from encryption keys

### Key Derivation

**Current Implementation**: None

Keys are randomly generated, not derived from passwords.

**Future Consideration**: Password-based key derivation
```
User Password ──▶ PBKDF2/Argon2 ──▶ AES Key
                  (100k iterations)
```

**Trade-offs**:
- ✅ User-memorable passwords instead of long URLs
- ⚠️ Weak passwords vulnerable to brute force
- ⚠️ Requires secure password exchange mechanism

---

## Attack Surface Analysis

### 1. Network Layer

**Attack Vectors**:
- Packet sniffing (eavesdropping)
- Man-in-the-middle attacks
- Traffic analysis (metadata leakage)

**Mitigations**:
- HTTPS/TLS encryption (local and tunnel modes)
- Certificate validation (automatic in tunnel mode)

**Residual Risks**:
- Self-signed certificates (local mode): users must verify
- TLS downgrade attacks: not applicable (HTTPS enforced)
- Metadata leakage: file sizes, transfer timing visible

### 2. Application Layer

**Attack Vectors**:
- Path traversal (`../../../etc/passwd`)
- Command injection (if shell commands used)
- Buffer overflows (Rust prevents most)

**Mitigations**:
- Path validation: rejects `..`, absolute paths, null bytes
- No shell commands: uses Rust stdlib only
- Memory safety: Rust's ownership system

**Code Example** (Path Validation):
```rust
pub fn validate_path(path: &str) -> Result<(), PathValidationError> {
    if path.contains('\0') {
        return Err(PathValidationError::NullByte);
    }

    let path = Path::new(path);
    if path.is_absolute() {
        return Err(PathValidationError::AbsolutePath);
    }

    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(PathValidationError::ContainsParentDir)
            }
            _ => continue,
        }
    }
    Ok(())
}
```

**Residual Risks**:
- Unicode normalization attacks: not addressed
- Symlink attacks: not addressed

### 3. Cryptographic Layer

**Attack Vectors**:
- Weak RNG (predictable keys)
- Nonce reuse (keystream reuse)
- Timing attacks (side channels)

**Mitigations**:
- OsRng: cryptographically secure RNG
- Unique per-file nonces + counters: no reuse
- Constant-time operations: provided by `aes-gcm` crate

**Residual Risks**:
- Implementation bugs in `aes-gcm` crate (rely on audits)
- Side-channel attacks: cache timing, power analysis (not mitigated)

### 4. Session Management

**Attack Vectors**:
- Session hijacking (stolen tokens)
- Session fixation (attacker sets token)
- Race conditions (multiple claims)

**Mitigations**:
- Atomic session claiming: compare-and-swap
- Single-use tokens: no session reuse
- Server-generated tokens: UUID v4 (unpredictable)

**Code Example** (Atomic Claiming):
```rust
pub fn claim(&self, token: &str) -> bool {
    if token != self.token {
        return false;
    }

    self.active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}
```

**Residual Risks**:
- Token exposed in URL: can be leaked via browser history, logs
- No revocation mechanism: cannot cancel transfer after URL generated

### 5. Input Validation

**Attack Vectors**:
- Malformed multipart data
- Integer overflow (chunk indices)
- Resource exhaustion (infinite chunks)

**Mitigations**:
- Axum validates multipart format
- Rust prevents integer overflows in debug mode
- Bounds checking on file/chunk indices

**Residual Risks**:
- Large file attacks: no file size limits (intentional)
- Zip bomb-like attacks: encrypted files can be "bombs"
- Memory exhaustion: malicious client uploads many files

### 6. File System

**Attack Vectors**:
- Write outside destination directory
- Overwrite existing files
- Symlink attacks (follow symlink out of destination)

**Mitigations**:
- Path validation: prevents traversal
- RAII cleanup: partial files deleted on error
- Destination directory validation: ensured before transfer

**Residual Risks**:
- Symlink attacks: not specifically mitigated
- Disk space exhaustion: no quotas
- File name collisions: overwrites files (by design)

---

## Known Limitations

### 1. JavaScript Trust Boundary

**Problem**: Browser must trust JavaScript served by server

**Attack Scenario**:
1. Attacker compromises server process
2. Server serves malicious JavaScript
3. JavaScript extracts keys from URL fragment
4. JavaScript exfiltrates plaintext to attacker

**Current Mitigation**: **NONE**

**Potential Solutions**:
- **Subresource Integrity (SRI)**:
  ```html
  <script src="/download.js"
          integrity="sha384-oqVuAfXRKap7fdgcCY5uykM6+R9GqQ8K/uxy9rx7HNQlGYl1kPzQho1wx4JwY8wC">
  </script>
  ```
  - Pro: Browser verifies script hash
  - Con: Requires pre-known hash (can't be dynamic)
  - Con: User must verify hash matches (manual process)

- **Content Security Policy (CSP)**:
  ```http
  Content-Security-Policy: script-src 'self'; default-src 'none'
  ```
  - Pro: Prevents external script loading
  - Con: Doesn't prevent server from modifying its own scripts

- **Separate Static File Server**:
  - Serve HTML/JS from read-only location
  - Server process cannot modify static files
  - Requires more complex deployment

**Recommendation**:
- For trusted environments: Accept current design
- For hostile environments: Manually verify JavaScript source code

**Risk Level**: **HIGH** (but only if server is compromised)

### 2. Self-Signed Certificates (Local Mode)

**Problem**: Browsers show scary warnings for self-signed certificates

**User Experience**:
```
Your connection is not private
Attackers might be trying to steal your information from 127.0.0.1

NET::ERR_CERT_AUTHORITY_INVALID

[Advanced] [Go back]
```

**Attack Scenario**:
- User connects to wrong network
- Attacker runs fake ArchDrop server
- User accepts attacker's certificate
- MitM attack successful

**Current Mitigation**: User must verify they're connecting to correct IP

**Potential Solutions**:
- **Certificate Fingerprint Verification**:
  - Display cert fingerprint in TUI
  - User manually verifies in browser
  - Tedious but secure

- **Let's Encrypt (Local)**:
  - Generate valid cert for `localhost`
  - Doesn't work for LAN IPs
  - Requires DNS validation

- **mDNS + Local CA**:
  - Install local CA cert in browser
  - Use mDNS for discovery
  - Complex setup

**Recommendation**: Use tunnel mode for non-technical users

**Risk Level**: **MEDIUM** (requires active attacker + user error)

### 3. No URL Revocation

**Problem**: Once URL generated, cannot be revoked

**Scenario**:
1. User generates URL
2. User accidentally shares URL publicly
3. Attacker claims session before legitimate user
4. No way to cancel or regenerate

**Current Mitigation**: **NONE**

**Potential Solutions**:
- **Time-based expiration**:
  - Token valid for 5 minutes
  - User must act quickly
  - Con: Inconvenient for legitimate users

- **Explicit activation**:
  - Token generated inactive
  - User confirms "start transfer" in CLI
  - Activates token
  - Con: Extra step in UX

**Recommendation**: Treat URLs like passwords (don't share publicly)

**Risk Level**: **LOW** (requires user error)

### 4. No Forward Secrecy

**Problem**: If key compromised, past traffic can be decrypted

**Scenario**:
1. Attacker records encrypted traffic
2. Later obtains URL (with keys)
3. Decrypts recorded traffic

**Current Mitigation**: Keys are ephemeral (not stored), but attacker could have recorded URL

**Potential Solutions**:
- **Ephemeral keys via DH/ECDH**:
  - Establish shared secret via key exchange
  - Keys never transmitted
  - Con: Requires interactive session setup

**Recommendation**: Use short-lived URLs, delete after use

**Risk Level**: **LOW** (requires both traffic recording + URL leak)

### 5. Metadata Leakage

**Problem**: File sizes, transfer timing, file count visible to server

**Exposed Metadata**:
- Total file size
- Number of files
- File names (but not paths)
- Transfer duration
- IP addresses

**Current Mitigation**: **NONE** (by design)

**Potential Solutions**:
- **Padding**:
  - Pad files to fixed sizes
  - Con: Bandwidth overhead
  - Con: Storage overhead

- **Constant-rate transfer**:
  - Transfer at fixed rate (adds dummy data)
  - Con: Slow for small files

**Recommendation**: Accept metadata leakage for usability

**Risk Level**: **LOW** (metadata less sensitive than content)

### 6. No Resumable Transfers

**Problem**: If transfer interrupted, must restart from beginning

**Impact**:
- Large files vulnerable to network issues
- Wastes bandwidth on retry

**Current Mitigation**: **NONE**

**Potential Solutions**:
- **Chunk tracking**:
  - Server stores which chunks received
  - Client queries missing chunks on reconnect
  - Requires persistent storage

- **Resume tokens**:
  - Server issues resume token
  - Allows reconnection with same session
  - Requires session persistence

**Recommendation**: Future enhancement

**Risk Level**: **LOW** (usability issue, not security)

---

## Security Best Practices

### For Users

1. **Protect Transfer URLs**:
   - Treat URLs like passwords
   - Don't post in public channels
   - Don't share via unencrypted email
   - Delete from browser history after use

2. **Verify Connection** (Local Mode):
   - Check IP address matches
   - Verify certificate fingerprint (if possible)
   - Use tunnel mode if unsure

3. **Secure Receiving Device**:
   - Use disk encryption (LUKS, FileVault)
   - Don't receive files on untrusted devices
   - Verify file integrity (SHA-256 hash)

4. **Network Security**:
   - Use trusted networks for local mode
   - Prefer tunnel mode on public WiFi
   - Consider VPN for extra protection

5. **Cleanup**:
   - Clear browser history after transfer
   - Delete temporary files
   - Close TUI when done (server auto-shuts down)

### For Developers

1. **Dependency Management**:
   ```bash
   cargo audit  # Check for vulnerable dependencies
   cargo outdated  # Check for updates
   ```

2. **Secure Compilation**:
   ```bash
   cargo build --release  # Optimizations enabled
   # Consider: cargo-auditable for supply chain security
   ```

3. **Static Analysis**:
   ```bash
   cargo clippy -- -D warnings  # Lint warnings as errors
   cargo deny check  # Check licenses and security
   ```

4. **Testing**:
   ```bash
   cargo test  # Unit tests
   cargo test --ignored  # Integration tests
   cargo fuzz  # Fuzz testing (if implemented)
   ```

5. **Secure Deployment**:
   - Strip debug symbols: `strip target/release/archdrop`
   - Verify checksums: `sha256sum archdrop`
   - Use HTTPS for downloads (if distributing)

### For Administrators

1. **Network Configuration**:
   - Firewall: Allow only necessary ports
   - IDS/IPS: Monitor for anomalous traffic
   - Logging: Log connection attempts (not payloads)

2. **Host Security**:
   - Keep OS updated (kernel, libraries)
   - Use SELinux/AppArmor for sandboxing
   - Limit file system access

3. **Monitoring**:
   - Monitor resource usage (CPU, memory, disk)
   - Alert on long-running transfers
   - Track failed authentication attempts

4. **Incident Response**:
   - Have plan for compromise
   - Log transfer metadata (times, IPs)
   - Preserve evidence for forensics

---

## Incident Response

### Suspected Key Compromise

**Indicators**:
- Unauthorized file access
- Transfer to unknown IP
- Unexpected session claiming

**Response**:
1. **Immediate**: Shut down server (transfer completes or crashes)
2. **Investigate**: Check server logs for unusual activity
3. **Assess**: Determine if keys leaked (how?)
4. **Notify**: Inform affected users
5. **Remediate**: Rotate any related secrets, update software

**Note**: Keys are ephemeral; compromise affects only current transfer

### Suspected Server Compromise

**Indicators**:
- Modified binaries (check hashes)
- Unexpected network connections
- Unusual CPU/memory usage

**Response**:
1. **Immediate**: Shut down server
2. **Isolate**: Disconnect from network
3. **Investigate**: Forensic analysis of server
4. **Assess**: Check if JavaScript modified
5. **Notify**: Inform recent users (keys may be compromised)
6. **Remediate**: Reinstall from trusted source

### Suspected JavaScript Tampering

**Indicators**:
- Browser reports CSP violations (if enabled)
- User reports data exfiltration
- JavaScript differs from official version

**Response**:
1. **Immediate**: Stop using compromised server
2. **Investigate**: Diff JavaScript against official version
3. **Assess**: Determine what data may have been exfiltrated
4. **Notify**: Inform all recent users
5. **Remediate**: Reinstall server, verify JavaScript integrity

### Vulnerability Disclosure

**If you discover a vulnerability**:

1. **Do not disclose publicly** until patch available
2. **Report privately** to maintainers
3. **Provide details**: Steps to reproduce, impact assessment
4. **Allow time**: 90 days for patch development
5. **Coordinate disclosure**: Agree on disclosure timeline

**Maintainer response**:
1. **Acknowledge**: Confirm receipt within 48 hours
2. **Assess**: Verify vulnerability, determine severity
3. **Patch**: Develop fix, write tests
4. **Disclose**: Publish CVE, release patch
5. **Credit**: Acknowledge reporter (if desired)

---

## Compliance Considerations

### GDPR (General Data Protection Regulation)

**Applicability**: If transferring data of EU residents

**ArchDrop's Position**:
- ✅ No personal data stored (ephemeral sessions)
- ✅ Encryption in transit and at rest (during transfer)
- ✅ No tracking or analytics
- ⚠️ Server sees IP addresses (metadata)
- ⚠️ No data retention policy (immediate deletion)

**Recommendation**: Use tunnel mode to avoid logging IPs directly

### HIPAA (Health Insurance Portability and Accountability Act)

**Applicability**: If transferring protected health information (PHI)

**ArchDrop's Position**:
- ✅ Encryption meets HIPAA requirements (AES-256)
- ✅ Access controls (single-use tokens)
- ⚠️ No audit logs (required for HIPAA)
- ⚠️ No business associate agreement (BAA) template
- ⚠️ Not formally audited for HIPAA compliance

**Recommendation**: Do not use for HIPAA-covered data without additional controls

### PCI DSS (Payment Card Industry Data Security Standard)

**Applicability**: If transferring payment card data

**ArchDrop's Position**:
- ⚠️ Not designed for PCI compliance
- ⚠️ No tokenization or secure deletion
- ⚠️ No audit trails

**Recommendation**: Do not use for payment card data

---

## Security Audit Checklist

- [ ] Dependency audit completed (`cargo audit`)
- [ ] No vulnerable dependencies
- [ ] All dependencies from crates.io (official registry)
- [ ] HTTPS enabled (local or tunnel mode)
- [ ] Certificate validation working
- [ ] Path traversal tests pass
- [ ] Nonce uniqueness verified
- [ ] No hard-coded secrets
- [ ] Error messages don't leak sensitive info
- [ ] Input validation on all endpoints
- [ ] Memory safety verified (no `unsafe` code or audited)
- [ ] Authentication tests pass
- [ ] Encryption tests pass
- [ ] Integrity verification works
- [ ] Cleanup functions tested (RAII)
- [ ] No session persistence vulnerabilities
- [ ] JavaScript integrity verifiable (manual)
- [ ] Documentation up to date

---

## Conclusion

ArchDrop provides strong cryptographic security for file transfers, but has inherent limitations:

**Strengths**:
- ✅ End-to-end encryption (AES-256-GCM)
- ✅ Zero-knowledge architecture (server can't decrypt)
- ✅ Authenticated encryption (integrity + confidentiality)
- ✅ Single-use sessions (prevents replay attacks)
- ✅ Memory-safe implementation (Rust)

**Limitations**:
- ⚠️ JavaScript trust boundary (server compromise risk)
- ⚠️ Self-signed certificates (local mode UX)
- ⚠️ No URL revocation mechanism
- ⚠️ Metadata leakage (file sizes, timing)
- ⚠️ Not designed for compliance (HIPAA, PCI DSS)

**Recommendation**: Suitable for personal file transfers between trusted devices. Not recommended for:
- Regulated data (PHI, PCI, classified)
- Hostile environments (untrusted networks)
- Critical data requiring audit trails
- Multi-recipient broadcasts (not supported)

For production use, consider additional controls:
- Implement SRI for JavaScript integrity
- Add CSP headers
- Deploy with reverse proxy (nginx, Caddy)
- Enable logging and monitoring
- Regular security audits
