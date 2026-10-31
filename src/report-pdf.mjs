import {layout} from './pdf-layout.mjs';
const formatted=(v,c)=>v==null||v===''?'—':typeof v==='number'?new Intl.NumberFormat('fr-FR',{minimumFractionDigits:c.kind==='money'?2:0,maximumFractionDigits:c.kind==='money'?2:3}).format(v):String(v);
export async function createReportPdf(table,fontBytes,logo){
 if(table.rows.length>2000)throw Error('Le PDF est limité à 2 000 lignes. Filtrez par bâtiment ou utilisez Excel/CSV.');
 const d=await layout(fontBytes,{landscape:true,title:table.title,subtitle:table.title});d.pdf.setTitle(`Libre ERP — ${table.title}`);d.pdf.setAuthor('Libre ERP');
 d.fresh();await d.logo(logo);d.heading(table.title,23);d.paragraph(`Édition du ${new Date(table.generated_at).toLocaleString('fr-FR',{timeZone:'Europe/Paris'})} | ${table.rows.length} élément(s)`,{size:9});d.paragraph(table.note,{size:9});
 const works=table.columns.some(c=>c.key==='progress_percent')&&table.columns.some(c=>c.key==='title');
 const preferred=works?['reference','building','title','priority','status','target_due_on','participants','estimated_budget_cents','actual_budget_cents']:table.columns.slice(0,6).map(c=>c.key);
 const selected=preferred.map(key=>table.columns.findIndex(c=>c.key===key)).filter(i=>i>=0),remaining=table.columns.map((_,i)=>i).filter(i=>!selected.includes(i)&&!(works&&['kind','planned_start','planned_end','progress_percent'].includes(table.columns[i].key)));
 const weights=selected.map(i=>({reference:1.05,building:1.35,title:2.1,priority:.65,status:1.25,target_due_on:1.5,estimated_budget_cents:1,actual_budget_cents:1,participants:1.4}[table.columns[i].key]??1.5));
 const value=(r,key)=>{const i=table.columns.findIndex(c=>c.key===key);return i<0?'':formatted(r[i],table.columns[i]);};
 const summary=(r,i)=>{const c=table.columns[i];if(works&&c.key==='status')return [formatted(r[i],c),value(r,'kind'),value(r,'progress_percent')?value(r,'progress_percent')+' %':''].filter(Boolean).join('\n');if(works&&c.key==='target_due_on')return formatted(r[i],c)+'\nDébut : '+value(r,'planned_start')+'\nFin : '+value(r,'planned_end');return formatted(r[i],c);};
 if(table.rows.length)d.table(selected.map(i=>works&&table.columns[i].key==='target_due_on'?'Échéance / prévision':table.columns[i].label),table.rows.map(r=>selected.map(i=>summary(r,i))),weights,{size:8.5});else d.paragraph('Aucune ligne dans ce périmètre.');
 if(remaining.length&&table.rows.length){d.fresh('Informations complémentaires');d.heading(works?'Descriptions et observations':'Informations complémentaires',19);d.paragraph('Les références ci-dessous permettent de retrouver chaque élément dans le tableau récapitulatif.',{size:9});
  const ref=table.columns.findIndex(c=>c.key==='reference');
  d.table(['Référence',...remaining.map(i=>table.columns[i].label)],table.rows.map((r,n)=>[ref>=0?r[ref]:String(n+1),...remaining.map(i=>formatted(r[i],table.columns[i]))]),[1,...remaining.map(()=>4)],{size:10});
 }

 return d.finish();
}
