// Prépare des fichiers publics ; ne contacte pas GitHub et ne publie aucune clé.
import {parseArgs} from 'node:util';import {readFile,mkdir,copyFile,writeFile,stat} from 'node:fs/promises';import {basename,resolve,join} from 'node:path';import {execFileSync} from 'node:child_process';import {payloadFor,fileDigest,envelopeFor} from './publication-utils.mjs';
const {values:a}=parseArgs({options:{channel:{type:'string'},sequence:{type:'string'},nsis:{type:'string'},msi:{type:'string'},output:{type:'string'},days:{type:'string',default:'180'},notes:{type:'string',default:''}}});
for(const k of ['channel','sequence','nsis','msi','output'])if(!a[k])throw Error(`Option --${k} requise`);
const root=resolve(import.meta.dirname,'..'),version=JSON.parse(await readFile(join(root,'package.json'),'utf8')).version;
if(execFileSync('git',['status','--porcelain','--untracked-files=no'],{cwd:root,encoding:'utf8'}).trim())throw Error('Validez les changements des sources dans Git avant de préparer une publication');
const commit=execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim();
const keyPath=process.env.ERP_MANIFEST_PRIVATE_KEY_PATH,password=process.env.ERP_MANIFEST_KEY_PASSWORD;
if((!keyPath&&!process.env.ERP_MANIFEST_PRIVATE_KEY)||!password)throw Error('Chargez le coffre de signature éditeur avant cette commande');
const encryptedPem=process.env.ERP_MANIFEST_PRIVATE_KEY?Buffer.from(process.env.ERP_MANIFEST_PRIVATE_KEY):await readFile(keyPath);const now=new Date(),items=[];
for(const [installer,path] of [['nsis',a.nsis],['msi',a.msi]]){if(!(await stat(path)).isFile())throw Error('Installeur absent');const digest=await fileDigest(path),signature=await readFile(path+'.sig','utf8');const payload=payloadFor({version,channel:a.channel,sequence:Number(a.sequence),installer,name:basename(path),signature,sha256:digest.sha256,now,days:Number(a.days),notes:a.notes});const envelope=envelopeFor(payload,encryptedPem,password);items.push({installer,path,name:basename(path),digest,payload,envelope})}
// Nouveau dossier obligatoire ; aucune ancienne livraison ou clé n'est remplacée.
const output=resolve(a.output);await mkdir(output);await mkdir(join(output,'v'+version));await mkdir(join(output,a.channel));
for(const item of items){const target=join(output,'v'+version,item.name);await copyFile(item.path,target);await copyFile(item.path+'.sig',target+'.sig');const envelope=JSON.stringify(item.envelope,null,2)+'\n';await writeFile(target+'.preuve.json',envelope,{flag:'wx'});await writeFile(join(output,a.channel,`windows-x86_64-${item.installer}.json`),envelope,{flag:'wx'})}
await writeFile(join(output,'publication.json'),JSON.stringify({version,channel:a.channel,sequence:Number(a.sequence),source_commit:commit,created_at:now.toISOString(),files:items.map(i=>({file:`v${version}/${i.name}`,sha256:i.digest.sha256,size:i.digest.size}))},null,2)+'\n',{flag:'wx'});
console.log('Publication préparée. Publiez les paquets du dossier version puis les manifestes du canal. Aucune clé privée n’est incluse.');
