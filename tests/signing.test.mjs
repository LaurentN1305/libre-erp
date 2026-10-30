import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';
import { execFileSync } from 'node:child_process';
import { createPublicKey, verify } from 'node:crypto';
test('Le manifeste signé lie les octets et rejette une modification', () => {
  const dir = mkdtempSync(join(tmpdir(), 'erp-sign-test-'));
  try {
    const key = join(dir, 'test.pem'), pub = join(dir, 'test-public.txt'), payloadPath = join(dir, 'payload.json'), output = join(dir, 'manifest.json');
    const env = { ...process.env, ERP_MANIFEST_KEY_PASSWORD: 'Cle de test jetable uniquement' };
    const script = resolve('scripts/Signer-Manifeste.mjs');
    execFileSync(process.execPath, [script, 'generer', key, pub], { env });
    const payload = Buffer.from(JSON.stringify({ channel: 'stable', target: 'windows-x86_64', sequence: 1, expires_at: '2030-01-01T00:00:00Z', release: { version: '0.1.1', url: 'https://example.invalid/setup.exe', signature: 'test-only', sha256:'a'.repeat(64) } }));
    writeFileSync(payloadPath, payload);
    execFileSync(process.execPath, [script, 'signer', key, payloadPath, output], { env });
    const raw = Buffer.from(readFileSync(pub, 'utf8').trim(), 'base64');
    const publicKey = createPublicKey({ key: { kty: 'OKP', crv: 'Ed25519', x: raw.toString('base64url') }, format: 'jwk' });
    const envelope = JSON.parse(readFileSync(output, 'utf8'));
    const bytes = Buffer.from(envelope.payload, 'base64'), sig = Buffer.from(envelope.signature, 'base64');
    assert.ok(verify(null, bytes, publicKey, sig));
    assert.deepEqual(bytes, payload);
    bytes[0] ^= 1;
    assert.equal(verify(null, bytes, publicKey, sig), false);
  } finally {
    assert.ok(resolve(dir).startsWith(resolve(tmpdir()) + sep));
    rmSync(dir, { recursive: true, force: true });
  }
});
