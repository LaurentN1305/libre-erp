export type RegisterField={key:string;label:string;kind:string;required:boolean;help?:string};
export type RegisterBlock={code:string;title:string;fields:RegisterField[]};
export type RegisterEntry={id:string;building_id:string;block:string;title:string;version:number;effective_on:string;applicability:string;justification:string|null;fields:Record<string,string>;files:string[];recorded_at:string;actor:string;reason:string;archived:boolean};
export type RegisterSnapshot={building:{id:string;name:string;address:string;category:number;capacity:number;manager:string|null;opened_on:string|null;organisation:string;types:string[]};entries:RegisterEntry[];files:{id:string;name:string;mime:string;size:number}[];writable:boolean;generated_at:string;schema:RegisterBlock[]};
export type RegisterHistory={version:number;payload:{title:string;effective_on:string;fields:Record<string,string>;applicability:string;justification:string;files:string[]};recorded_at:string;actor:string;reason:string};
