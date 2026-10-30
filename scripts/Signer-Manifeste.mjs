// Outil éditeur. Aucune clé privée n'est embarquée dans l'application.
// node scripts/Signer-Manifeste.mjs generer chemin-cle-privee.pem chemin-cle-publique.txt
// node scripts/Signer-Manifeste.mjs signer chemin-cle-privee.pem payload.json manifeste.json
import { generateKeyPairSync, sign, createPrivateKey } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
const [mode, keyPath, input, output] = process.argv.slice(2);
if (mode === 'generer' && keyPath && input) {
  // Phrase fournie par coffre CI ou processus parent, jamais sur la ligne de commande.
  const password = process.env.ERP_MANIFEST_KEY_PASSWORD;
  if (!password || password.length < 12) throw new Error('ERP_MANIFEST_KEY_PASSWORD doit contenir au moins 12 caractères.');
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const privatePem = privateKey.export({ type: 'pkcs8', format: 'pem', cipher: 'aes-256-cbc', passphrase: password });
  writeFileSync(keyPath, privatePem, { flag: 'wx', mode: 0o600 });
  const jwk = publicKey.export({ format: 'jwk' });
  writeFileSync(input, Buffer.from(jwk.x, 'base64url').toString('base64') + '\n', { flag: 'wx' });
  console.log('Paire Ed25519 créée. Protégez le fichier privé et retirez-le du dossier de travail.');
} else if (mode === 'signer' && keyPath && input && output) {
  const payload = readFileSync(input);
  if (payload.length > 512 * 1024) throw new Error('Payload trop volumineux.');
  const parsed = JSON.parse(payload.toString('utf8'));
  if (!['stable', 'beta'].includes(parsed.channel) || parsed.target !== 'windows-x86_64' || !Number.isSafeInteger(parsed.sequence) || parsed.sequence < 1 || !parsed.release?.signature || !/^[a-f0-9]{64}$/i.test(parsed.release?.sha256??'') || !parsed.release?.version || !parsed.release?.url || !parsed.expires_at) throw new Error('Payload incomplet.');
  const key = createPrivateKey({ key: readFileSync(keyPath), passphrase: process.env.ERP_MANIFEST_KEY_PASSWORD });
  if (key.asymmetricKeyType !== 'ed25519') throw new Error('La clé doit être Ed25519.');
  const signature = sign(null, payload, key);
  writeFileSync(output, JSON.stringify({ payload: payload.toString('base64'), signature: signature.toString('base64') }, null, 2) + '\n', { flag: 'wx' });
  console.log('Manifeste signé. Publiez uniquement ce fichier et les installateurs publics.');
} else {
  console.error('Utilisation : generer <cle.pem> <publique.txt> OU signer <cle.pem> <payload.json> <manifeste.json>');
  process.exitCode = 1;
}
