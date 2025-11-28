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
        if (!manifestResponse) {
            throw new Error('Failed to fetch file list')
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
    // import nonce unique to file
    const nonceBase = urlSafeBase64ToUint8Array(fileEntry.nonce)

    // Fetch encrypted stream
    const response = await fetch(`/send/${token}/${fileEntry.index}/data`)
    if (!response.ok) {
        throw new Error(`Download failed: ${response.status}`)
    }

    // Decrypt & save
    await streamDownload(response, fileEntry.name, sessionKey, nonceBase)

}

async function streamDownload(response, filename, key, nonceBase) {
    let buffer = new Uint8Array(0)
    let counter = 0

    // create decryption transform stream
    const decryptTransform = new TransformStream({
        async transform(chunk, controller) {
            // append to buffer
            buffer = concatArrays(buffer, new Uint8Array(chunk))

            while (buffer.length >= 4) {
                const view = new DataView(buffer.buffer, buffer.byteOffset, 4)
                const frameLength = view.getInt32(0)

                if (buffer.length < 4 + frameLength) {
                    break
                }

                // wait for complete frame
                const encryptedFrame = buffer.slice(4, 4 + frameLength)
                buffer = buffer.slice(4 + frameLength)

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


