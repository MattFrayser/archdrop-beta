
function urlSafeBase64ToUint8Array(str) {
    // Convert URL-safe base64 to standard base64
    let base64 = str.replace(/-/g, '+').replace(/_/g, '/')
    
    // Add padding if needed (Rust uses URL_SAFE_NO_PAD, so padding may be missing)
    // Base64 padding: length mod 4 determines padding
    const padLength = (4 - (base64.length % 4)) % 4
    base64 += '='.repeat(padLength)

    const binaryString = atob(base64)
    const bytes = new Uint8Array(binaryString.length)

    for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i)
    }

    return bytes
}

function arrayBufferToBase64(buffer) {
    // Convert Uint8Array to base64 string
    const bytes = new Uint8Array(buffer)
    let binary = ''
    for (let i = 0; i < bytes.length; i++) {
        binary += String.fromCharCode(bytes[i])
    }
    // Convert to URL-safe base64 (no padding) to match Rust's URL_SAFE_NO_PAD
    const base64 = btoa(binary)
        .replace(/\+/g, '-')  // Replace + with -
        .replace(/\//g, '_')  // Replace / with _
        .replace(/=+$/, '');  // Remove padding
    return base64
}

// Construct nonce to match Rusts EncryptorBE32
// [7 byte base][4 byte counter][1 byte last flag]
function generateNonce(nonceBase64, counter) {
    const nonce = new Uint8Array(12)
    nonce.set(nonceBase64,  0) // first 7 bytes


    // last 5 bytes (4 + last flag)
    const view = new DataView(nonce.buffer)
    view.setUint32(7, counter, false) // false = BE32

    return nonce
}

function concatArrays(...arrays) {
    const totalLength = arrays.reduce((sum, arr) => sum + arr.length, 0);
    const result = new Uint8Array(totalLength);
    let offset = 0;
    for (const arr of arrays) {
        result.set(arr, offset);
        offset += arr.length;
    }
    return result;
}

async function getCredentialsFromUrl() {
    const fragment = window.location.hash.substring(1) // remove #
    const params = new URLSearchParams(fragment)
    const keyBase64 = params.get('key')
    const nonceBase64 = params.get('nonce')

    if (!keyBase64 || !nonceBase64) {
        throw new Error('Missing encryption key')
    }

    // Clear url fragment immediatly after getti 
    // window.location.replace(window.location.href.split('#')[0])

    // base64 -> string -> byte array
    const keyData = urlSafeBase64ToUint8Array(keyBase64);
    const nonceData = urlSafeBase64ToUint8Array(nonceBase64);

    const key = await crypto.subtle.importKey(
        'raw',
        keyData,
        { name: 'AES-GCM' },
        false,
        ['encrypt', 'decrypt']
    )

    return { key, nonceBase: nonceData }


}

async function calculateHash(data) {
    const hashBuffer = await crypto.subtle.digest('SHA-256', data)
    const hashArray = Array.from(new Uint8Array(hashBuffer))
    const hashHex = hashArray.map(b => b.toString(16).padStart(2, '0')).join('')
    return hashHex
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
