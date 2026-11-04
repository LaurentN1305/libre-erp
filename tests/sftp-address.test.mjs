import test from 'node:test';
import assert from 'node:assert/strict';
import {normalizeSftpAddress} from '../src/sftp-address.mjs';
test('SFTP : adresse copiée avec préfixe et port séparé',()=>{
  assert.deepEqual(normalizeSftpAddress('sftp://sauvagnon.fr',2222),{host:'sauvagnon.fr',port:2222});
  assert.deepEqual(normalizeSftpAddress('  SFTP://exemple.fr/  ',2222),{host:'exemple.fr',port:2222});
  assert.deepEqual(normalizeSftpAddress('exemple.fr',2222),{host:'exemple.fr',port:2222});
});
test('SFTP : port intégré et adresses IPv6',()=>{
  assert.deepEqual(normalizeSftpAddress('sftp://exemple.fr:2222',22),{host:'exemple.fr',port:2222});
  assert.deepEqual(normalizeSftpAddress('exemple.fr:2222',2222),{host:'exemple.fr',port:2222});
  assert.deepEqual(normalizeSftpAddress('sftp://[::1]:2222',22),{host:'::1',port:2222});
  assert.deepEqual(normalizeSftpAddress('::1',2222),{host:'::1',port:2222});
  assert.throws(()=>normalizeSftpAddress('sftp://exemple.fr:2222',2200),/différents/);
});
test('SFTP : protocoles, secrets et chemins ne sont pas convertis silencieusement',()=>{
  for(const address of ['ftp://exemple.fr','ftps://exemple.fr','https://exemple.fr','sftp://agent:secret-fictif@exemple.fr','sftp://exemple.fr/dossier','sftp://exemple.fr?secret=fictif','sftp://exemple.fr#dossier'])assert.throws(()=>normalizeSftpAddress(address,22));
  for(const port of [0,-1,65536,NaN,22.5])assert.throws(()=>normalizeSftpAddress('exemple.fr',port),/port SFTP/);
});
