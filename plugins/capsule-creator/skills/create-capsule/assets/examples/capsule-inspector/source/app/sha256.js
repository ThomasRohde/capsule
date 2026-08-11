const SHA256_INITIAL = new Uint32Array([
  0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
  0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
]);

const SHA256_ROUND = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
  0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
  0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
  0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
  0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
  0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
  0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
  0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
  0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

function rotateRight(value, bits) {
  return (value >>> bits) | (value << (32 - bits));
}

function portableSha256(bytes) {
  const state = new Uint32Array(SHA256_INITIAL);
  const words = new Uint32Array(64);

  function compress(block, offset) {
    for (let index = 0; index < 16; index += 1) {
      const cursor = offset + index * 4;
      words[index] = (
        (block[cursor] << 24)
        | (block[cursor + 1] << 16)
        | (block[cursor + 2] << 8)
        | block[cursor + 3]
      ) >>> 0;
    }
    for (let index = 16; index < 64; index += 1) {
      const previous = words[index - 15];
      const recent = words[index - 2];
      const sigma0 = rotateRight(previous, 7) ^ rotateRight(previous, 18) ^ (previous >>> 3);
      const sigma1 = rotateRight(recent, 17) ^ rotateRight(recent, 19) ^ (recent >>> 10);
      words[index] = (words[index - 16] + sigma0 + words[index - 7] + sigma1) >>> 0;
    }

    let a = state[0];
    let b = state[1];
    let c = state[2];
    let d = state[3];
    let e = state[4];
    let f = state[5];
    let g = state[6];
    let h = state[7];
    for (let index = 0; index < 64; index += 1) {
      const sum1 = rotateRight(e, 6) ^ rotateRight(e, 11) ^ rotateRight(e, 25);
      const choice = (e & f) ^ (~e & g);
      const temporary1 = (h + sum1 + choice + SHA256_ROUND[index] + words[index]) >>> 0;
      const sum0 = rotateRight(a, 2) ^ rotateRight(a, 13) ^ rotateRight(a, 22);
      const majority = (a & b) ^ (a & c) ^ (b & c);
      const temporary2 = (sum0 + majority) >>> 0;
      h = g;
      g = f;
      f = e;
      e = (d + temporary1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (temporary1 + temporary2) >>> 0;
    }

    state[0] = (state[0] + a) >>> 0;
    state[1] = (state[1] + b) >>> 0;
    state[2] = (state[2] + c) >>> 0;
    state[3] = (state[3] + d) >>> 0;
    state[4] = (state[4] + e) >>> 0;
    state[5] = (state[5] + f) >>> 0;
    state[6] = (state[6] + g) >>> 0;
    state[7] = (state[7] + h) >>> 0;
  }

  let offset = 0;
  while (offset + 64 <= bytes.byteLength) {
    compress(bytes, offset);
    offset += 64;
  }

  const remaining = bytes.byteLength - offset;
  const tail = new Uint8Array(remaining < 56 ? 64 : 128);
  tail.set(bytes.subarray(offset));
  tail[remaining] = 0x80;
  const bitLengthHigh = Math.floor(bytes.byteLength / 0x20000000) >>> 0;
  const bitLengthLow = (bytes.byteLength << 3) >>> 0;
  const lengthOffset = tail.byteLength - 8;
  tail[lengthOffset] = bitLengthHigh >>> 24;
  tail[lengthOffset + 1] = bitLengthHigh >>> 16;
  tail[lengthOffset + 2] = bitLengthHigh >>> 8;
  tail[lengthOffset + 3] = bitLengthHigh;
  tail[lengthOffset + 4] = bitLengthLow >>> 24;
  tail[lengthOffset + 5] = bitLengthLow >>> 16;
  tail[lengthOffset + 6] = bitLengthLow >>> 8;
  tail[lengthOffset + 7] = bitLengthLow;
  for (let cursor = 0; cursor < tail.byteLength; cursor += 64) compress(tail, cursor);

  return [...state].map((value) => value.toString(16).padStart(8, "0")).join("");
}

export async function sha256Hex(data) {
  const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
  const digest = globalThis.crypto?.subtle?.digest;
  if (typeof digest === "function") {
    try {
      const result = await digest.call(globalThis.crypto.subtle, "SHA-256", bytes);
      return [...new Uint8Array(result)]
        .map((value) => value.toString(16).padStart(2, "0"))
        .join("");
    } catch (_) {
      // Some embedded WebViews expose a partial Web Crypto object. The
      // deterministic local implementation below keeps inspection available.
    }
  }
  return portableSha256(bytes);
}

export { portableSha256 };
