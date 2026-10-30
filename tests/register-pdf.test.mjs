import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {PDFDocument} from 'pdf-lib';
import {createRegisterPdf} from '../src/register-pdf.mjs';
test('Le PDF du registre conserve A–I, les types X/L, les versions et les annexes',async()=>{
 const schema=JSON.parse(await readFile(new URL('../src/register-schema.json',import.meta.url),'utf8'));
 const annex=await PDFDocument.create();annex.addPage([595,842]);annex.addPage([595,842]);const annexBytes=await annex.save();
 const png=Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a1ZkAAAAASUVORK5CYII=','base64');
 const snapshot={building:{id:'test',name:'Salle des fêtes et gymnase',address:'Place de la République',category:4,capacity:200,manager:'Élodie François',opened_on:'2001-09-01',organisation:'Commune de test',types:['X','L']},generated_at:'2026-10-29T10:00:00Z',schema,files:[{id:'pdf',name:'Procès-verbal de test.pdf',mime:'application/pdf',size:annexBytes.length},{id:'png',name:'Plan de test.png',mime:'image/png',size:png.length}],entries:schema.map(b=>({id:b.code,block:b.code,title:`Entrée de test ${b.code}`,version:2,effective_on:'2026-10-29',applicability:'applicable',justification:'',recorded_at:'2026-10-29T09:00:00Z',actor:'Élodie François',reason:'Correction et vérification',archived:false,fields:Object.fromEntries(b.fields.map(f=>[f.key,f.key==='observations'?'Observation détaillée et mesure corrective. '.repeat(180):'Renseignement vérifié — sécurité et évacuation.'])),files:b.code==='B'?['pdf','png']:[]}))};
 snapshot.entries.push({...snapshot.entries[3],id:'archive-test',title:'Travaux historiques archivés',archived:true});
 const font=await readFile(`${process.env.WINDIR??'C:/Windows'}/Fonts/arial.ttf`);
 const bytes=await createRegisterPdf(snapshot,async id=>id==='pdf'?annexBytes:png,font);
 const loaded=await PDFDocument.load(bytes);assert.ok(loaded.getPageCount()>=15);assert.equal(loaded.getTitle(),'Registre de sécurité — Salle des fêtes et gymnase');
 await mkdir('.tools',{recursive:true});await writeFile('.tools/m2-registre-test.pdf',bytes);
});
