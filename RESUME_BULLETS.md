# Resume Bullet Points for ArchDrop

## Option 1: Security-Focused

**Engineered zero-knowledge file transfer CLI in Rust with AES-256-GCM streaming encryption, eliminating server access to plaintext data through client-side key management and achieving 1+ GB/s throughput on AES-NI hardware**

**Key highlights:** Zero-knowledge architecture, cryptography, performance optimization

---

## Option 2: Full-Stack Systems Programming

**Built peer-to-peer file transfer system using Rust async runtime (Tokio), Axum web framework, and Web Crypto API, implementing chunked streaming with stateless decryption to support unbounded file sizes while maintaining constant memory usage**

**Key highlights:** Systems programming, async Rust, memory efficiency, full-stack

---

## Option 3: Infrastructure & DevOps Angle

**Developed cross-platform secure file transfer tool with dual-mode networking (self-signed TLS and Cloudflare Tunnels), QR-based authentication, and automated session lifecycle management, deployed as single static binary with embedded web assets**

**Key highlights:** DevOps, networking, deployment automation

---

## Recommended: Option 2

This bullet point best demonstrates breadth of skills (systems programming, networking, cryptography, full-stack web) while highlighting technical depth (async Rust, memory management, stateless crypto design).

## Usage Tips

- Place under "Projects" or "Personal Projects" section
- Add sub-bullets if space allows:
  - "Implemented counter-based nonce construction enabling parallel chunk processing and resume capability"
  - "Integrated terminal UI (ratatui) with real-time progress tracking via Tokio watch channels"
  - "Secured against path traversal via canonical path validation and single-use session tokens"

- Adjust based on target role:
  - Backend/Systems: Emphasize Rust, async, memory efficiency
  - Security: Emphasize zero-knowledge, cryptography, threat model
  - Full-stack: Emphasize browser crypto, web framework, end-to-end ownership
  - DevOps: Emphasize deployment, tunneling, single binary distribution
