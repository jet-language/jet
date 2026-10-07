// Build-time constants for the private bootstrap host, never runtime selection.
import { createHash } from 'node:crypto';

export function installRelative(value) {
  if (typeof value !== 'string' || !value || value.split('/').some(part => !/^[A-Za-z0-9_.-]+$/.test(part) || part === '.' || part === '..')) throw Error('libraryInstallRelative must be a nonescaping ASCII install-relative path');
  return value;
}

export function embeddedAdmission(receiptBytes, librarySHA256, relative) {
  installRelative(relative);
  if (!/^[a-f0-9]{64}$/.test(librarySHA256)) throw Error('missing produced library SHA-256');
  const receiptSHA256 = createHash('sha256').update(receiptBytes).digest('hex');
  const hashArray = hex => [...Buffer.from(hex, 'hex')].map(byte => `0x${byte.toString(16).padStart(2, '0')}`).join(', ');
  const chunks = [];
  for (let at = 0; at < receiptBytes.length; at += 24) chunks.push(`    ${[...receiptBytes.subarray(at, at + 24)].join(', ')},`);
  return {
    receiptSHA256,
    source: `// Generated from the same sealed runtime candidate as the installed .so.\n// Include in a private host module; no user environment or runtime receipt file.\npub(crate) const LIBRARY_INSTALL_RELATIVE: &str = ${JSON.stringify(relative)};\npub(crate) const EXPECTED_LIBRARY_SHA256: [u8; 32] = [${hashArray(librarySHA256)}];\npub(crate) const EXPECTED_RECEIPT_SHA256: [u8; 32] = [${hashArray(receiptSHA256)}];\npub(crate) const RECEIPT_BYTES: &[u8] = &[\n${chunks.join('\n')}\n];\n`,
  };
}
