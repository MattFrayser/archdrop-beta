const fileInput = document.getElementById('file-input');
const uploadBtn = document.querySelector('button');

fileInput.addEventListener('change', (e) => {
    const files = e.target.files;
    console.log('Files selected:', files.length);

    if (files.length > 0) {
        uploadBtn.disabled = false;
        uploadBtn.textContent = `Upload ${files.length} file${files.length > 1 ? 's' : ''}`;
    }
});

async function uploadFile() {
    const files = Array.from(fileInput.files)

        console.log(files)

        if (files.length === 0) {
            alert('Please select a file')
            return 
    }

    try {

        // zip files
        const zip = new JSZip()

        for (const file of files) {
            const path = file.webkitRelativePath || file.name
            zip.file(path, file)
        }

        // Generate zip as blob
        const zipBlob = await zip.generateAsync({
            type: 'blob',
            compression: 'DEFLATE',
            compresssionOptions: { level: 6 }
        }, (metadata) => {
            // visual updates
        })
        console.log("zip created")

        // Encrypt Zip 
        const { key, nonceBase } = await getCredentialsFromUrl()
        const encryptedFrames = await encryptZipStream(zipBlob, key, nonceBase)
        console.log("Encrypt complete")
        
        // Upload encrypted zip
        const token = window.location.pathname.split('/').pop()
        const blob = new Blob(encryptedFrames)

        const response = await fetch(`/upload/${token}/data`, {
            method: 'POST',
            body: blob,
            headers: {
                'Content-Type': 'application/octet-stream',
                'X-Filename': files.length === 1 ? files[0].name : 'upload.zip', // Send original filename
            }
        });

        if (!response.ok) {
            throw new Error(`Upload failed: ${response.status}`);
        }

        console.log("upload successfull")
    } catch (error) {
        console.error('upload error:', error)
    }
}

async function encryptZipStream(zipBlob, key, nonceBase) {
    const encryptedFrames = [];
    let counter = 0;

    // Read zip blob as stream
    const reader = zipBlob.stream().getReader();

    while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        // Encrypt chunk
        const nonce = generateNonce(nonceBase, counter++);
        const encrypted = await crypto.subtle.encrypt(
            { name: 'AES-GCM', iv: nonce },
            key,
            value
        );

        // build frame [4 byte len][data]
        const frame = createFrame(encrypted);
        encryptedFrames.push(frame);

    }

    return encryptedFrames;
}

