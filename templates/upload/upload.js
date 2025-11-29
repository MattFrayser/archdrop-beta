const uploadArea = document.getElementById('uploadArea');
const fileInput = document.getElementById('fileInput');
const fileList = document.getElementById('fileList');
const uploadBtn = document.getElementById('uploadBtn');
let selectedFiles = [];

// Click to upload
uploadArea.addEventListener('click', () => fileInput.click())

// File selected
fileInput.addEventListener('change', (e) => {
    handleFiles(Array.from(e.target.files))
});

// Drag and drop
uploadArea.addEventListener('dragover', (e) => {
    e.preventDefault()
    uploadArea.classList.add('dragover')
});

uploadArea.addEventListener('dragleave', () => {
    uploadArea.classList.remove('dragover')
});

uploadArea.addEventListener('drop', (e) => {
    e.preventDefault()
    uploadArea.classList.remove('dragover')
    handleFiles(Array.from(e.dataTransfer.files))
});

// Handle multiple files
function handleFiles(files) {
    if (!files || files.length === 0) return

    // Add new files to existing selection
    selectedFiles = [...selectedFiles, ...files]
    
    updateFileList()
}

// Create file item element
function createFileItem(file, index) {
    const item = document.createElement('div')
    item.className = 'file-item'

    // File icon
    const icon = document.createElement('div')
    icon.className = 'file-icon'
    icon.innerHTML = `
        <svg viewBox="0 0 24 24" stroke-linecap="round" stroke-linejoin="round">
            <path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"></path>
            <polyline points="13 2 13 9 20 9"></polyline>
        </svg>
    `;

    // File details container
    const details = document.createElement('div')
    details.className = 'file-details'

    const name = document.createElement('div')
    name.className = 'file-name'
    name.textContent = file.name

    const size = document.createElement('div')
    size.className = 'file-size'
    size.textContent = formatFileSize(file.size)

    details.appendChild(name)
    details.appendChild(size)

    // Remove button
    const removeBtn = document.createElement('button')
    removeBtn.className = 'remove-file-btn'
    removeBtn.type = 'button'
    removeBtn.innerHTML = `
        <svg viewBox="0 0 24 24" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="6" x2="6" y2="18"></line>
            <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
    `;
    removeBtn.addEventListener('click', () => removeFile(index))

    item.appendChild(icon)
    item.appendChild(details)
    item.appendChild(removeBtn)

    return item;
}

// Create summary element
function createSummary(fileCount, totalSize) {
    const summary = document.createElement('div')
    summary.className = 'file-list-summary'
    summary.textContent = `${fileCount} files selected • Total: ${formatFileSize(totalSize)}`
    return summary
}

// Update the file list UI
function updateFileList() {
    // Clear existing content
    fileList.innerHTML = ''

    if (selectedFiles.length === 0) {
        fileList.classList.remove('show')
        uploadBtn.classList.remove('show')
        return
    }

    fileList.classList.add('show')
    uploadBtn.classList.add('show')

    // Add each file
    selectedFiles.forEach((file, index) => {
        const fileItem = createFileItem(file, index)
        fileList.appendChild(fileItem)
    })

    // Add summary if multiple files
    if (selectedFiles.length > 1) {
        const totalSize = selectedFiles.reduce((sum, file) => sum + file.size, 0)
        const summary = createSummary(selectedFiles.length, totalSize)
        fileList.appendChild(summary)
    }

    // Update button text
    uploadBtn.textContent = selectedFiles.length === 1 
        ? 'Upload File' 
        : `Upload ${selectedFiles.length} Files`
}

// Remove individual file
function removeFile(index) {
    selectedFiles.splice(index, 1)
    
    if (selectedFiles.length === 0) {
        fileInput.value = ''
    }
    
    updateFileList()
}


async function uploadFiles(selectedFiles) {
    if (selectedFiles.length === 0) {
        alert('Please select files')
        return
    }

    const uploadBtn = document.getElementById('uploadBtn')
    uploadBtn.disabled = true

    try {
        const { key, nonceBase } = await getCredentialsFromUrl()
        const token = window.location.pathname.split('/').pop()

        for (let i = 0; i < selectedFiles.length; i++) {
            const file = selectedFiles[i]
            const relativePath = file.webkitRelativePath || file.name;
            await uploadSingleFile(file, relativePath, token, key, nonceBase);
        }
        const complete = await fetch(`/receive/${token}/complete`)
        if (!complete.ok) {
            alert("Upload failed to complete")
        }

        uploadBtn.textContent = 'Upload Complete!'
    } catch (error) {
        alert(`Upload failed: ${error.message}`)
        uploadBtn.disabled = false
    }
}

