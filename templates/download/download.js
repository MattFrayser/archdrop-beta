const CHUNK_SIZE = 64 * 1024 // 64kb
const MAX_MEMORY = 100 * 1024 * 1024 // 100MB
document.addEventListener('DOMContentLoaded', () => {
    const downloadBtn = document.getElementById('downloadBtn');
    if (downloadBtn) {
        downloadBtn.addEventListener('click', startDownload);
    }
});

async function startDownload() {
    try { 
        // Get session key form url  
        const { key } = await getCredentialsFromUrl()
        const token = window.location.pathname.split('/').pop()

        // Fetch manifest
        const manifestResponse = await fetch(`/send/${token}/manifest`)
        if (!manifestResponse.ok) {
            throw new Error(`Failed to fetch file list: HTTP ${manifestResponse.status}`)
        }

        const manifest = await manifestResponse.json()

        // Download each file
        for (const fileEntry of manifest.files) {
            await downloadSingleFile(token, fileEntry, key)
        }

    } catch(error) {
        console.error(error)
        alert(`Download failed: ${error.message}`)
    }
}

async function downloadSingleFile(token, fileEntry, sessionKey) {
    const nonceBase = urlSafeBase64ToUint8Array(fileEntry.nonce)
    const totalChunks = Math.ceil(fileEntry.size / CHUNK_SIZE)

    // Large file -> Use File System Access API
    if (fileEntry.size > MAX_MEMORY && 'showSaveFilePicker' in window) {
        await downloadLargeFile(token, fileEntry, sessionKey, nonceBase, totalChunks)
    } else {
        await downloadSmallFile(token, fileEntry, sessionKey, nonceBase, totalChunks)
    }
}

async function downloadLargeFile(token, fileEntry, key, nonceBase, totalChunks) {
    const fileHandle = await window.showSaveFilePicker({
        suggestedName: fileEntry.name,
    });

    const writable = await fileHandle.createWritable()

    try {
        const decryptedChunks = []

        for (let i = 0; i < totalChunks; i++) {
            // download
            const encrypted = await downloadChunk(token, fileEntry.index, i)

            // decrypt
            const nonce = generateNonce(nonceBase, i)
            const decrypted = await crypto.subtle.decrypt(
                { name: 'AES-GCM', iv: nonce },
                key,
                encrypted
            )

            const decryptedArray = new Uint8Array(decrypted)

            // write to disk
            await writable.write(decryptedArray)

            decryptedChunks.push(decryptedArray)
        }
        await writable.close()
        // verify hash 
        const blob = new Blob(decryptedChunks)
        await verifyHash(blob, fileEntry)

    } catch (error) {
        await writable.abort()
        throw error
    }
}

async function downloadSmallFile(token, fileEntry, key, nonceBase, totalChunks) {
    const decryptedChunks = []

    for (let i = 0; i < totalChunks; i++) {
        const encrypted = await downloadChunk(token, fileEntry.index, i)

        const nonce = generateNonce(nonceBase, i)
        const decrypted = await crypto.subtle.decrypt(
            { name: 'AES-GCM', iv: nonce },
            key,
            encrypted
        )

        decryptedChunks.push(new Uint8Array(decrypted))
    }

    const blob = new Blob(decryptedChunks)
    await verifyHash(blob, fileEntry)

    // Trigger Download
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = fileEntry.name
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
}

async function verifyHash(blob, fileEntry) {
    const arrayBuffer = await blob.arrayBuffer()
    const hashBuffer = await crypto.subtle.digest('SHA-256', arrayBuffer)
    const hashArray = Array.from(new Uint8Array(hashBuffer))
    const computedHash = hashArray
        .map(b => b.toString(16).padStart(2,'0'))
        .join('')

    if (computedHash !== fileEntry.sha256) {
        throw new Error(`File integrity check failed! Expected ${fileEntry.sha256}, got ${computedHash}`)
    }
}

async function downloadChunk(token, fileIndex, chunkIndex, maxRetries = 3) {
    for (let attempt = 0; attempt < maxRetries; attempt++) {
        try {
            const response = await fetch(`/send/${token}/${fileIndex}/chunk/${chunkIndex}`)
            if (!response.ok) {
                throw new Error(`HTTP ${response.status}`)
            }
            return await response.arrayBuffer()
        } catch(e) {
            if (attempt === maxRetries - 1) {
                throw new Error(`Failed to download chunk ${chunkIndex} after ${maxRetries} attempts: ${e.message}`)
            }
            const delay = 1000 * Math.pow(2, attempt);
            await new Promise(r => setTimeout(r, delay));
        }
    }
}


