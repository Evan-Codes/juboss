import fs from 'node:fs';
import path from 'node:path';
import { PNG } from 'pngjs';

const inputDir = process.argv[2];

if (!inputDir) {
  console.error('Usage: node scripts/remove-connected-white-bg.mjs <frames-dir>');
  process.exit(1);
}

const files = fs
  .readdirSync(inputDir)
  .filter((file) => /^frame_\d+\.png$/.test(file))
  .sort();

function isBackgroundCandidate(data, index) {
  const r = data[index];
  const g = data[index + 1];
  const b = data[index + 2];
  const a = data[index + 3];
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

  return a < 16 || (luma >= 238 && max - min <= 24);
}

function isNearWhite(data, index) {
  const r = data[index];
  const g = data[index + 1];
  const b = data[index + 2];
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

  return luma >= 228 && max - min <= 34;
}

function processFrame(file) {
  const filePath = path.join(inputDir, file);
  const png = PNG.sync.read(fs.readFileSync(filePath));
  const { width, height, data } = png;
  const total = width * height;
  const visited = new Uint8Array(total);
  const background = new Uint8Array(total);
  const queue = [];

  function enqueue(x, y) {
    if (x < 0 || y < 0 || x >= width || y >= height) return;
    const pixel = y * width + x;
    if (visited[pixel]) return;
    visited[pixel] = 1;

    if (isBackgroundCandidate(data, pixel * 4)) {
      background[pixel] = 1;
      queue.push(pixel);
    }
  }

  for (let x = 0; x < width; x += 1) {
    enqueue(x, 0);
    enqueue(x, height - 1);
  }

  for (let y = 0; y < height; y += 1) {
    enqueue(0, y);
    enqueue(width - 1, y);
  }

  for (let cursor = 0; cursor < queue.length; cursor += 1) {
    const pixel = queue[cursor];
    const x = pixel % width;
    const y = Math.floor(pixel / width);

    enqueue(x + 1, y);
    enqueue(x - 1, y);
    enqueue(x, y + 1);
    enqueue(x, y - 1);
  }

  const soften = [];
  for (let pixel = 0; pixel < total; pixel += 1) {
    if (!background[pixel]) continue;
    const index = pixel * 4;
    data[index + 3] = 0;

    const x = pixel % width;
    const y = Math.floor(pixel / width);
    const neighbors = [
      [x + 1, y],
      [x - 1, y],
      [x, y + 1],
      [x, y - 1]
    ];

    for (const [nx, ny] of neighbors) {
      if (nx < 0 || ny < 0 || nx >= width || ny >= height) continue;
      const neighbor = ny * width + nx;
      if (!background[neighbor] && isNearWhite(data, neighbor * 4)) {
        soften.push(neighbor);
      }
    }
  }

  for (const pixel of soften) {
    const index = pixel * 4;
    data[index + 3] = Math.min(data[index + 3], 96);
  }

  fs.writeFileSync(filePath, PNG.sync.write(png));
}

for (const file of files) {
  processFrame(file);
}

console.log(`Processed ${files.length} frames in ${inputDir}`);