async function uploadSingleFile(file, relativePath, token, key, nonceBase) {
    const CHUNK_SIZE = 1024 * 1024;  // 1MB chunks (increased from 64KB)
    const MAX_CONCURRENT_UPLOADS = 8; // Parallel upload limit
    const totalChunks = Math.ceil(file.size / CHUNK_SIZE)

    console.log(`Uploading: ${relativePath} (${totalChunks} chunks)`);

    // Prepare all chunk upload tasks
    const chunkIndexes = Array.from({ length: totalChunks }, (_, i) => i)

    // Process chunk upload
    const processChunk = async (chunkIndex) => {
        const start = chunkIndex * CHUNK_SIZE
        const end = Math.min(start + CHUNK_SIZE, file.size)
        const chunkBlob = file.slice(start, end)
        const chunkData = await chunkBlob.arrayBuffer()

        // Encrypt chunk
        const nonce = generateNonce(nonceBase, chunkIndex);
        const encrypted = await crypto.subtle.encrypt(
            { name: 'AES-GCM', iv: nonce },
            key,
            chunkData
        );

        // Create FormData with chunk and metadata
        const formData = new FormData();
        formData.append('chunk', new Blob([encrypted]));
        formData.append('relativePath', relativePath);
        formData.append('fileName', file.name);
        formData.append('chunkIndex', chunkIndex.toString());
        formData.append('totalChunks', totalChunks.toString());
        formData.append('fileSize', file.size.toString());

        if (chunkIndex === 0) {
            const nonceBase64 = arrayBufferToBase64(nonceBase);
            formData.append('nonce', nonceBase64);
        }

        // Upload chunk
        await uploadChunk(token, formData, chunkIndex, relativePath);
    }

    // Upload chunks in parallel with concurrency limit
    await runWithConcurrency(chunkIndexes, processChunk, MAX_CONCURRENT_UPLOADS)

    // Finalize (merge chunks)
    await finalizeFile(token, relativePath);
}

async function getCompletedChunks(token, relativePath) {
    try {
        const url = `/receive/${token}/status?${new URLSearchParams({ relativePath })}`;
        const response = await fetch(url);

        if (!response.ok) return []

        const data = await response.json()
        return data.completed_chunks || []
    
    } catch (error) {
        return []
    }
}

async function uploadChunk(token, formData, chunkIndex, relativePath, maxRetries = 3) {
    for (let attempt = 0; attempt < maxRetries; attempt++) {
        try {
            const response = await fetch(`/receive/${token}/chunk`, {
                method: 'POST',
                body: formData
            });
            
            if (response.ok) {
                console.log(`Chunk ${chunkIndex} of ${relativePath} GOOD`);
                return;
            }
            
            throw new Error(`Upload failed: ${response.status}`);
        } catch (e) {
            if (attempt === maxRetries - 1) {
                console.error(`Failed to upload chunk ${chunkIndex} of ${relativePath}:`, e);
                throw e;
            }
            // Exponential backoff: 1s, 2s, 4s
            await new Promise(r => setTimeout(r, 1000 * Math.pow(2, attempt)));
            console.log(`Retrying chunk ${chunkIndex} (attempt ${attempt + 2}/${maxRetries})...`);
        }
    }
}

async function finalizeFile(token, relativePath) {
    const formData = new FormData();
    formData.append('relativePath', relativePath);
    
    const response = await fetch(`/receive/${token}/finalize`, {
        method: 'POST',
        body: formData
    });
    
    if (!response.ok) {
        throw new Error(`Failed to finalize ${relativePath}`);
    }
    
    console.log(`✓ Completed: ${relativePath}`);
}

function formatFileSize(bytes) {
    if (bytes === 0) return '0 Bytes';
    const k = 1024;
    const sizes = ['Bytes', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return Math.round(bytes / Math.pow(k, i) * 100) / 100 + ' ' + sizes[i];
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

