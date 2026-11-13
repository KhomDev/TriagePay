// Keccak-256 (the pre-NIST padding, not SHA3-256). Used to build an
// ERC-20 selector. No dependency.

const MASK = (1n << 64n) - 1n;
const RC = [
  0x0000000000000001n, 0x0000000000008082n, 0x800000000000808an, 0x8000000080008000n,
  0x000000000000808bn, 0x0000000080000001n, 0x8000000080008081n, 0x8000000000008009n,
  0x000000000000008an, 0x0000000000000088n, 0x0000000080008009n, 0x000000008000000an,
  0x000000008000808bn, 0x800000000000008bn, 0x8000000000008089n, 0x8000000000008003n,
  0x8000000000008002n, 0x8000000000000080n, 0x000000000000800an, 0x800000008000000an,
  0x8000000080008081n, 0x8000000000008080n, 0x0000000080000001n, 0x8000000080008008n,
].map((value) => value & MASK);
const ROTC = [1, 3, 6, 10, 15, 21, 28, 36, 45, 55, 2, 14, 27, 41, 56, 8, 25, 43, 62, 18, 39, 61, 20, 44];
const PILN = [10, 7, 11, 17, 18, 3, 5, 16, 8, 21, 24, 4, 15, 23, 19, 13, 12, 2, 20, 14, 22, 9, 6, 1];

function rotl(x, n) {
  const shift = BigInt(n);
  return ((x << shift) | (x >> (64n - shift))) & MASK;
}

function keccakf(state) {
  const bc = [0n, 0n, 0n, 0n, 0n];
  for (let round = 0; round < 24; round++) {
    for (let i = 0; i < 5; i++) {
      bc[i] = state[i] ^ state[i + 5] ^ state[i + 10] ^ state[i + 15] ^ state[i + 20];
    }
    for (let i = 0; i < 5; i++) {
      const t = bc[(i + 4) % 5] ^ rotl(bc[(i + 1) % 5], 1);
      for (let j = 0; j < 25; j += 5) state[j + i] = (state[j + i] ^ t) & MASK;
    }
    let t = state[1];
    for (let i = 0; i < 24; i++) {
      const j = PILN[i];
      bc[0] = state[j];
      state[j] = rotl(t, ROTC[i]);
      t = bc[0];
    }
    for (let j = 0; j < 25; j += 5) {
      for (let i = 0; i < 5; i++) bc[i] = state[j + i];
      for (let i = 0; i < 5; i++) {
        state[j + i] = (state[j + i] ^ ((~bc[(i + 1) % 5]) & bc[(i + 2) % 5])) & MASK;
      }
    }
    state[0] = (state[0] ^ RC[round]) & MASK;
  }
}

/** Keccak-256 of `bytes`, as a `Buffer`. */
export function keccak256(bytes) {
  const input = Buffer.isBuffer(bytes) ? bytes : Buffer.from(bytes);
  const rate = 136;
  const state = Array.from({ length: 25 }, () => 0n);
  let offset = 0;
  while (offset + rate <= input.length) {
    for (let i = 0; i < rate; i++) {
      const lane = Math.floor(i / 8);
      state[lane] = (state[lane] ^ (BigInt(input[offset + i]) << BigInt((i % 8) * 8))) & MASK;
    }
    keccakf(state);
    offset += rate;
  }
  const block = Buffer.alloc(rate);
  input.copy(block, 0, offset);
  block[input.length - offset] ^= 0x01;
  block[rate - 1] ^= 0x80;
  for (let i = 0; i < rate; i++) {
    const lane = Math.floor(i / 8);
    state[lane] = (state[lane] ^ (BigInt(block[i]) << BigInt((i % 8) * 8))) & MASK;
  }
  keccakf(state);
  const out = Buffer.alloc(32);
  for (let i = 0; i < 32; i++) {
    const lane = Math.floor(i / 8);
    out[i] = Number((state[lane] >> BigInt((i % 8) * 8)) & 0xffn);
  }
  return out;
}

/** Keccak-256 of `bytes`, as a hex string without a `0x` prefix. */
export function keccak256Hex(bytes) {
  return keccak256(bytes).toString("hex");
}
