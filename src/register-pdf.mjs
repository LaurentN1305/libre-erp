import {PDFDocument,rgb} from 'pdf-lib';
import {layout,red,ink,muted,pale} from './pdf-layout.mjs';
const date=v=>v?(/^\d{4}-\d{2}-\d{2}$/.test(v)?v.split('-').reverse().join('/'):v):'Non renseignée';
const shown=(e,key)=>key==='suivi'?({none:'Aucune observation à suivre',open:'Observation ouverte',resolved:'Observation levée'}[e.fields[key]]??'Non renseigné'):key==='type_controle'?({commission:'Visite de commission',verification:'Vérification périodique'}[e.fields[key]]??'Non renseigné'):(e.fields[key]||'Non renseigné');
const brief=v=>String(v??'').length>140?String(v).slice(0,137)+'…':String(v??'');
export async function createRegisterPdf(snapshot,readAttachment,fontBytes,logo){
 const d=await layout(fontBytes,{subtitle:'Registre de sécurité'}),pdf=d.pdf,b=snapshot.building,contents=[],files=new Map(snapshot.files.map(f=>[f.id,f])),annexes=new Map();
 pdf.setTitle(`Registre de sécurité — ${b.name}`);pdf.setAuthor(b.organisation);pdf.setSubject('Registre A–I · CCH R.143-44, R.141-10 et R.141-11');
 d.fresh();d.covers.add(d.page);d.page.drawRectangle({x:0,y:0,width:d.W,height:d.H,color:red});d.y=790;d.paragraph('LIBRE ERP  /  PATRIMOINE & SÉCURITÉ',{size:10,color:rgb(1,1,1)});d.y-=28;
 d.paragraph('REGISTRE',{size:35,strong:true,color:rgb(1,1,1),gap:0});d.paragraph('DE SÉCURITÉ',{size:35,strong:true,color:rgb(1,1,1),gap:20});
 // The identity panel stays readable on white, including a municipal logo if supplied.
 d.y-=8;d.paragraph(b.name,{size:20,color:rgb(1,1,1),strong:true});d.paragraph(b.organisation,{size:12,color:rgb(1,1,1),gap:20});
 if(logo){d.page.drawRectangle({x:36,y:d.y-74,width:170,height:72,color:rgb(1,1,1)});d.y-=8;await d.logo(logo,150,48);}
 d.table(['Établissement','Identification'],[['Adresse',b.address],['Types / catégorie',`${b.types.join(' / ')} · catégorie ${b.category}`],['Capacité déclarée',`${b.capacity} personnes`],['Ouverture au public',date(b.opened_on)],['Responsable sécurité',b.manager||'À désigner'],['Date de cette édition',new Date(snapshot.generated_at).toLocaleString('fr-FR',{timeZone:'Europe/Paris'})]],[1,2.2],{size:10});
 d.ensure(90);const noteColor=d.covers.has(d.page)?rgb(1,1,1):ink;
 d.paragraph('DOCUMENT DE SUIVI DE L’ÉTABLISSEMENT',{size:10,color:noteColor});d.paragraph('À conserver à disposition de la commission de sécurité. Édition locale : les informations et justificatifs doivent être tenus à jour par l’exploitant.',{size:9,color:noteColor});
 d.fresh('Sommaire et mode de lecture');const toc=d.page;d.heading('Sommaire',24);const tocTop=d.y;d.y=220;
 d.heading('Lire et compléter ce registre',13);d.paragraph('Les neuf rubriques A à I rassemblent les informations saisies, leurs dates, leurs versions et leurs pièces jointes. Les tableaux de suivi précèdent les fiches détaillées. Une rubrique vide est signalée et ne vaut pas déclaration de conformité.',{size:10});d.paragraph('Références conservées : CCH R.143-44, R.141-10 et R.141-11. Les espaces de visa sont réservés à une signature manuscrite éventuelle ; ils ne constituent pas une signature électronique.',{size:9});
 const summaryKeys={A:['exploitant','activites'],B:['nature','reference'],C:['date','organisme','avis','echeance'],D:['debut','fin','nature','entrepreneurs'],E:['nom','prenom','fonction','rang','statut'],F:['generales','rassemblement'],G:['date','nature','intervenant','resultat','echeance'],H:['date','duree','participants','bilan'],I:['nature','attestations']};
 for(const block of snapshot.schema){d.fresh(`${block.code} · ${block.title}`);contents.push({code:block.code,title:block.title,page:pdf.getPageCount()});
  d.page.drawRectangle({x:40,y:d.y-52,width:515.28,height:52,color:red});d.text(block.code,54,d.y-37,30,rgb(1,1,1),true);d.y-=74;d.heading(block.title,22);
  const entries=snapshot.entries.filter(e=>e.block===block.code);if(block.code==='E')entries.sort((a,b)=>(Number(a.fields.rang)||0)-(Number(b.fields.rang)||0)||(a.fields.nom||'').localeCompare(b.fields.nom||'','fr'));
  d.paragraph(`${entries.length} entrée(s) dans cette édition · ordre ${block.code==='E'?'nominatif et hiérarchique':'du registre'}`,{size:9});
  if(!entries.length){d.paragraph('Aucune entrée renseignée. À compléter ou à justifier explicitement.');d.table(['Date','Information / observation à consigner'],[['',''],['',''],['','']],[1,4]);continue;}
  const keys=summaryKeys[block.code]??[],fields=keys.map(k=>block.fields.find(f=>f.key===k)).filter(Boolean);
  d.table(['Fiche',...fields.map(f=>f.label)],entries.map((e,i)=>[`${block.code}${i+1}`, ...fields.map(f=>brief(shown(e,f.key)))]),[.5,...fields.map(()=>1.5)],{size:8});
  d.paragraph('Les textes complets, justificatifs et données de traçabilité figurent dans les fiches ci-dessous.',{size:9});
  for(let i=0;i<entries.length;i++){const e=entries[i];d.ensure(215);d.heading(`${block.code}${i+1} · ${e.title}`,14);
   d.table(['Suivi de la fiche','Valeur'],[['Date / version',`${date(e.effective_on)} · version ${e.version}${e.archived?' · ARCHIVÉE':''}`],['Applicabilité',e.applicability==='applicable'?'Applicable':e.applicability==='not_applicable'?'Non applicable':'À confirmer'],['Enregistrement',`${e.recorded_at} · ${e.actor}`],['Motif de version',e.reason||'Non renseigné'],...(e.justification?[['Justification',e.justification]]:[])],[1,2.4],{size:9,padding:6});
   const detailRows=block.fields.map(f=>[f.label,shown(e,f.key)]);if(['C','D','G','H'].includes(block.code))detailRows.push(['Visa manuscrit éventuel','Date / nom / signature :\n\n']);d.table(['Information','Renseignement'],detailRows,[1,2.4],{size:9,padding:6,keepTogether:true});
   for(const id of e.files){const f=files.get(id);if(!f)throw Error('Une pièce jointe référencée est absente.');if(!annexes.has(id))annexes.set(id,{...f,number:annexes.size+1});d.paragraph(`Annexe ${annexes.get(id).number} : ${f.name} (réf. ${id})`,{size:9});}

  }
 }
 if([...annexes.values()].reduce((s,f)=>s+f.size,0)>150*1024*1024)throw Error('Les annexes dépassent 150 Mio. Réduisez leur taille avant l’export.');
 let annexStart=null;
 if(annexes.size){d.fresh('Inventaire des annexes');annexStart=pdf.getPageCount();d.heading('Pièces jointes et annexes',22);d.table(['N°','Document','Référence'],[...annexes.entries()].map(([id,f])=>[f.number,f.name,id]),[.4,2,1],{size:9});}
 for(const [id,f] of annexes){d.fresh(`Annexe ${f.number}`);d.heading(`Annexe ${f.number} · ${f.name}`,18);d.paragraph(`Référence : ${id}`,{size:9});const data=await readAttachment(id);
  if(f.mime==='application/pdf'){let doc;try{doc=await PDFDocument.load(data)}catch{throw Error(`L’annexe « ${f.name} » ne peut pas être intégrée. Vérifiez qu’elle est lisible et non protégée par mot de passe.`)}const pages=await pdf.copyPages(doc,doc.getPageIndices());for(const p of pages){pdf.addPage(p);d.external.add(p);}}
  else{const img=f.mime==='image/png'?await pdf.embedPng(data):await pdf.embedJpg(data);const scale=Math.min(515/img.width,(d.y-60)/img.height);d.page.drawImage(img,{x:40,y:d.y-img.height*scale,width:img.width*scale,height:img.height*scale});}
 }
 let ty=tocTop;
 for(const c of [...contents,...(annexStart?[{code:'PJ',title:'Pièces jointes et annexes',page:annexStart}]:[])]){const lines=d.wrap(c.title,370,11),h=Math.max(38,lines.length*15+14);toc.drawRectangle({x:40,y:ty-h,width:515.28,height:h-3,color:pale});toc.drawText(c.code,{x:50,y:ty-22,size:13,font:d.bold,color:red});lines.forEach((line,i)=>toc.drawText(line,{x:88,y:ty-20-i*15,size:11,font:d.font,color:ink}));toc.drawText(String(c.page),{x:527,y:ty-22,size:11,font:d.font,color:red});ty-=h;}
 return d.finish();
}
