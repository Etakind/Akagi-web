// Called on Runtime.queryObjects(WebSocket.prototype)'s result array.
// Observe the outer protobuf name only; never copy game payloads or credentials.
function () {
    const key = '__akagiRecoveryV1';
    let registry = window[key];
    if (!registry) {
        registry = { sockets: new Map(), games: new Set() };
        window[key] = registry;
    }
    const outerName = async (data) => {
        const bytes = data instanceof Blob ? new Uint8Array(await data.arrayBuffer())
            : data instanceof ArrayBuffer ? new Uint8Array(data)
            : ArrayBuffer.isView(data) ? new Uint8Array(data.buffer, data.byteOffset, data.byteLength) : null;
        if (!bytes || bytes[0] !== 1) return '';
        let offset = 1;
        const uint = () => {
            let value = 0, shift = 0;
            for (let n = 0; n < 5 && offset < bytes.length; n++) {
                const b = bytes[offset++]; value += (b & 127) * 2 ** shift;
                if (!(b & 128)) return value;
                shift += 7;
            }
            throw new Error('invalid wrapper');
        };
        while (offset < bytes.length) {
            const tag = uint();
            if ((tag & 7) === 2) {
                const length = uint();
                if (offset + length > bytes.length) return '';
                if (tag >>> 3 === 1 && length <= 64) return new TextDecoder().decode(bytes.subarray(offset, offset + length));
                offset += length;
            } else if ((tag & 7) === 0) uint();
            else return '';
        }
        return '';
    };
    for (const [socket, observe] of registry.sockets) {
        if (socket.readyState !== WebSocket.OPEN) { socket.removeEventListener('message', observe); registry.sockets.delete(socket); registry.games.delete(socket); }
    }
    for (const socket of this) {
        if (socket.readyState !== WebSocket.OPEN || registry.sockets.has(socket)) continue;
        const observe = event => {
            outerName(event.data).then(name => {
                if (name === '.lq.ActionPrototype' && socket.readyState === WebSocket.OPEN && window[key] === registry) registry.games.add(socket);
            }).catch(() => {});
        };
        socket.addEventListener('message', observe);
        registry.sockets.set(socket, observe);
    }
    return { observed: registry.sockets.size };
}
