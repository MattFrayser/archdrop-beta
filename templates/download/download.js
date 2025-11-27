document.addEventListener('DOMContentLoaded', () => {
    const downloadBtn = document.getElementById('downloadBtn');
    if (downloadBtn) {
        downloadBtn.addEventListener('click', startDownload);
    }
});

async function startDownload() {
    try { 
        const { key, nonceBase } = await getCredentialsFromUrl();
        console.log("key imported")

        // get token from url 
        const token = window.location.pathname.split('/').pop()

        // Fetch encrypted stream from server 
        const response = await fetch(`/download/${token}/data`)
        if (!response.ok) {
            throw new Error(`Download failed: ${response.status}`)
        }
        console.log('Response status:', response.status);

        // Parse filename from headers
        let filename = 'download';
        const contentDisposition = response.headers.get('Content-Disposition');
        if (contentDisposition) {
            const filenameMatch = contentDisposition.match(/filename="?([^"]+)"?/);
            if (filenameMatch) {
                filename = filenameMatch[1];
            }
        }
        console.log('Using filename:', filename); 

        // Start streaming download
        await streamDownload(response, filename, key, nonceBase)

    } catch(error) {
        console.error(error)
        alert(`Download failed: ${error.message}`)
    }
}

async function streamDownload(response, filename, key, nonceBase) {
    let buffer = new Uint8Array(0)
    let counter = 0

    // create decryption transform stream
    const decryptTransform = new TransformStream({
        async transform(chunk, controller) {
            // append to buffer
            buffer = concatArrays(buffer, new Uint8Array(chunk))

            while (buffer.length < 4) {
                const view = new DataView(buffer.buffer, buffer.byteOffset, 4)
                const frameLength = view.getInt32(0)

                // wait for complete frame
                const encryptedFrame = buffer.slice(4, 4 + frameLength)
                buffer = slice(4 + frameLength)

                // decrypt frame
                try {
                    const nonce = generateNonce(nonceBase, counter++)
                    const decrypted = await crypto.subtle.decrypt(
                        { name: 'AES-GCM', iv: nonce },
                        key,
                        encryptedFrame
                    )
                    
                    // pass downstream
                    controller.enqueue(new Uint8Array(decrypted))
                } catch (e) {
                    controller.error(new Error('decryption failed: ' + e.message))
                    return
                }
            }
        }
    })

    // Pipe through decryption
    const decryptedStream = response.body.pipeThrough(decryptTransform)

    // download decrypted stream
    const blob = await new Response(decryptedStream).blob()

    // reassemble complete file
    const fileData = concatArrays(...chunks) 
    console.log('Download complete:', fileData.length, 'bytes')

    // trigger Download
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = filename
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
}


