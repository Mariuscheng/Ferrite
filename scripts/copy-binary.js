const fs = require('fs');
const path = require('path');
const os = require('os');

const projectRoot = path.resolve(__dirname, '..');
const binDir = path.join(projectRoot, 'bin');
const targetDir = path.join(projectRoot, 'target', 'release');
const binaryName = os.platform() === 'win32' ? 'ferrite-agent.exe' : 'ferrite-agent';
const source = path.join(targetDir, binaryName);
const dest = path.join(binDir, binaryName);

console.log(`Copying binary from ${source} to ${dest}`);

if (!fs.existsSync(binDir)) {
    fs.mkdirSync(binDir, { recursive: true });
}

if (!fs.existsSync(source)) {
    console.error('ERROR: Release binary not found!');
    console.error('Please run: cargo build --release');
    process.exit(1);
}

fs.copyFileSync(source, dest);
console.log('Binary copied successfully!');

// On non-Windows, ensure the binary is executable
if (os.platform() !== 'win32') {
    try {
        fs.chmodSync(dest, 0o755);
        console.log('Set executable permission');
    } catch (e) {
        console.warn('Failed to set executable permission:', e.message);
    }
}