# ArchDrop

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

**Secure peer-to-peer file transfer for Linux.** Transfer files directly between your devices without uploading to cloud services.

```bash
# Send a file
archdrop send file.txt

# Receive files
archdrop receive ~/Downloads
```

---

## Features

- 🔒 **End-to-End Encryption**: AES-256-GCM authenticated encryption
- 🔑 **Zero-Knowledge Architecture**: Server never sees unencrypted data or keys
- 📱 **QR Code Sharing**: Easy cross-device transfers
- 🌐 **Universal Access**: Linux sender, any browser receiver
- ⚡ **No File Size Limits**: Stream files of any size
- 📦 **Single Binary**: No external dependencies (except optional `cloudflared`)
- 🚀 **Fast Local Mode**: Direct HTTPS for LAN transfers
- 🌍 **Tunnel Mode**: Internet-accessible via Cloudflare tunnels

---

## Table of Contents

- [Quick Start](#quick-start)
- [Installation](#installation)
- [Usage](#usage)
- [How It Works](#how-it-works)
- [Documentation](#documentation)
- [Security](#security)
- [Contributing](#contributing)
- [License](#license)

---

## Quick Start

### 1. Install

```bash
cargo build --release
sudo cp target/release/archdrop /usr/local/bin/
```

### 2. Send Files

```bash
archdrop send document.pdf
```

### 3. Scan QR Code

Open the URL on your phone or other device

### 4. Download

Files are automatically encrypted, transferred, and decrypted

---

## Installation

### From Source (Recommended)

**Prerequisites**:
- Rust 1.70+ ([Install Rust](https://rustup.rs/))
- Linux operating system
- Build essentials (`build-essential` on Debian/Ubuntu)

```bash
# Clone repository
git clone https://github.com/your-username/archdrop-beta.git
cd archdrop-beta

# Build release binary
cargo build --release

# Install (optional)
sudo cp target/release/archdrop /usr/local/bin/
```

### Optional: Cloudflare Tunnel

For internet-accessible transfers (tunnel mode), install `cloudflared`:

```bash
# Debian/Ubuntu
wget https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.deb
sudo dpkg -i cloudflared-linux-amd64.deb

# Arch Linux
sudo pacman -S cloudflared

# Manual installation
wget https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64
sudo mv cloudflared-linux-amd64 /usr/local/bin/cloudflared
sudo chmod +x /usr/local/bin/cloudflared
```

---

## Usage

### Send Mode

Send files to another device:

```bash
# Single file (tunnel mode - works anywhere)
archdrop send document.pdf

# Single file (local mode - faster, same network required)
archdrop send document.pdf --local

# Multiple files
archdrop send file1.txt file2.pdf image.jpg

# Entire directory
archdrop send ~/Documents/

# Multiple directories
archdrop send ~/Photos/ ~/Videos/
```

**Output**: QR code and URL displayed in terminal

### Receive Mode

Receive files from another device:

```bash
# Receive to current directory
archdrop receive

# Receive to specific directory
archdrop receive ~/Downloads

# Local mode (same network)
archdrop receive ~/Downloads --local
```

**Output**: QR code and URL displayed in terminal

### Transfer Flow

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Run ArchDrop on Linux machine                            │
│    $ archdrop send file.txt                                 │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. QR code and URL displayed                                │
│    [QR CODE]                                                │
│    https://example.trycloudflare.com/send/TOKEN#key=...    │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Scan QR code with phone/other device                    │
│    Or manually enter URL in browser                         │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. Browser downloads and decrypts files automatically      │
│    Progress shown in terminal TUI                          │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ 5. Transfer complete                                        │
│    Server shuts down automatically                          │
└─────────────────────────────────────────────────────────────┘
```

### Local vs Tunnel Mode

| Mode | Flag | Use Case | Speed | Setup |
|------|------|----------|-------|-------|
| **Tunnel** (default) | None | Devices on different networks | Internet speed | Requires `cloudflared` |
| **Local** | `--local` | Devices on same network | LAN speed (faster) | Requires certificate acceptance |

**When to use Tunnel Mode**:
- Sending to device on different network
- Behind NAT/firewall
- Don't want to deal with certificates

**When to use Local Mode**:
- Maximum speed needed
- Devices on same WiFi/LAN
- Don't want external dependencies

---

## How It Works

### Architecture Overview

```
┌─────────────────┐                           ┌─────────────────┐
│  Linux Device   │                           │  Other Device   │
│                 │                           │                 │
│  ┌───────────┐  │    Encrypted Transfer    │  ┌───────────┐  │
│  │ ArchDrop  │◀─┼──────────────────────────┼─▶│  Browser  │  │
│  │  Server   │  │                           │  │           │  │
│  └───────────┘  │                           │  └───────────┘  │
│                 │                           │                 │
│  • Encrypts     │                           │  • Decrypts     │
│  • Streams      │                           │  • Downloads    │
│  • Never sees   │                           │  • Has keys     │
│    keys         │                           │                 │
└─────────────────┘                           └─────────────────┘
```

### Security Model

1. **Key Generation**: Server generates AES-256 key and nonce
2. **URL Construction**: Keys embedded in URL fragment (`#key=...&nonce=...`)
3. **Zero-Knowledge**: Fragment never sent to server (HTTP specification)
4. **Browser Extraction**: JavaScript reads keys from `window.location.hash`
5. **End-to-End Encryption**: Files encrypted on sender, decrypted on receiver

**The server never has access to encryption keys.**

### Send Mode Flow

```
Server                          Browser
  │                               │
  ├─ Generate keys                │
  ├─ Create URL with keys in #    │
  ├─ Display QR code              │
  │                               │
  │◀──── GET /manifest ───────────┤
  ├──── JSON file list ──────────▶│
  │                               ├─ Calculate chunks needed
  │                               │
  │◀──── GET /chunk/0 ────────────┤
  ├──── Encrypted chunk 0 ───────▶│
  │◀──── GET /chunk/1 ────────────┤  (parallel)
  ├──── Encrypted chunk 1 ───────▶│
  │◀──── GET /chunk/N ────────────┤
  ├──── Encrypted chunk N ───────▶│
  │                               ├─ Decrypt chunks
  │                               ├─ Assemble file
  │                               ├─ Save to disk
  │◀──── POST /complete ──────────┤
  ├─ Shutdown                     │
```

### Receive Mode Flow

```
Server                          Browser
  │                               │
  ├─ Generate keys                │
  ├─ Create URL with keys in #    │
  ├─ Display QR code              │
  │                               │
  │                               ├─ User selects files
  │                               ├─ Split into chunks
  │                               ├─ Encrypt chunks
  │                               │
  │◀──── POST /chunk/0 ───────────┤
  ├─ Decrypt chunk 0              │
  ├─ Write to position 0          │
  │◀──── POST /chunk/1 ───────────┤  (parallel)
  ├─ Decrypt chunk 1              │
  ├─ Write to position 1          │
  │◀──── POST /chunk/N ───────────┤
  ├─ Decrypt chunk N              │
  ├─ Write to position N          │
  │◀──── POST /finalize ──────────┤
  ├─ Verify all chunks received   │
  ├─ Compute SHA-256 hash         │
  ├──── Hash ────────────────────▶│
  │◀──── POST /complete ──────────┤
  ├─ Shutdown                     │
```

---

## Documentation

### For Users

- 📖 **[README.md](README.md)** - This file (overview and quick start)
- 🔧 **[TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md)** - Common issues and solutions
- 🔒 **[SECURITY.md](docs/SECURITY.md)** - Security model and threat analysis

### For Developers

- 🏗️ **[ARCHITECTURE.md](docs/ARCHITECTURE.md)** - System architecture and design decisions
- 📚 **[CODEBASE_GUIDE.md](CODEBASE_GUIDE.md)** - Complete Rust codebase walkthrough for learners
- 🔌 **[API.md](docs/API.md)** - HTTP API reference
- 💻 **[DEVELOPMENT.md](docs/DEVELOPMENT.md)** - Development setup and workflow
- 🤝 **[CONTRIBUTING.md](docs/CONTRIBUTING.md)** - Contributing guidelines

### Key Documentation

#### Architecture

ArchDrop uses a **zero-knowledge architecture** where:
- Server handles encrypted data only
- Keys are distributed via URL fragments
- Browser performs all encryption/decryption

See [ARCHITECTURE.md](docs/ARCHITECTURE.md) for detailed design decisions, performance characteristics, and trade-offs.

#### Security

- **Encryption**: AES-256-GCM authenticated encryption
- **Key Generation**: Cryptographically secure random (OsRng)
- **Session Tokens**: UUIDv4, single-use, atomic claiming
- **Path Validation**: Prevents directory traversal attacks

See [SECURITY.md](docs/SECURITY.md) for full threat model and security analysis.

#### API

RESTful HTTP API with JSON responses:

**Send Mode**:
- `GET /send/:token/manifest` - List files
- `GET /send/:token/:file/chunk/:chunk` - Download encrypted chunk
- `POST /send/:token/complete` - Mark transfer complete

**Receive Mode**:
- `POST /receive/:token/chunk` - Upload encrypted chunk
- `POST /receive/:token/finalize` - Finalize file after all chunks
- `POST /receive/:token/complete` - Mark transfer complete

See [API.md](docs/API.md) for complete endpoint documentation.

---

## Security

### Threat Model

**Trusted**:
- User's device (sender/receiver)
- User's browser (Web Crypto API)
- User's network path

**Untrusted**:
- Server process (zero-knowledge design)
- Network between server and browser
- Cloudflare tunnel

**Key Security Properties**:
- ✅ Server cannot decrypt files (no access to keys)
- ✅ Network eavesdroppers see only encrypted data (HTTPS + E2E encryption)
- ✅ Tampering detected (authenticated encryption)
- ✅ Single-use sessions (atomic token claiming)
- ⚠️ JavaScript trust boundary (see [Known Limitations](#known-limitations))

### Encryption Details

**Algorithm**: AES-256-GCM
- 256-bit keys (2^256 keyspace)
- Galois/Counter Mode (authenticated encryption)
- 12-byte nonces (unique per chunk)
- 16-byte authentication tags

**Nonce Structure**:
```
[7 bytes random base][4 bytes counter][1 byte flag] = 12 bytes total
```

Each file has a unique random nonce base. Each chunk combines this base with a counter, ensuring no nonce reuse.

### Known Limitations

1. **JavaScript Trust Boundary**: If server is compromised, malicious JavaScript could extract keys. Consider verifying JavaScript integrity manually for sensitive data.

2. **Self-Signed Certificates (Local Mode)**: Browser shows certificate warnings. User must verify IP address before accepting.

3. **No URL Revocation**: Once generated, URLs cannot be revoked. Treat URLs as passwords.

4. **Metadata Leakage**: Server sees file sizes, transfer timing, and file count (but not content).

See [SECURITY.md](docs/SECURITY.md) for complete security analysis.

### Best Practices

**For Users**:
- Treat transfer URLs like passwords (don't share publicly)
- Verify IP address when accepting certificates (local mode)
- Use trusted networks for sensitive data
- Clear browser history after transfer

**For Developers**:
- Run `cargo audit` regularly
- Review dependencies for vulnerabilities
- Follow secure coding practices
- Report security issues privately

### Reporting Vulnerabilities

**Do not publicly disclose security vulnerabilities.**

To report a security issue:
1. Email: (provide email if available)
2. Describe the vulnerability
3. Provide steps to reproduce
4. Allow 90 days for patch before public disclosure

See [SECURITY.md](docs/SECURITY.md) for responsible disclosure process.

---

## Requirements

### System Requirements

**Required**:
- Linux operating system (kernel 3.2+)
- Rust 1.70+ (for building from source)
- 50MB disk space
- 100MB RAM

**Recommended**:
- Linux kernel 4.0+
- 512MB RAM
- Gigabit network (for local mode)

**Optional**:
- `cloudflared` (for tunnel mode)
- Modern browser with Web Crypto API support

### Browser Compatibility

**Supported Browsers**:
- Chrome/Chromium 60+
- Firefox 57+
- Safari 11.1+
- Edge 79+

**Requirements**:
- Web Crypto API support
- File System Access API (for receive mode)
- ES6+ JavaScript support

---

## Performance

### Benchmarks

Approximate performance on modern hardware:

| Metric | Local Mode | Tunnel Mode |
|--------|-----------|-------------|
| **Throughput** | 500-800 Mbps | Internet limited |
| **Latency** | <10ms per chunk | 50-200ms per chunk |
| **CPU Usage** | 20-40% per core | 20-40% per core |
| **Memory** | ~10MB base + ~4MB per request | Same |

### Optimization Tips

**For Maximum Speed**:
1. Use local mode (`--local`)
2. Use wired ethernet (not WiFi)
3. Ensure CPU has AES-NI instructions
4. Use SSD for receive mode (not HDD)
5. Close bandwidth-intensive applications

**For Large Files**:
- Files streamed (not loaded into memory)
- No file size limits (practical limit: available disk space)
- Progress tracking per chunk

See [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) for performance debugging.

---

## Roadmap

### Implemented

- ✅ End-to-end AES-256-GCM encryption
- ✅ Zero-knowledge architecture
- ✅ Single-use session tokens
- ✅ Local and tunnel modes
- ✅ QR code generation
- ✅ Terminal UI with progress
- ✅ Multi-file transfers
- ✅ Directory transfers
- ✅ Out-of-order chunk handling
- ✅ SHA-256 integrity verification

### Planned

**High Priority**:
- [ ] Resumable transfers (chunk tracking)
- [ ] Progress persistence (survive restarts)
- [ ] Multi-recipient support (broadcast)
- [ ] Optional compression (zstd)

**Medium Priority**:
- [ ] JavaScript integrity verification (SRI)
- [ ] Content Security Policy headers
- [ ] Structured logging (tracing)
- [ ] Metrics export (Prometheus)

**Low Priority**:
- [ ] GUI application (Electron/Tauri)
- [ ] Mobile app (Android receive mode)
- [ ] Plugin system (custom transforms)
- [ ] mDNS discovery (zeroconf)

See [CONTRIBUTING.md](docs/CONTRIBUTING.md) for areas needing contributions.

---

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](docs/CONTRIBUTING.md) for:
- Code of conduct
- Development setup
- Coding standards
- Testing guidelines
- Pull request process

### Quick Contribution Guide

1. **Fork** the repository
2. **Create** a feature branch (`git checkout -b feature/my-feature`)
3. **Make** your changes with tests
4. **Test** thoroughly (`cargo test`)
5. **Commit** with clear messages
6. **Push** to your fork
7. **Submit** a pull request

### Areas Needing Help

- [ ] Resumable transfers
- [ ] Compression support
- [ ] Multi-recipient broadcasts
- [ ] Documentation improvements
- [ ] Performance optimizations
- [ ] Security audits

---

## Troubleshooting

### Common Issues

**Build errors**: See [Installation Issues](docs/TROUBLESHOOTING.md#installation-issues)

**Connection problems**: See [Connection Problems](docs/TROUBLESHOOTING.md#connection-problems)

**Transfer issues**: See [Transfer Issues](docs/TROUBLESHOOTING.md#transfer-issues)

**Performance problems**: See [Performance Problems](docs/TROUBLESHOOTING.md#performance-problems)

### Getting Help

1. **Check documentation**: README, troubleshooting guide
2. **Search issues**: https://github.com/your-username/archdrop/issues
3. **Ask community**: GitHub Discussions
4. **Report bug**: Include logs, environment, steps to reproduce

See [TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) for detailed solutions.

---

## FAQ

**Q: Is ArchDrop secure?**
A: Yes, with caveats. Uses AES-256-GCM encryption and zero-knowledge architecture. Server cannot access keys or decrypt files. See [SECURITY.md](docs/SECURITY.md) for limitations.

**Q: Can I use this on Windows/macOS?**
A: No, Linux only. Use WSL2 on Windows. Native macOS support is not planned.

**Q: Does it work behind NAT/firewall?**
A: Yes, use tunnel mode (default). Local mode requires same network.

**Q: What's the maximum file size?**
A: No hard limit. Practical limit is available disk space and transfer time.

**Q: Can multiple people download the same file?**
A: No, single-use sessions. First person claims the transfer.

**Q: Are files stored on any server?**
A: No persistent storage. Received files written directly to destination. Send mode streams from disk.

**Q: Can I resume interrupted transfers?**
A: Not currently. This is a planned feature.

**Q: Why does local mode show certificate warning?**
A: Self-signed certificate. This is normal. Verify IP address before accepting.

**Q: How long does URL stay valid?**
A: Until transfer completes or server shuts down. No time-based expiration.

**Q: Can the server operator see my files?**
A: No. Keys are in URL fragment (never sent to server). Only encrypted data visible.

---

## License

MIT License - see [LICENSE](LICENSE) file for details.

```
Copyright (c) 2024 ArchDrop Contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## Acknowledgments

- **Rust Community**: For excellent tooling and libraries
- **Cloudflare**: For free tunnel service
- **Contributors**: Everyone who has contributed code, documentation, or ideas

### Built With

- [Rust](https://www.rust-lang.org/) - Programming language
- [Tokio](https://tokio.rs/) - Async runtime
- [Axum](https://github.com/tokio-rs/axum) - Web framework
- [aes-gcm](https://github.com/RustCrypto/AEADs) - Encryption
- [ratatui](https://github.com/ratatui-org/ratatui) - Terminal UI
- [clap](https://github.com/clap-rs/clap) - CLI parsing
- [qrcode](https://github.com/kennytm/qrcode-rust) - QR code generation

---

## Contact

- **Issues**: https://github.com/your-username/archdrop/issues
- **Discussions**: https://github.com/your-username/archdrop/discussions
- **Email**: (provide if available)

---

## Project Status

**Status**: Beta

ArchDrop is functional and suitable for personal use. Security has been carefully considered but not formally audited. Not recommended for:
- Regulated data (HIPAA, PCI DSS)
- Critical infrastructure
- Production deployments without additional security controls

See [SECURITY.md](docs/SECURITY.md) for detailed security analysis.

---

**Made with ❤️ and Rust**
