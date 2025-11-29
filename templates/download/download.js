const CHUNK_SIZE = 1024 * 1024 // 1MB (increased from 64KB for better throughput)
const MAX_MEMORY = 100 * 1024 * 1024 // 100MB
const MAX_CONCURRENT_DOWNLOADS = 8 // Parallel chunk download limit
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
        // Store decrypted chunks in order for hash verification
        const decryptedChunks = new Array(totalChunks)

        // Parallel chunk downloads with concurrency limit
        await downloadChunksParallel(
            token,
            fileEntry.index,
            totalChunks,
            key,
            nonceBase,
            async (chunkIndex, decryptedData) => {
                // Write to disk immediately
                await writable.write(decryptedData)
                // Store for hash verification
                decryptedChunks[chunkIndex] = decryptedData
            }
        )

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
    // Store decrypted chunks in order
    const decryptedChunks = new Array(totalChunks)

    // Parallel chunk downloads with concurrency limit
    await downloadChunksParallel(
        token,
        fileEntry.index,
        totalChunks,
        key,
        nonceBase,
        async (chunkIndex, decryptedData) => {
            decryptedChunks[chunkIndex] = decryptedData
        }
    )

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

// Parallel chunk download with concurrency control
async function downloadChunksParallel(token, fileIndex, totalChunks, key, nonceBase, onChunkReady) {
    const chunkIndexes = Array.from({ length: totalChunks }, (_, i) => i)

    // Process chunks with concurrency limit
    const processChunk = async (chunkIndex) => {
        // Download encrypted chunk
        const encrypted = await downloadChunk(token, fileIndex, chunkIndex)

        // Decrypt chunk
        const nonce = generateNonce(nonceBase, chunkIndex)
        const decrypted = await crypto.subtle.decrypt(
            { name: 'AES-GCM', iv: nonce },
            key,
            encrypted
        )

        const decryptedData = new Uint8Array(decrypted)

        // Call callback with chunk
        await onChunkReady(chunkIndex, decryptedData)
    }

    // Run with concurrency limit
    await runWithConcurrency(chunkIndexes, processChunk, MAX_CONCURRENT_DOWNLOADS)
}

// Helper: Run async tasks with concurrency limit
async function runWithConcurrency(items, asyncFn, concurrency) {
    const results = []
    const executing = []

    for (const item of items) {
        const promise = asyncFn(item).then(result => {
            executing.splice(executing.indexOf(promise), 1)
            return result
        })

        results.push(promise)
        executing.push(promise)

        if (executing.length >= concurrency) {
            await Promise.race(executing)
        }
    }

    return Promise.all(results)
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


