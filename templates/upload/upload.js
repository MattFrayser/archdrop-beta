const uploadArea = document.getElementById('uploadArea');
const fileInput = document.getElementById('fileInput');
const fileList = document.getElementById('fileList');
const uploadBtn = document.getElementById('uploadBtn');
let selectedFiles = [];

// Click to upload
uploadArea.addEventListener('click', () => fileInput.click());

// File selected
fileInput.addEventListener('change', (e) => {
    handleFiles(Array.from(e.target.files));
});

// Drag and drop
uploadArea.addEventListener('dragover', (e) => {
    e.preventDefault();
    uploadArea.classList.add('dragover');
});

uploadArea.addEventListener('dragleave', () => {
    uploadArea.classList.remove('dragover');
});

uploadArea.addEventListener('drop', (e) => {
    e.preventDefault();
    uploadArea.classList.remove('dragover');
    handleFiles(Array.from(e.dataTransfer.files));
});

// Handle multiple files
function handleFiles(files) {
    if (!files || files.length === 0) return;

    // Add new files to existing selection
    selectedFiles = [...selectedFiles, ...files];
    
    updateFileList();
}

// Create file item element
function createFileItem(file, index) {
    const item = document.createElement('div');
    item.className = 'file-item';

    // File icon
    const icon = document.createElement('div');
    icon.className = 'file-icon';
    icon.innerHTML = `
        <svg viewBox="0 0 24 24" stroke-linecap="round" stroke-linejoin="round">
            <path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"></path>
            <polyline points="13 2 13 9 20 9"></polyline>
        </svg>
    `;

    // File details container
    const details = document.createElement('div');
    details.className = 'file-details';

    const name = document.createElement('div');
    name.className = 'file-name';
    name.textContent = file.name;

    const size = document.createElement('div');
    size.className = 'file-size';
    size.textContent = formatFileSize(file.size);

    details.appendChild(name);
    details.appendChild(size);

    // Remove button
    const removeBtn = document.createElement('button');
    removeBtn.className = 'remove-file-btn';
    removeBtn.type = 'button';
    removeBtn.innerHTML = `
        <svg viewBox="0 0 24 24" stroke-linecap="round" stroke-linejoin="round">
            <line x1="18" y1="6" x2="6" y2="18"></line>
            <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
    `;
    removeBtn.addEventListener('click', () => removeFile(index));

    item.appendChild(icon);
    item.appendChild(details);
    item.appendChild(removeBtn);

    return item;
}

// Create summary element
function createSummary(fileCount, totalSize) {
    const summary = document.createElement('div');
    summary.className = 'file-list-summary';
    summary.textContent = `${fileCount} files selected • Total: ${formatFileSize(totalSize)}`;
    return summary;
}

// Update the file list UI
function updateFileList() {
    // Clear existing content
    fileList.innerHTML = '';

    if (selectedFiles.length === 0) {
        fileList.classList.remove('show');
        uploadBtn.classList.remove('show');
        return;
    }

    fileList.classList.add('show');
    uploadBtn.classList.add('show');

    // Add each file
    selectedFiles.forEach((file, index) => {
        const fileItem = createFileItem(file, index);
        fileList.appendChild(fileItem);
    });

    // Add summary if multiple files
    if (selectedFiles.length > 1) {
        const totalSize = selectedFiles.reduce((sum, file) => sum + file.size, 0);
        const summary = createSummary(selectedFiles.length, totalSize);
        fileList.appendChild(summary);
    }

    // Update button text
    uploadBtn.textContent = selectedFiles.length === 1 
        ? 'Upload File' 
        : `Upload ${selectedFiles.length} Files`;
}

// Remove individual file
function removeFile(index) {
    selectedFiles.splice(index, 1);
    
    if (selectedFiles.length === 0) {
        fileInput.value = '';
    }
    
    updateFileList();
}

// Upload files
async function uploadFiles(selectedFiles) {
    if (selectedFiles.length === 0) {
        alert('Please select files');
        return;
    }

    const uploadBtn = document.getElementById('uploadBtn');
    uploadBtn.disabled = true;

    try {
        const { key, nonceBase } = await getCredentialsFromUrl();
        const token = window.location.pathname.split('/').pop();

        // Upload each file individually
        for (let i = 0; i < selectedFiles.length; i++) {
            const file = selectedFiles[i];
            
            uploadBtn.textContent = `Uploading ${i + 1}/${selectedFiles.length}: ${file.name}`;
            
            await uploadSingleFile(file, token, key, nonceBase, i, selectedFiles.length);
        }

        uploadBtn.textContent = 'Upload Complete!';
        setTimeout(() => {
            uploadBtn.disabled = false;
            uploadBtn.textContent = 'Upload';
        }, 2000);

    } catch (error) {
        console.error('Upload error:', error);
        alert(`Upload failed: ${error.message}`);
        uploadBtn.disabled = false;
    }
}

async function uploadSingleFile(file, token, key, nonceBase, index, total) {
    // Read file in chunks for encryption
    const CHUNK_SIZE = 5 * 1024 * 1024; // 5MB chunks
    const encryptedChunks = [];
    let counter = 0;

    // Encrypt in chunks
    for (let offset = 0; offset < file.size; offset += CHUNK_SIZE) {
        const chunk = file.slice(offset, Math.min(offset + CHUNK_SIZE, file.size));
        const arrayBuffer = await chunk.arrayBuffer();
        
        const nonce = generateNonce(nonceBase, counter++);
        const encrypted = await crypto.subtle.encrypt(
            { name: 'AES-GCM', iv: nonce },
            key,
            arrayBuffer
        );
        
        encryptedChunks.push(createFrame(encrypted));
    }

    // Create blob from encrypted chunks
    const encryptedBlob = new Blob(encryptedChunks);

    // relative path for rebuilding
    const relativePath = file.webkitRelativePath || file.name

    // Send via multipart
    const formData = new FormData();
    formData.append('file', encryptedBlob, file.name);
    formData.append('fileIndex', index);
    formData.append('totalFiles', total);
    formData.append('relativePath', relativePath)

    const response = await fetch(`/upload/${token}/file/${index}`, {
        method: 'POST',
        body: formData
    });

    if (!response.ok) {
        throw new Error(`Upload failed for ${file.name}: ${response.status}`);
    }
}

function formatFileSize(bytes) {
    if (bytes === 0) return '0 Bytes';
    const k = 1024;
    const sizes = ['Bytes', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return Math.round(bytes / Math.pow(k, i) * 100) / 100 + ' ' + sizes[i];
}

