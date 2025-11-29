# ArchDrop

Secure peer-to-peer file transfer CLI for Linux. Transfer files directly between your devices without uploading to cloud services.

## Features

- End-to-end AES-256-GCM encryption
- Zero-knowledge architecture (server never sees unencrypted data)
- QR code for easy cross-device transfers
- Works across different operating systems (Linux sender, any browser receiver)
- No file size limits
- Single binary, no external dependencies

## Installation

```bash
cargo build --release
sudo cp target/release/archdrop /usr/local/bin/
```

## Usage

### Send Files

```bash
# Local network (faster, requires HTTPS certificate acceptance)
archdrop send file.txt --local

# Internet-accessible (via Cloudflare tunnel)
archdrop send file.txt
```

### Receive Files

```bash
# Receive files to current directory
archdrop receive

# Receive files to specific directory
archdrop receive ~/Downloads --local
```

### Transfer Flow

1. Run `archdrop send` or `archdrop receive` on your Linux machine
2. Scan the QR code with your phone/other device
3. Files are encrypted client-side and transferred directly
4. Server shuts down automatically after transfer completes

## How It Works

1. **Send Mode**: Server streams encrypted files. Client (browser) decrypts and downloads.
2. **Receive Mode**: Client (browser) encrypts files. Server stores encrypted chunks, decrypts on finalization.

Encryption keys are transmitted in the URL fragment (after `#`), which never reaches the server.

## Security

- AES-256-GCM authenticated encryption
- Cryptographically random session tokens (UUID v4)
- Path traversal protection
- Single-use sessions

**Note:** This is a proof-of-concept. See CODE_REVIEW.md for security considerations before production use.

## Requirements

- Linux (tested on Ubuntu/Debian)
- Rust 1.70+ (for building)
- Modern browser with Web Crypto API support
- `cloudflared` (optional, for tunnel mode)

## Tunnel Mode

To use tunnel mode (default), install cloudflared:

```bash
# Debian/Ubuntu
wget https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.deb
sudo dpkg -i cloudflared-linux-amd64.deb
```

Or use `--local` flag for local network transfers without tunnel.

## Architecture

See ARCHITECTURE.md for detailed technical documentation.

## Testing

```bash
cargo test
```

## License

MIT

## Contributing

See CODE_REVIEW.md for areas needing improvement.
