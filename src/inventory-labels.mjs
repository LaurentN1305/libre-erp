import {PDFDocument,rgb} from 'pdf-lib';
import fontkit from '@pdf-lib/fontkit';
import QRCode from 'qrcode';
const mm=72/25.4;
export async function createEquipmentLabels(equipment,fontBytes){
 if(!equipment.length||equipment.length>500)throw Error('Choisissez de 1 à 500 équipements.');
 const doc=await PDFDocument.create();doc.registerFontkit(fontkit);const font=await doc.embedFont(fontBytes,{subset:true});
 const fit=(s,size,width)=>{s=String(s??'').replace(/[\r\n\t]+/g,' ');while(s.length&&font.widthOfTextAtSize(s,size)>width)s=s.slice(0,-1);return s};
 for(let i=0;i<equipment.length;i++){
 const e=equipment[i];const page=i%21===0?doc.addPage([210*mm,297*mm]):doc.getPages().at(-1);const col=i%3,row=Math.floor((i%21)/3),x=(9.75+col*63.5)*mm,y=(297-15-row*38.1-38.1)*mm;
 page.drawRectangle({x,y,width:63.5*mm,height:38.1*mm,borderColor:rgb(.82,.79,.8),borderWidth:.25});
 const data=await QRCode.toDataURL('SERP1:'+e.asset_key,{width:384,margin:4,errorCorrectionLevel:'M'});const png=await doc.embedPng(data);page.drawImage(png,{x:x+1.5*mm,y:y+5*mm,width:27*mm,height:27*mm});
 const tx=x+30*mm,w=32*mm;page.drawText(fit(e.reference,8,w),{x:tx,y:y+30*mm,size:8,font,color:rgb(.6,.1,.2)});let title=e.designation??'';
 for(let line=0;line<3&&title;line++){const part=fit(title,8,w);page.drawText(part,{x:tx,y:y+(25-line*3.7)*mm,size:8,font});title=title.slice(part.length).trimStart()}
 page.drawText(fit(e.building,7,w),{x:tx,y:y+10*mm,size:7,font});page.drawText(fit(e.location_label??e.location??'',6.5,w),{x:tx,y:y+6*mm,size:6.5,font});page.drawText('Libre ERP · Inventaire',{x:x+2*mm,y:y+2.2*mm,size:5.5,font,color:rgb(.4,.35,.38)});
 }
 doc.setTitle('Étiquettes des équipements');return doc.save();
}
