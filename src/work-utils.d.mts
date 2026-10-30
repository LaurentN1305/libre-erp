export const statuses:Record<string,string>;export const kinds:Record<string,string>;export const origins:Record<string,string>;export const priorities:Record<string,string>;export const purposes:Record<string,string>;
export function parseMoney(value:string):number;export function dayNumber(iso:string):number;export function addDays(iso:string,n:number):string;export function ganttSpan(start:string,end:string,from:string,to:string):{left:number;width:number}|null;

export function simpleStatus(status:string):{id:string;label:string};
