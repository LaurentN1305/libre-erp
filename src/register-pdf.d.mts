import type { RegisterSnapshot } from './register-types';
export function createRegisterPdf(snapshot: RegisterSnapshot, readAttachment:(id:string)=>Promise<Uint8Array>, fontBytes?:Uint8Array):Promise<Uint8Array>;
