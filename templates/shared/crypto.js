
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

// Create framed chunk [4 byte len][data]
function createFrame(data) {
    const dataArray = new Uint8Array(data)
    const frame = new Uint8Array(4 + dataArray.length)
    const view = new DataView(frame.buffer)

    // write len as 4byte BE
    view.setInt32(0, dataArray.length, false)

    // Copy data 
    frame.set(dataArray, 4)

    return frame
}

function* parseFrames(buffer) {
    while (buffer.length >= 4) {
        // read prefix
        const view = new DataView(buffer.buffer, buffer.byteOffset,  4)
        const length = view.getUint32(0) // # encrpted bytes

        if (buffer.length < 4 + length) {
            break // dont have full chunk yet
        }

        const frame = buffer.slice(4, 4 + length)
        remaining = buffer.slice(4 + length) // remove chunk

        yield { frame, remaining }
        buffer = remaining
    }

}

async function calculateHash(data) {
    const hashBuffer = await crypto.subtle.digest('SHA-256', data)
    const hashArray = Array.from(new Uint8Array(hashBuffer))
    const hashHex = hashArray.map(b => b.toString(16).padStart(2, '0')).join('')
    return hashHex
}
