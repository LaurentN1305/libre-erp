import {PDFDocument,StandardFonts,rgb} from 'pdf-lib';
import fontkit from '@pdf-lib/fontkit';
export const ink=rgb(.12,.19,.23),red=rgb(.62,.10,.19),muted=rgb(.35,.41,.44),pale=rgb(.97,.94,.95),rule=rgb(.84,.86,.87);
export const clean=value=>String(value??'').replace(/\r/g,'').replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f]/g,'').replace(/\t/g,'    ');
export async function layout(fontBytes,{landscape=false,title='',subtitle=''}={}){
 const pdf=await PDFDocument.create();pdf.registerFontkit(fontkit);
 const font=await pdf.embedFont(fontBytes??StandardFonts.Helvetica,{subset:!!fontBytes});
 const bold=fontBytes?font:await pdf.embedFont(StandardFonts.HelveticaBold);
 const W=landscape?841.89:595.28,H=landscape?595.28:841.89,M=40,bottom=48;
 let page,y=0,sectionTitle=subtitle;const external=new Set(),covers=new Set();
 function wrap(value,width,size=10){const out=[];for(const paragraph of clean(value).split('\n')){let line='';for(const word of paragraph.split(/\s+/)){if(!word)continue;const candidate=line?line+' '+word:word;if(font.widthOfTextAtSize(candidate,size)<=width){line=candidate;continue}if(line){out.push(line);line=''}for(const char of word){if(line&&font.widthOfTextAtSize(line+char,size)>width){out.push(line);line=''}line+=char}}out.push(line)}return out.length?out:[''];}
 function text(s,x,at,size=10,color=ink,strong=false){page.drawText(clean(s),{x,y:at,size,font:strong?bold:font,color});}
 function fresh(label=sectionTitle){sectionTitle=label;page=pdf.addPage([W,H]);text('LIBRE ERP',M,H-31,9,red,true);const labelLines=wrap(label,W-M*2-105,8);text(labelLines[0]??'',145,H-31,8,muted);page.drawLine({start:{x:M,y:H-43},end:{x:W-M,y:H-43},color:red,thickness:1.2});y=H-68;return page;}
 function ensure(height){if(!page||y-height<bottom)fresh();}
 function paragraph(value,{size=10,color=ink,strong=false,gap=8}={}){for(const line of wrap(value,W-2*M,size)){ensure(size+5);text(line,M,y-size,size,color,strong);y-=size+5}y-=gap;}
 function heading(value,size=18){ensure(64);paragraph(value,{size,color:red,strong:true,gap:12});}
 function table(headers,rows,widths,{size=9,headerSize=8.5,padding=8,keepTogether=false}={}){
  const total=W-2*M,weights=widths??headers.map(()=>1),sum=weights.reduce((a,b)=>a+b,0),cw=weights.map(w=>w/sum*total),leading=size+4,pad=padding;
  const hl=headers.map((h,i)=>wrap(h,cw[i]-2*pad,headerSize)),hh=Math.max(...hl.map(l=>l.length))*(headerSize+4)+2*pad;
  function band(){ensure(hh+leading+pad*2);page.drawRectangle({x:M,y:y-hh,width:total,height:hh,color:red});let x=M;for(let c=0;c<headers.length;c++){for(let l=0;l<hl[c].length;l++)text(hl[c][l],x+pad,y-pad-headerSize-l*(headerSize+4),headerSize,rgb(1,1,1),true);x+=cw[c]}y-=hh;}
  const estimated=hh+rows.reduce((sum,row)=>sum+Math.max(...headers.map((_,i)=>wrap(row[i]??'—',cw[i]-2*pad,size).length))*leading+2*pad,0);
  if(keepTogether&&estimated<=H-68-bottom&&y-estimated<bottom)fresh();
  band();
  for(let r=0;r<rows.length;r++){
   const cells=headers.map((_,c)=>wrap(rows[r][c]??'—',cw[c]-2*pad,size));let offset=0,max=Math.max(...cells.map(c=>c.length));
   // Split exceptionally long rows, repeat the header and preserve every line.
   const full=max*leading+2*pad;if(full<=H-68-bottom-hh&&y-full<bottom){fresh();band();}
   while(offset<max){let fit=Math.floor((y-bottom-2*pad)/leading);if(fit<1){fresh();band();fit=Math.floor((y-bottom-2*pad)/leading)}const count=Math.min(fit,max-offset),height=count*leading+2*pad;
    page.drawRectangle({x:M,y:y-height,width:total,height,color:r%2?pale:rgb(1,1,1),borderColor:rule,borderWidth:.5});let x=M;
    for(let c=0;c<cells.length;c++){if(c)page.drawLine({start:{x,y},end:{x,y:y-height},color:rule,thickness:.4});for(let l=0;l<count;l++){const line=cells[c][offset+l];if(line!==undefined)text(line,x+pad,y-pad-size-l*leading,size,c===0?red:ink,c===0)}x+=cw[c]}y-=height;offset+=count;if(offset<max){fresh();band();}
   }
  }
  y-=16;
 }
 async function logo(data,maxW=130,maxH=52){if(!data)return;const bytes=Uint8Array.from(atob(data.split(',')[1]),c=>c.charCodeAt(0));const image=data.startsWith('data:image/png')?await pdf.embedPng(bytes):await pdf.embedJpg(bytes);const scale=Math.min(maxW/image.width,maxH/image.height);ensure(maxH+12);page.drawImage(image,{x:M,y:y-image.height*scale,width:image.width*scale,height:image.height*scale});y-=image.height*scale+16;}
 function finish(){const pages=pdf.getPages();for(let i=0;i<pages.length;i++){const p=pages[i];if(external.has(p))continue;const c=covers.has(p)?rgb(1,1,1):muted;p.drawText(`Libre ERP  |  ${i+1} / ${pages.length}`,{x:30,y:22,size:8,font,color:c});}return pdf.save();}
 return {pdf,font,bold,W,H,M,external,covers,wrap,text,fresh,ensure,paragraph,heading,table,logo,finish,get page(){return page},get y(){return y},set y(v){y=v}};
}
