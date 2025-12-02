// Detect browser capabilities once on page load
const browserCaps = detectBrowserCapabilities()

function detectBrowserCapabilities() {
    const caps = {
        // File System Access API (Chrome, Edge, Opera, Brave)
        hasFileSystemAccess: 'showSaveFilePicker' in window,
        
        // Device memory in GB (Chrome-only, returns 2, 4, 8, etc.)
        deviceMemoryGB: navigator.deviceMemory || null,
        
        // Estimated available memory in bytes
        estimatedMemory: navigator.deviceMemory 
            ? navigator.deviceMemory * 1024 * 1024 * 1024 
            : 4 * 1024 * 1024 * 1024, // Default 4GB
    }
    
    console.log('Browser capabilities:', caps)
    return caps
}

document.addEventListener('DOMContentLoaded', async () => {
    const downloadBtn = document.getElementById('downloadBtn');
    if (downloadBtn) {
        downloadBtn.addEventListener('click', startDownload);
    }

    // Load manifest and display files
    try {
        const token = window.location.pathname.split('/').pop()
        const manifestResponse = await fetch(`/send/${token}/manifest`)
        if (manifestResponse.ok) {
            const manifest = await manifestResponse.json()
            displayFileList(manifest.files)
        }
    } catch (error) {
        console.error('Failed to load file list:', error)
    }
})

function displayFileList(files) {
    const fileList = document.getElementById('fileList')
    if (!fileList || files.length === 0) return

    fileList.classList.add('show')

    files.forEach((file, index) => {
        const item = createFileItem(file, index, {
            initialProgressText: 'Ready to download',
            useSummaryWrapper: true
        })

        const progress = item.querySelector('.file-progress')
        if (progress) progress.classList.add('show')

        fileList.appendChild(item)
    })
}

function supportFileSystemAccess() {
    return 'showOpenFilePicker' in window;
}

async function startDownload() {
    const fileList = document.getElementById('fileList')
    const fileItems = fileList.querySelectorAll('.file-item')

    // Show progress bars
    fileItems.forEach(item => {
        const progress = item.querySelector('.file-progress')
        if (progress) progress.classList.add('show')
    })

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

        // download files concurrently
        await runWithConcurrency(
            manifest.files.map((file, index) => ({ file, index, fileItem: fileItems[index] })),
            async ({ file, fileItem }) => {
                fileItem.classList.add('downloading')
                try {
                    await downloadFile(token, file, key, fileItem)
                    fileItem.classList.remove('downloading')
                    fileItem.classList.add('completed')
                } catch (error) {
                    fileItem.classList.remove('downloading')
                    fileItem.classList.add('error')
                    throw error
                }
            },
            MAX_CONCURRENT_FILES
        )

        await fetch(`/send/${token}/complete`, { method: 'POST' })

        const downloadBtn = document.getElementById('downloadBtn')
        downloadBtn.textContent = 'Download Complete!'

    } catch(error) {
        console.error(error)
        alert(`Download failed: ${error.message}`)
    }
}

async function downloadFile(token, fileEntry, key, fileItem) {
    const nonceBase = urlSafeBase64ToUint8Array(fileEntry.nonce)
    const totalChunks = Math.ceil(fileEntry.size / CHUNK_SIZE)
    
    if (browserCaps.hasFileSystemAccess && fileEntry.size > FILE_SYSTEM_API_THRESHOLD) {
        console.log(`Using File System API for ${fileEntry.name} (${formatFileSize(fileEntry.size)})`)
        await downloadViaFileSystemAPI(token, fileEntry, key, nonceBase, totalChunks, fileItem)
    } else {
        // Check if file might be too large for available memory
        if (fileEntry.size > browserCaps.estimatedMemory * 0.5) {
            await showMemoryWarning(fileEntry)
        }
        
        console.log(`Using in-memory download for ${fileEntry.name}`)
        await downloadViaBlob(token, fileEntry, key, nonceBase, totalChunks, fileItem)
    }
}

async function downloadViaFileSystemAPI(token, fileEntry, key, nonceBase, totalChunks, fileItem) {
    // Prompt user to save file
    const fileHandle = await window.showSaveFilePicker({
        suggestedName: fileEntry.name,
    })
    
    const writable = await fileHandle.createWritable()
    
    try {
        let completedChunks = 0

        // Download chunks with concurrency control (NO in-memory storage)
        await runWithConcurrency(
            Array.from({ length: totalChunks }, (_, i) => i),
            async (chunkIndex) => {
                const encrypted = await downloadChunk(token, fileEntry.index, chunkIndex)
                const nonce = generateNonce(nonceBase, chunkIndex)
                const decrypted = await crypto.subtle.decrypt(
                    { name: 'AES-GCM', iv: nonce },
                    key,
                    encrypted
                )

                // Write directly to disk (not stored in memory)
                await writable.write(new Uint8Array(decrypted))

                completedChunks++
                updateFileProgress(fileItem, completedChunks, totalChunks)
            },
            MAX_CONCURRENT_DOWNLOADS
        )
        
        // Close file to flush to disk
        await writable.close()
        
        // Verify hash by reading back from disk
        const file = await fileHandle.getFile()
        await verifyHash(file, fileEntry)
        
        // Update UI
        const progressText = fileItem.querySelector('.progress-text')
        if (progressText) progressText.textContent = 'Download complete!'
        
    } catch (error) {
        await writable.abort()
        throw error
    }
}

// In-memory blob path (Firefox/Safari/small files)
async function downloadViaBlob(token, fileEntry, key, nonceBase, totalChunks, fileItem) {
    const decryptedChunks = new Array(totalChunks)
    let completedChunks = 0

    await runWithConcurrency(
        Array.from({ length: totalChunks }, (_, i) => i),
        async (chunkIndex) => {
            const encrypted = await downloadChunk(token, fileEntry.index, chunkIndex)
            const nonce = generateNonce(nonceBase, chunkIndex)
            const decrypted = await crypto.subtle.decrypt(
                { name: 'AES-GCM', iv: nonce },
                key,
                encrypted
            )
            decryptedChunks[chunkIndex] = new Uint8Array(decrypted)
            completedChunks++
            updateFileProgress(fileItem, completedChunks, totalChunks)
        },
        MAX_CONCURRENT_DOWNLOADS
    )

    const blob = new Blob(decryptedChunks)
    await verifyHash(blob, fileEntry)

    // Trigger download
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = fileEntry.name
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
}

async function showMemoryWarning(fileEntry) {
    const fileSize = formatFileSize(fileEntry.size)
    const availableMem = formatFileSize(browserCaps.estimatedMemory)
    
    const message = `Warning: This file (${fileSize}) is very large and may use significant memory.

Available memory: ~${availableMem}
Your browser: ${browserCaps.hasFileSystemAccess ? 'Chrome/Edge' : 'Firefox/Safari'}

${browserCaps.hasFileSystemAccess ? '' : 'Recommendation: Use Chrome or Edge for files over 200MB for better memory efficiency.\n\n'}Continue download?`
    
    if (!confirm(message)) {
        throw new Error('Download cancelled by user')
    }
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
    return await retryWithExponentialBackoff(async () => {
        const response = await fetch(`/send/${token}/${fileIndex}/chunk/${chunkIndex}`)
        if (!response.ok) {
            throw new Error(`HTTP ${response.status}`)
        }
        return await response.arrayBuffer()
    }, maxRetries, `chunk ${chunkIndex}`)
}


