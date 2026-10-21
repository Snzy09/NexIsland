// Generates brand new high-resolution icons for NexIsland on Windows and Android.
// Pure Node.js (no external dependencies), using node:zlib for PNG and ICO encoding.

import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const WIN_OUT = join(root, "src-tauri", "icons");
const ANDROID_RES = resolve(root, "..", "android", "app", "src", "main", "res");

const SS = 4; // Supersampling factor for crystal-clear anti-aliasing

// ── Geometry Helpers ─────────────────────────────────────────────────────────

function insideSquircle(x, y, r, n = 3.6) {
  return Math.pow(Math.abs(x / r), n) + Math.pow(Math.abs(y / r), n) <= 1;
}

function insidePill(x, y, w, h) {
  const hw = w / 2;
  const hh = h / 2;
  const r = Math.min(hw, hh);
  const cx = Math.max(-hw + r, Math.min(hw - r, x));
  const cy = Math.max(-hh + r, Math.min(hh - r, y));
  return (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
}

function insideCircle(x, y, cx, cy, r) {
  return (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
}

function insideLineSegment(px, py, x1, y1, x2, y2, radius) {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const l2 = dx * dx + dy * dy;
  if (l2 === 0) return insideCircle(px, py, x1, y1, radius);
  let t = ((px - x1) * dx + (py - y1) * dy) / l2;
  t = Math.max(0, Math.min(1, t));
  const projX = x1 + t * dx;
  const projY = y1 + t * dy;
  return (px - projX) ** 2 + (py - projY) ** 2 <= radius * radius;
}

// ── NexIsland Icon Renderer ──────────────────────────────────────────────────

function renderNexIsland(size) {
  const px = new Uint8Array(size * size * 4);
  const cx = size / 2;
  const cy = size / 2;

  // Squircle base
  const R_base = size * 0.44;
  const R_rim = size * 0.465;

  // Top Island Pill
  const pillW = size * 0.58;
  const pillH = size * 0.18;
  const pillY = -size * 0.22;

  // Monogram N geometry (in center/lower half)
  const nW = size * 0.46;
  const nTop = -size * 0.05;
  const nBottom = size * 0.32;
  const nStroke = size * 0.07;
  const nLeft = -nW / 2;
  const nRight = nW / 2;

  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let rimHits = 0;
      let baseHits = 0;
      let islandHits = 0;
      let cameraHits = 0;
      let nHits = 0;

      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const px0 = x + (sx + 0.5) / SS - cx;
          const py0 = y + (sy + 0.5) / SS - cy;

          // Rim / glow test
          if (insideSquircle(px0, py0, R_rim)) {
            rimHits++;
          }

          // Base squircle
          if (insideSquircle(px0, py0, R_base)) {
            baseHits++;

            // Dynamic Island Pill
            if (insidePill(px0, py0 - pillY, pillW, pillH)) {
              islandHits++;

              // Camera lens & LED sensor inside the pill
              if (insideCircle(px0, py0, -pillW * 0.24, pillY, pillH * 0.22)) {
                cameraHits = 1;
              } else if (insideCircle(px0, py0, pillW * 0.26, pillY, pillH * 0.16)) {
                cameraHits = 2; // Green/cyan status indicator LED
              }
            }

            // Monogram "N" (Left pillar, diagonal bridge, right pillar)
            const inLeftCol = insideLineSegment(px0, py0, nLeft, nTop, nLeft, nBottom, nStroke);
            const inRightCol = insideLineSegment(px0, py0, nRight, nTop, nRight, nBottom, nStroke);
            const inDiag = insideLineSegment(px0, py0, nLeft, nTop, nRight, nBottom, nStroke);

            if (inLeftCol || inRightCol || inDiag) {
              nHits++;
            }
          }
        }
      }

      if (rimHits === 0) continue;

      const total = SS * SS;
      const rimA = rimHits / total;
      const baseA = baseHits / total;
      const islandA = islandHits / total;
      const nA = nHits / total;

      // Color computation
      const normX = (x - cx) / size;
      const normY = (y - cy) / size;
      const angle = Math.atan2(normY, normX); // -PI to PI
      const gradT = (angle + Math.PI) / (2 * Math.PI);

      // 1. Neon Rim Gradient (Cyan -> Violet -> Electric Blue)
      let rimR = 0, rimG = 240, rimB = 255;
      if (gradT < 0.5) {
        const u = gradT * 2;
        rimR = Math.round(0 + (168 - 0) * u);
        rimG = Math.round(240 + (85 - 240) * u);
        rimB = Math.round(255 + (247 - 255) * u);
      } else {
        const u = (gradT - 0.5) * 2;
        rimR = Math.round(168 + (0 - 168) * u);
        rimG = Math.round(85 + (210 - 85) * u);
        rimB = 255;
      }

      // 2. Obsidian Glass Base Gradient
      const baseTop = [22, 24, 38];
      const baseBottom = [8, 10, 16];
      const baseV = Math.min(1, Math.max(0, (y / size)));
      const baseCol = [
        Math.round(baseTop[0] + (baseBottom[0] - baseTop[0]) * baseV),
        Math.round(baseTop[1] + (baseBottom[1] - baseTop[1]) * baseV),
        Math.round(baseTop[2] + (baseBottom[2] - baseTop[2]) * baseV),
      ];

      // 3. Composite Base over Rim
      let finalCol = [rimR, rimG, rimB];
      let finalAlpha = rimA;

      if (baseA > 0) {
        const frac = baseA / rimA;
        finalCol = [
          Math.round(finalCol[0] * (1 - frac) + baseCol[0] * frac),
          Math.round(finalCol[1] * (1 - frac) + baseCol[1] * frac),
          Math.round(finalCol[2] * (1 - frac) + baseCol[2] * frac),
        ];
      }

      // 4. Composite Dynamic Island Pill
      if (islandA > 0) {
        // Deep pitch black island with subtle glossy border
        const islandCol = [4, 4, 6];
        finalCol = [
          Math.round(finalCol[0] * (1 - islandA) + islandCol[0] * islandA),
          Math.round(finalCol[1] * (1 - islandA) + islandCol[1] * islandA),
          Math.round(finalCol[2] * (1 - islandA) + islandCol[2] * islandA),
        ];

        if (cameraHits === 1) {
          // Dark camera glass with blue-purple reflection
          finalCol = [24, 32, 60];
        } else if (cameraHits === 2) {
          // Glowing status indicator LED
          finalCol = [52, 211, 153];
        }
      }

      // 5. Composite Monogram "N" (Electric Cyan -> Neon Purple gradient)
      if (nA > 0) {
        const nGrad = Math.min(1, Math.max(0, (x - (cx + nLeft)) / nW));
        const nCol = [
          Math.round(0 + (192 - 0) * nGrad),
          Math.round(240 + (132 - 240) * nGrad),
          Math.round(255 + (252 - 255) * nGrad),
        ];
        finalCol = [
          Math.round(finalCol[0] * (1 - nA) + nCol[0] * nA),
          Math.round(finalCol[1] * (1 - nA) + nCol[1] * nA),
          Math.round(finalCol[2] * (1 - nA) + nCol[2] * nA),
        ];
      }

      const o = (y * size + x) * 4;
      px[o] = finalCol[0];
      px[o + 1] = finalCol[1];
      px[o + 2] = finalCol[2];
      px[o + 3] = Math.round(Math.min(1, finalAlpha) * 255);
    }
  }

  return px;
}

