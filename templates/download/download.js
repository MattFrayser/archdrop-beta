const CHUNK_SIZE = 1024 * 1024 // 1MB (increased from 64KB for better throughput)
const MAX_MEMORY = 100 * 1024 * 1024 // 100MB
const MAX_CONCURRENT_DOWNLOADS = 8 // Parallel chunk download limit

document.addEventListener('DOMContentLoaded', async () => {
    const downloadBtn = document.getElementById('downloadBtn');
    if (downloadBtn) {
        downloadBtn.addEventListener('click', startDownload);
    }

    // Load manifest and display files
    try {
        const { key } = await getCredentialsFromUrl()
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
        const item = createFileItem(file, index)
        fileList.appendChild(item)
    })
}

function createFileItem(file, index) {
    const item = document.createElement('div')
    item.className = 'file-item'
    item.dataset.fileIndex = index

    const icon = document.createElement('div')
    icon.className = 'file-icon'
    icon.innerHTML = `
        <svg viewBox="0 0 24 24" stroke-linecap="round" stroke-linejoin="round">
            <path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"></path>
            <polyline points="13 2 13 9 20 9"></polyline>
        </svg>
    `

    const details = document.createElement('div')
    details.className = 'file-details'

    const name = document.createElement('div')
    name.className = 'file-name'
    name.textContent = file.name

    const size = document.createElement('div')
    size.className = 'file-size'
    size.textContent = formatFileSize(file.size)

    const progress = document.createElement('div')
    progress.className = 'file-progress'
    progress.innerHTML = `
        <div class="progress-bar-container">
            <div class="progress-bar"></div>
        </div>
        <div class="progress-text">Ready to download</div>
    `

    details.appendChild(name)
    details.appendChild(size)
    details.appendChild(progress)

    item.appendChild(icon)
    item.appendChild(details)

    return item
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

        // Download each file
        for (let i = 0; i < manifest.files.length; i++) {
            const fileEntry = manifest.files[i]
            const fileItem = fileItems[i]

            fileItem.classList.add('downloading')
            await downloadSingleFile(token, fileEntry, key, fileItem)
            fileItem.classList.remove('downloading')
            fileItem.classList.add('completed')
        }

        // Notify server that download is complete (for TUI progress)
        await fetch(`/send/${token}/complete`, { method: 'POST' })

        const downloadBtn = document.getElementById('downloadBtn')
        downloadBtn.textContent = 'Download Complete!'

    } catch(error) {
        console.error(error)
        alert(`Download failed: ${error.message}`)
    }
}

async function downloadSingleFile(token, fileEntry, sessionKey, fileItem) {
    const nonceBase = urlSafeBase64ToUint8Array(fileEntry.nonce)
    const totalChunks = Math.ceil(fileEntry.size / CHUNK_SIZE)

    // Large file -> Use File System Access API
    if (fileEntry.size > MAX_MEMORY && 'showSaveFilePicker' in window) {
        await downloadLargeFile(token, fileEntry, sessionKey, nonceBase, totalChunks, fileItem)
    } else {
        await downloadSmallFile(token, fileEntry, sessionKey, nonceBase, totalChunks, fileItem)
    }
}

async function downloadLargeFile(token, fileEntry, key, nonceBase, totalChunks, fileItem) {
    const fileHandle = await window.showSaveFilePicker({
        suggestedName: fileEntry.name,
    });

    const writable = await fileHandle.createWritable()

    try {
        // Store decrypted chunks in order for hash verification
        const decryptedChunks = new Array(totalChunks)

        // Track progress
        let completedChunks = 0
        const updateProgress = () => {
            const percent = Math.round((completedChunks / totalChunks) * 100)
            const progressBar = fileItem.querySelector('.progress-bar')
            const progressText = fileItem.querySelector('.progress-text')
            if (progressBar) progressBar.style.width = `${percent}%`
            if (progressText) progressText.textContent = `${completedChunks}/${totalChunks} chunks (${percent}%)`
        }

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
                // Update progress
                completedChunks++
                updateProgress()
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

async function downloadSmallFile(token, fileEntry, key, nonceBase, totalChunks, fileItem) {
    // Store decrypted chunks in order
    const decryptedChunks = new Array(totalChunks)

    // Track progress
    let completedChunks = 0
    const updateProgress = () => {
        const percent = Math.round((completedChunks / totalChunks) * 100)
        const progressBar = fileItem.querySelector('.progress-bar')
        const progressText = fileItem.querySelector('.progress-text')
        if (progressBar) progressBar.style.width = `${percent}%`
        if (progressText) progressText.textContent = `${completedChunks}/${totalChunks} chunks (${percent}%)`
    }

    // Parallel chunk downloads with concurrency limit
    await downloadChunksParallel(
        token,
        fileEntry.index,
        totalChunks,
        key,
        nonceBase,
        async (chunkIndex, decryptedData) => {
            decryptedChunks[chunkIndex] = decryptedData
            // Update progress
            completedChunks++
            updateProgress()
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


