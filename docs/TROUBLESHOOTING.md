# ArchDrop Troubleshooting Guide

Solutions to common issues and debugging techniques.

## Table of Contents

1. [Installation Issues](#installation-issues)
2. [Runtime Errors](#runtime-errors)
3. [Connection Problems](#connection-problems)
4. [Transfer Issues](#transfer-issues)
5. [Performance Problems](#performance-problems)
6. [Browser Issues](#browser-issues)
7. [Debug Mode](#debug-mode)

---

## Installation Issues

### Build Fails: "linker 'cc' not found"

**Problem**: Missing C compiler

**Solution**:
```bash
# Debian/Ubuntu
sudo apt install build-essential

# Fedora/RHEL
sudo dnf install gcc

# Arch
sudo pacman -S base-devel
```

### Build Fails: "failed to run custom build command for `openssl-sys`"

**Problem**: Missing OpenSSL development files

**Solution**:
```bash
# Debian/Ubuntu
sudo apt install libssl-dev pkg-config

# Fedora/RHEL
sudo dnf install openssl-devel pkgconf

# Arch
sudo pacman -S openssl pkg-config
```

### Cargo Not Found

**Problem**: Rust not installed or not in PATH

**Solution**:
```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add to PATH (add to ~/.bashrc or ~/.zshrc)
source $HOME/.cargo/env

# Verify
cargo --version
```

### Wrong Rust Version

**Problem**: Rust version too old

**Solution**:
```bash
# Update Rust
rustup update stable

# Verify version (need 1.70+)
rustc --version
```

---

## Runtime Errors

### "File not found" Error

**Problem**: File specified doesn't exist or wrong path

**Symptoms**:
```
Error: File not found: /path/to/file.txt
```

**Solutions**:
1. Check file exists:
   ```bash
   ls -l /path/to/file.txt
   ```

2. Use absolute path:
   ```bash
   archdrop send /absolute/path/to/file.txt
   ```

3. Check current directory:
   ```bash
   pwd
   archdrop send ./file.txt
   ```

### "Permission denied" Error

**Problem**: Insufficient permissions to read file or write to directory

**Solutions**:
1. Check file permissions:
   ```bash
   ls -l file.txt
   ```

2. For sending, ensure read permission:
   ```bash
   chmod +r file.txt
   ```

3. For receiving, ensure write permission on directory:
   ```bash
   chmod +w ~/Downloads
   ```

4. Don't use `sudo` unnecessarily (security risk)

### "Cannot create directory" Error

**Problem**: Destination directory cannot be created

**Solutions**:
1. Check parent directory exists:
   ```bash
   ls -ld ~/Downloads
   ```

2. Create manually:
   ```bash
   mkdir -p ~/Downloads
   ```

3. Check permissions:
   ```bash
   ls -ld ~
   ```

### "Address already in use" Error

**Problem**: Port already bound by another process

**Symptoms**:
```
Error: Address already in use (os error 98)
```

**Solutions**:
1. Kill existing archdrop process:
   ```bash
   pkill archdrop
   ```

2. Find process using port:
   ```bash
   # Find which process is using the port
   sudo lsof -i :PORT
   sudo kill PID
   ```

3. Wait a few seconds for OS to release port

4. Use different network interface (automatic, but check firewall)

---

## Connection Problems

### Browser Shows "Connection Refused"

**Problem**: Server not running or port not accessible

**Solutions**:
1. Verify server is running:
   ```bash
   # Server should show QR code and be waiting
   ```

2. Check port is open:
   ```bash
   ss -tuln | grep PORT
   ```

3. Check firewall:
   ```bash
   # Debian/Ubuntu
   sudo ufw status
   sudo ufw allow PORT

   # Fedora/RHEL
   sudo firewall-cmd --list-ports
   sudo firewall-cmd --add-port=PORT/tcp
   ```

### Browser Shows Certificate Error (Local Mode)

**Problem**: Self-signed certificate not trusted

**Symptoms**:
```
Your connection is not private
NET::ERR_CERT_AUTHORITY_INVALID
```

**Solutions**:
1. Click "Advanced" → "Proceed to 127.0.0.1 (unsafe)"

2. Or use tunnel mode instead:
   ```bash
   archdrop send file.txt  # Without --local
   ```

3. **Verify IP address** before accepting certificate (security)

### "Failed to establish Cloudflare tunnel"

**Problem**: `cloudflared` not installed or not in PATH

**Solutions**:
1. Install cloudflared:
   ```bash
   # Debian/Ubuntu
   wget https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64.deb
   sudo dpkg -i cloudflared-linux-amd64.deb

   # Manual install
   wget https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64
   sudo mv cloudflared-linux-amd64 /usr/local/bin/cloudflared
   sudo chmod +x /usr/local/bin/cloudflared
   ```

2. Verify installation:
   ```bash
   cloudflared --version
   ```

3. Use local mode as workaround:
   ```bash
   archdrop send file.txt --local
   ```

### Tunnel Times Out

**Problem**: Network blocks Cloudflare tunnel or tunnel service down

**Solutions**:
1. Check internet connection:
   ```bash
   ping 1.1.1.1
   ```

2. Check Cloudflare status:
   - Visit https://www.cloudflarestatus.com/

3. Try again (tunnel establishment can be slow)

4. Use local mode if on same network:
   ```bash
   archdrop send file.txt --local
   ```

5. Check corporate firewall/proxy settings

### "No route to host" (Local Mode)

**Problem**: Devices not on same network

**Solutions**:
1. Verify both devices on same network:
   ```bash
   ip addr show
   # Check IP address is in same subnet
   ```

2. Check WiFi network name matches

3. Disable VPN (may route traffic differently)

4. Use tunnel mode instead:
   ```bash
   archdrop send file.txt  # Without --local
   ```

---

## Transfer Issues

### Transfer Hangs at 0%

**Problem**: Session not claimed or browser not fetching chunks

**Solutions**:
1. Check browser console for errors (F12)

2. Verify URL is complete (includes fragment with keys):
   ```
   https://...#key=...&nonce=...
   ```

3. Refresh browser page

4. Restart server and try again

### Transfer Fails Midway

**Problem**: Network interruption, disk full, or decryption error

**Solutions**:
1. Check disk space:
   ```bash
   df -h ~/Downloads
   ```

2. Check network connection

3. Check browser console for specific error

4. Retry transfer (no resume support currently)

### "Session already claimed" Error

**Problem**: Another client already claimed the session

**Solutions**:
1. Generate new URL:
   ```bash
   # Restart server
   archdrop send file.txt
   ```

2. Ensure only one browser tab accesses URL

3. Don't share URL publicly

### "Decryption failed" Error

**Problem**: Wrong key, corrupted data, or mismatched nonce

**Solutions**:
1. Verify URL includes full fragment:
   ```
   #key=...&nonce=...
   ```

2. Check for URL encoding issues (% characters)

3. Try regenerating URL (restart server)

4. Check for network corruption:
   ```bash
   # Use local mode to eliminate network issues
   archdrop send file.txt --local
   ```

### "Invalid file index" or "Invalid chunk index"

**Problem**: Browser requesting non-existent chunk

**Solutions**:
1. Check browser console for JavaScript errors

2. Refresh browser page

3. Restart transfer

4. **Report as bug** (shouldn't happen normally)

### Incomplete Upload (Receive Mode)

**Problem**: Not all chunks uploaded before finalization

**Symptoms**:
```
Error: Incomplete upload: received 95/100 chunks
```

**Solutions**:
1. Check browser console for failed chunk uploads

2. Retry transfer (browser should re-upload)

3. Check network stability

4. Check disk space on server

---

## Performance Problems

### Slow Transfer Speed

**Problem**: Network bottleneck, encryption overhead, or disk I/O

**Diagnosis**:
1. Check network speed:
   ```bash
   # Local mode: should be LAN speed (100Mbps-10Gbps)
   # Tunnel mode: limited by internet upload speed
   ```

2. Monitor CPU usage:
   ```bash
   htop
   # Check if archdrop is CPU-bound
   ```

3. Monitor disk I/O:
   ```bash
   iotop
   # Check if disk is bottleneck
   ```

**Solutions**:
1. Use local mode (faster than tunnel):
   ```bash
   archdrop send file.txt --local
   ```

2. Close other bandwidth-intensive applications

3. Use SSD instead of HDD (especially for receive mode)

4. Check CPU has AES-NI instructions:
   ```bash
   grep aes /proc/cpuinfo
   ```

5. Use wired ethernet instead of WiFi

### High CPU Usage

**Problem**: Encryption/decryption is CPU-intensive

**Expected Behavior**: CPU usage 20-80% per core during transfer

**Solutions**:
1. This is normal for encryption (AES-256-GCM)

2. Use hardware acceleration if available (AES-NI)

3. Close other CPU-intensive applications

4. Wait for transfer to complete (CPU usage drops to zero)

### High Memory Usage

**Problem**: Multiple concurrent chunks in memory

**Expected**: ~10MB base + ~4MB per concurrent request

**Solutions**:
1. Memory usage should stabilize, not grow indefinitely

2. If memory grows without bound:
   - **Report as bug** (memory leak)

3. Monitor with:
   ```bash
   ps aux | grep archdrop
   ```

### Disk Space Issues

**Problem**: Receiving large files without enough space

**Solutions**:
1. Check space before receiving:
   ```bash
   df -h ~/Downloads
   ```

2. Clean up old files

3. Change destination to larger partition:
   ```bash
   archdrop receive /mnt/large_disk/
   ```

---

## Browser Issues

### QR Code Doesn't Scan

**Problem**: QR code corrupted or camera focus issues

**Solutions**:
1. Increase terminal window size (make QR larger)

2. Take screenshot and scan screenshot

3. Manually copy URL:
   - URL is printed above QR code
   - Paste into browser on other device

4. Adjust terminal contrast/brightness

### Browser Doesn't Support Web Crypto API

**Problem**: Old browser or insecure context

**Symptoms**:
```
Error: crypto.subtle is undefined
```

**Solutions**:
1. Update browser to latest version

2. Ensure using HTTPS (not HTTP):
   - Tunnel mode: automatic HTTPS
   - Local mode: uses HTTPS (accept certificate)

3. Try different browser:
   - Chrome/Chromium
   - Firefox
   - Safari
   - Edge

4. Don't use incognito/private mode (some browsers restrict crypto API)

### Browser Runs Out of Memory

**Problem**: Large files exhaust browser memory

**Symptoms**:
- Browser tab crashes
- "Out of memory" error
- Browser freezes

**Solutions**:
1. Close other tabs and applications

2. Use desktop browser (not mobile) for large files

3. Split large files:
   ```bash
   split -b 1G large_file.bin part_
   archdrop send part_*
   ```

4. Wait for reassembly to complete before opening file

### File Download Doesn't Start

**Problem**: Browser popup blocker or download permission issue

**Solutions**:
1. Allow popups for the site

2. Check browser download settings

3. Grant file system access permission (receive mode)

4. Try different browser

### JavaScript Errors in Console

**Problem**: Browser compatibility or server issue

**Solutions**:
1. Open browser console (F12) and copy error

2. Check if error is network-related (red errors)

3. Try refreshing page

4. Try different browser

5. **Report as bug** with error details

---

## Debug Mode

### Enable Verbose Logging

**Current Implementation**: Basic logging to stderr

**View Logs**:
```bash
# Redirect stderr to file
archdrop send file.txt 2> debug.log

# View live logs
archdrop send file.txt 2>&1 | tee debug.log
```

### Check Server Logs

**Look for**:
- Connection attempts
- Token validation failures
- Decryption errors
- File I/O errors

**Example Log Output**:
```
[receive] Setting nonce from chunk 0
Chunk 5 received: 6/10 chunks
Finalize: file.txt - SHA256: e3b0c442...
```

### Network Debugging

**Capture HTTP Traffic**:
```bash
# In one terminal
archdrop send file.txt --local

# In another terminal
sudo tcpdump -i lo -w capture.pcap port 54321

# Analyze with Wireshark
wireshark capture.pcap
```

**Test with curl**:
```bash
TOKEN="your-token"
PORT="54321"

# Health check
curl -k "https://127.0.0.1:${PORT}/health"

# Get manifest
curl -k "https://127.0.0.1:${PORT}/send/${TOKEN}/manifest"

# Download chunk
curl -k "https://127.0.0.1:${PORT}/send/${TOKEN}/0/chunk/0" -o chunk.bin
```

### Verify Binary Integrity

**Check for Corruption**:
```bash
# Get hash from official release
sha256sum archdrop
# Compare with official hash
```

**Rebuild from Source**:
```bash
cargo clean
cargo build --release
```

---

## Common Error Messages

### "Invalid token"

**Cause**: Token in URL doesn't match server's session token

**Solutions**:
- Verify URL is complete and correct
- Regenerate URL (restart server)
- Don't modify URL

### "Session already claimed"

**Cause**: Another client already accessed the URL

**Solutions**:
- Generate new URL
- Ensure URL is private
- Check for malicious access (if unexpected)

### "Invalid or inactive session"

**Cause**: Session completed or never claimed

**Solutions**:
- Check if transfer already completed
- Restart server if transfer not complete

### "Invalid file path"

**Cause**: Path traversal attempt or invalid characters

**Solutions**:
- Check file path is valid
- Avoid special characters in filenames
- Report as bug if legitimate path rejected

### "No manifest available"

**Cause**: Server in receive mode, but client requested send manifest

**Solutions**:
- Verify correct URL (send vs receive)
- Restart server in correct mode

---

## Platform-Specific Issues

### Linux

**Firewall Blocking Connections**:
```bash
# Check firewall status
sudo ufw status

# Temporarily disable (testing only)
sudo ufw disable

# Or allow specific port
sudo ufw allow 54321
```

**SELinux Denials**:
```bash
# Check for denials
sudo ausearch -m AVC -ts recent

# Temporarily set permissive (testing only)
sudo setenforce 0

# Fix with policy module (advanced)
```

### Arch Linux

**Missing Dependencies**:
```bash
# Install all build dependencies
sudo pacman -S base-devel openssl pkg-config
```

### Ubuntu/Debian

**OpenSSL Version Mismatch**:
```bash
# Install correct version
sudo apt install libssl-dev
```

---

## Getting Help

### Information to Include

When reporting issues, include:

1. **System Info**:
   ```bash
   uname -a
   archdrop --version  # (if it builds)
   rustc --version
   ```

2. **Error Message**:
   - Full error text
   - Stack trace if available

3. **Steps to Reproduce**:
   - Exact commands run
   - File sizes
   - Network setup (local/tunnel)

4. **Logs**:
   ```bash
   archdrop send file.txt 2> error.log
   # Attach error.log
   ```

5. **Browser Info**:
   - Browser name and version
   - Console errors (F12)
   - Network tab (if relevant)

### Where to Get Help

1. **GitHub Issues**: https://github.com/your-username/archdrop/issues
   - Bug reports
   - Feature requests

2. **GitHub Discussions**: For questions and general discussion

3. **Email**: (if provided)

---

## FAQ

**Q: Why does local mode show a certificate warning?**
A: Self-signed certificate is used. This is normal. Verify IP address before accepting.

**Q: Can I resume interrupted transfers?**
A: Not currently supported. Transfer must restart from beginning.

**Q: Why is tunnel mode slower than local mode?**
A: Tunnel routes through Cloudflare's CDN, adding latency and bandwidth limits.

**Q: Does archdrop work on Windows?**
A: No, Linux only. Use WSL2 on Windows.

**Q: Can I transfer multiple files at once?**
A: Yes, `archdrop send file1.txt file2.txt` or `archdrop send directory/`.

**Q: How do I stop a transfer?**
A: Press `q`, `c`, or `Esc` in the TUI, or Ctrl+C.

**Q: Are files encrypted?**
A: Yes, AES-256-GCM end-to-end encryption.

**Q: Can the server read my files?**
A: No, encryption keys are in URL fragment (never sent to server).

**Q: What's the maximum file size?**
A: No hard limit. Practical limit depends on available disk space and time.

**Q: Can multiple people download the same file?**
A: No, single-use sessions. First person claims the transfer.

**Q: How long does the URL stay valid?**
A: Until transfer completes or server shuts down. No time-based expiration.

---

## Still Having Issues?

If this guide doesn't solve your problem:

1. **Search existing issues**: https://github.com/your-username/archdrop/issues
2. **Open new issue**: Include all information from "Getting Help" section
3. **Check documentation**: README, API.md, SECURITY.md
4. **Ask community**: GitHub Discussions

For security vulnerabilities, see [SECURITY.md](SECURITY.md) for responsible disclosure.