// ── PNG & ICO Encoders ───────────────────────────────────────────────────────

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC_TABLE[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

function encodePNG(size, rgba) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;
  ihdr[9] = 6;
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) {
    raw[y * (size * 4 + 1)] = 0;
    Buffer.from(rgba.buffer, y * size * 4, size * 4).copy(raw, y * (size * 4 + 1) + 1);
  }
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function encodeICO(entries) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(0, 0);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(entries.length, 4);
  const dir = Buffer.alloc(16 * entries.length);
  let offset = header.length + dir.length;
  entries.forEach((e, i) => {
    const o = i * 16;
    dir[o] = e.size >= 256 ? 0 : e.size;
    dir[o + 1] = e.size >= 256 ? 0 : e.size;
    dir[o + 2] = 0;
    dir[o + 3] = 0;
    dir.writeUInt16LE(1, o + 4);
    dir.writeUInt16LE(32, o + 6);
    dir.writeUInt32LE(e.png.length, o + 8);
    dir.writeUInt32LE(offset, o + 12);
    offset += e.png.length;
  });
  return Buffer.concat([header, dir, ...entries.map((e) => e.png)]);
}

// ── Execution ────────────────────────────────────────────────────────────────

console.log("Generating NexIsland brand icons for Windows and Android...");

mkdirSync(WIN_OUT, { recursive: true });

const png = (size) => encodePNG(size, renderNexIsland(size));

// 1. Windows icons
const winFiles = {
  "32x32.png": png(32),
  "128x128.png": png(128),
  "128x128@2x.png": png(256),
  "icon.png": png(512),
};

for (const [name, data] of Object.entries(winFiles)) {
  writeFileSync(join(WIN_OUT, name), data);
  console.log(`[Windows] ${name} — ${data.length} bytes`);
}

const ico = encodeICO([16, 24, 32, 48, 64, 128, 256].map((size) => ({ size, png: png(size) })));
writeFileSync(join(WIN_OUT, "icon.ico"), ico);
console.log(`[Windows] icon.ico — ${ico.length} bytes`);

// 2. Android launcher icons
const androidDensities = [
  { folder: "mipmap-mdpi", size: 48 },
  { folder: "mipmap-hdpi", size: 72 },
  { folder: "mipmap-xhdpi", size: 96 },
  { folder: "mipmap-xxhdpi", size: 144 },
  { folder: "mipmap-xxxhdpi", size: 192 },
];

for (const { folder, size } of androidDensities) {
  const dir = join(ANDROID_RES, folder);
  mkdirSync(dir, { recursive: true });
  const data = png(size);
  writeFileSync(join(dir, "ic_launcher.png"), data);
  writeFileSync(join(dir, "ic_launcher_round.png"), data);
  console.log(`[Android] ${folder}/ic_launcher.png (${size}x${size}) — ${data.length} bytes`);
}

console.log("All NexIsland icons successfully generated!");
