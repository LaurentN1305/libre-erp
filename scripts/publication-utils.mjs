import {createHash,createPrivateKey,sign} from 'node:crypto';import {createReadStream} from 'node:fs';
export const REPOSITORY='https://github.com/LaurentN1305/libre-erp';
export function payloadFor({version,channel,sequence,installer,name,signature,sha256,now=new Date(),days=180,notes=''}){
 if(!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/.test(version))throw Error('Version invalide');
 if(!['stable','beta'].includes(channel)||channel==='stable'&&version.includes('-'))throw Error('Canal incompatible avec la version');
 if(!Number.isSafeInteger(sequence)||sequence<1||!Number.isInteger(days)||days<1||days>365)throw Error('Séquence ou durée invalide');
 if(!['nsis','msi'].includes(installer)||!name.endsWith(installer==='msi'?'.msi':'.exe')||/[\\/\r\n]/.test(name)||!name.includes(`_${version}_`))throw Error('Nom ou type d’installeur incompatible');
 if(!/^[a-f0-9]{64}$/i.test(sha256)||!signature)throw Error('Empreinte ou signature absente');
 const decoded=Buffer.from(signature.trim(),'base64').toString('utf8');const trusted=decoded.split(/\r?\n/).find(s=>s.startsWith('trusted comment: '));
 if(!trusted?.slice('trusted comment: '.length).split('\t').includes(`version:${version}`))throw Error('La signature Tauri doit être liée à cette version (signer --app-version)');
 if(!Number.isFinite(now.getTime()))throw Error('Date invalide');
 return {channel,target:'windows-x86_64',sequence,expires_at:new Date(now.getTime()+days*86400000).toISOString(),release:{version,pub_date:now.toISOString(),notes,installer,url:`${REPOSITORY}/releases/download/v${version}/${encodeURIComponent(name)}`,signature:signature.trim(),sha256:sha256.toLowerCase()}};
}
export async function fileDigest(path){const hash=createHash('sha256');let size=0;for await(const chunk of createReadStream(path)){size+=chunk.length;if(size>1024*1024*1024)throw Error('Installeur limité à 1 Gio');hash.update(chunk)}return {sha256:hash.digest('hex'),size}}
export function envelopeFor(payload,encryptedPem,password){const key=createPrivateKey({key:encryptedPem,passphrase:password});if(key.asymmetricKeyType!=='ed25519')throw Error('Clé Ed25519 requise');const bytes=Buffer.from(JSON.stringify(payload));return {payload:bytes.toString('base64'),signature:sign(null,bytes,key).toString('base64')}}
