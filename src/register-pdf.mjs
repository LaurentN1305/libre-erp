import { PDFDocument, StandardFonts, rgb } from 'pdf-lib';
import fontkit from '@pdf-lib/fontkit';
export async function createRegisterPdf(snapshot, readAttachment, fontBytes) {
 const externalPages=new Set();const pdf=await PDFDocument.create();pdf.registerFontkit(fontkit);
 const font=await pdf.embedFont(fontBytes??StandardFonts.Helvetica,{subset:!!fontBytes});
 const bold=fontBytes?font:await pdf.embedFont(StandardFonts.HelveticaBold);
 const building=snapshot.building;
 pdf.setTitle(`Registre de sécurité — ${building.name}`);pdf.setAuthor(building.organisation);pdf.setSubject('Registre A–I · CCH R.143-44, R.141-10 et R.141-11');
 let page,y;const contents=[];
 const clean=s=>String(s??'').replace(/\r/g,'').replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/g,'').replace(/\t/g,'    ');
 function width(text,size){try{return font.widthOfTextAtSize(text,size)}catch{throw new Error('Un caractère ne peut pas être imprimé avec la police Windows disponible. Vérifiez les noms et les textes saisis.')}}
 function newPage(){page=pdf.addPage([595.28,841.89]);y=775;page.drawText('LIBRE ERP · REGISTRE DE SÉCURITÉ',{x:44,y:808,size:9,font,color:rgb(.12,.38,.35)});}
 function line(text='',size=10,emphasis=false){for(const paragraph of clean(text).split('\n')){let current='';for(const word of paragraph.split(/\s+/)){const candidate=current?`${current} ${word}`:word;if(width(candidate,size)<=505){current=candidate;continue}if(current){draw(current,size,emphasis);current=''}for(const char of Array.from(word)){if(width(current+char,size)>505&&current){draw(current,size,emphasis);current=''}current+=char;}}draw(current,size,emphasis);}}
 function draw(text,size,strong){if(y<55)newPage();page.drawText(text,{x:44,y,size,font:strong?bold:font,color:rgb(.09,.18,.22)});y-=size+5;}
 newPage();line('REGISTRE DE SÉCURITÉ',23,true);line(building.name,18,true);line(building.organisation,12);line(building.address);line(`Date d’ouverture au public : ${building.opened_on??'Non renseignée'}`);line(`Types ERP : ${building.types.join(' / ')} · Catégorie ${building.category} · Capacité ${building.capacity}`);line(`Responsable sécurité : ${building.manager??'À désigner'}`);line(`Édition du ${new Date(snapshot.generated_at).toLocaleString('fr-FR',{timeZone:'Europe/Paris'})}`);line('');
 line('CCH R.143-44, R.141-10 et R.141-11',12,true);line('Registre structuré en neuf rubriques. Les informations et justificatifs doivent être tenus à jour par l’exploitant. Une rubrique vide est signalée et ne vaut pas déclaration de conformité.');line('');
 newPage();const cover=page,coverY=y;line('SOMMAIRE',12,true);
 const active=snapshot.entries;const files=new Map(snapshot.files.map(f=>[f.id,f]));const annexes=new Map();
 for(const block of snapshot.schema){newPage();contents.push({text:`${block.code}. ${block.title}`,page:pdf.getPageCount()});line(`${block.code}. ${block.title}`,17,true);line('');
 const entries=active.filter(e=>e.block===block.code);if(block.code==='E')entries.sort((a,b)=>Number(a.fields.rang||0)-Number(b.fields.rang||0)||(a.fields.nom||'').localeCompare(b.fields.nom||'','fr'));
 if(!entries.length)line('Aucune entrée renseignée dans cette rubrique. À compléter ou à justifier explicitement.');
 for(const e of entries){line(e.title,13,true);line(`Date : ${e.effective_on} · Version ${e.version}${e.archived?' · ARCHIVÉE':''} · ${e.applicability==='applicable'?'Applicable':e.applicability==='not_applicable'?'Non applicable':'À confirmer'}`);line(`Saisie : ${e.recorded_at} · ${e.actor}`,9);line(`Motif : ${e.reason}`,9);if(e.justification)line(`Justification : ${e.justification}`);
 for(const f of block.fields){line(f.label,10,true);line(f.key==='suivi'?({none:'Aucune observation à suivre',open:'Observation ouverte',resolved:'Observation levée'}[e.fields[f.key]]??'Non renseigné'):f.key==='type_controle'?({commission:'Visite de commission',verification:'Vérification périodique'}[e.fields[f.key]]??'Non renseigné'):e.fields[f.key]||'Non renseigné');}
 for(const id of e.files){const f=files.get(id);if(!f)throw new Error('Une pièce jointe référencée est absente.');annexes.set(id,f);line(`Pièce jointe : ${f.name} (réf. ${id})`,9);}line('');}
 }
 for(let i=0;i<contents.length;i++){const c=contents[i];cover.drawText(`${c.text} · page ${c.page}`,{x:44,y:coverY-22-i*20,size:10,font});}
 if([...annexes.values()].reduce((s,f)=>s+f.size,0)>150*1024*1024)throw new Error('Les annexes dépassent 150 Mio. Réduisez leur taille avant l’export.');
 for(const [id,f] of annexes){newPage();line('ANNEXE',12,true);line(f.name,14,true);line(`Référence : ${id}`,9);const data=await readAttachment(id);
 if(f.mime==='application/pdf'){let doc;try{doc=await PDFDocument.load(data)}catch{throw new Error(`L’annexe « ${f.name} » ne peut pas être intégrée. Vérifiez qu’elle est lisible et non protégée par mot de passe.`)}const pages=await pdf.copyPages(doc,doc.getPageIndices());for(const p of pages){pdf.addPage(p);externalPages.add(p);}}
 else{const img=f.mime==='image/png'?await pdf.embedPng(data):await pdf.embedJpg(data);const p=pdf.addPage([595.28,841.89]);const scale=Math.min(505/img.width,720/img.height);p.drawImage(img,{x:44,y:70,width:img.width*scale,height:img.height*scale});}}
 const pages=pdf.getPages();for(let i=0;i<pages.length;i++){const p=pages[i];if(externalPages.has(p))continue;p.drawText(`Libre ERP · ${i+1}/${pages.length}`,{x:30,y:20,size:8,font,color:rgb(.25,.35,.4)});}
 return pdf.save();
}
