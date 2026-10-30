export const statuses={identified:'Identifié',scheduled:'Programmé',in_progress:'En cours',pending_validation:'En attente de validation',closed:'Clôturé',archived:'Archivé'};
export const kinds={preventive:'Préventif',corrective:'Correctif',compliance:'Conformité'};
export const origins={commission_observation:'Observation de commission',failure:'Panne ou défaut',regulation:'Réglementation',maintenance:'Maintenance',other:'Autre'};
export const priorities={1:'Basse',2:'Normale',3:'Haute',4:'Critique'};
export const purposes={quote:'Devis',invoice:'Facture',report:'Compte rendu',photo:'Photo',other:'Autre document'};
export function parseMoney(value){const s=String(value).replace(/[\s\u00a0\u202f]/g,'').replace(',','.');if(!/^\d{1,11}(\.\d{1,2})?$/.test(s))throw new Error('Montant attendu en euros, avec au plus deux décimales.');const [whole,part='']=s.split('.');const n=Number(whole)*100+Number(part.padEnd(2,'0'));if(!Number.isSafeInteger(n)||n>1000000000000)throw new Error('Montant trop élevé.');return n;}
export function dayNumber(iso){if(!/^\d{4}-\d{2}-\d{2}$/.test(iso))throw new Error('Date invalide.');const value=Date.parse(`${iso}T00:00:00Z`);if(!Number.isFinite(value)||new Date(value).toISOString().slice(0,10)!==iso)throw new Error('Date invalide.');return value/86400000;}
export function addDays(iso,n){return new Date((dayNumber(iso)+n)*86400000).toISOString().slice(0,10);}
export function ganttSpan(start,end,from,to){const a=dayNumber(from),b=dayNumber(to);if(b<a||b-a>730)throw new Error('Choisissez une période de 1 à 731 jours.');const s=dayNumber(start),e=dayNumber(end);if(e<s)throw new Error('Dates de travail inversées.');if(e<a||s>b)return null;const count=b-a+1;return{left:100*(Math.max(s,a)-a)/count,width:100*(Math.min(e,b)-Math.max(s,a)+1)/count};}

export function simpleStatus(status){if(['closed','archived'].includes(status))return {id:'done',label:'Terminé'};if(['in_progress','pending_validation'].includes(status))return {id:'active',label:status==='pending_validation'?'En cours · à valider':'En cours'};return {id:'todo',label:'À faire'};}
