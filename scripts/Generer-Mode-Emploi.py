"""Rebuild the public user guide from its French content, without opening app data.
Requires reportlab and Pillow; Windows Arial fonts (or DejaVu Sans).
"""
from pathlib import Path
import json, os
from html import escape
from reportlab.pdfgen import canvas
from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer, Table, TableStyle, PageBreak, Image, KeepTogether
from reportlab.lib.styles import ParagraphStyle
from reportlab.lib import colors
from reportlab.lib.enums import TA_LEFT
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont

ROOT=Path(__file__).resolve().parents[1]
data=json.loads((ROOT/'docs/guide-utilisateur.json').read_text(encoding='utf-8'))
out=ROOT/'output/pdf';out.mkdir(parents=True,exist_ok=True)
fontdir=Path(os.environ.get('WINDIR','C:/Windows'))/'Fonts'
regular=fontdir/'arial.ttf';bold=fontdir/'arialbd.ttf'
if not regular.exists():
 regular=Path('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf');bold=regular.with_name('DejaVuSans-Bold.ttf')
pdfmetrics.registerFont(TTFont('Guide',str(regular)));pdfmetrics.registerFont(TTFont('GuideBold',str(bold)))
pdfmetrics.registerFontFamily('Guide',normal='Guide',bold='GuideBold',italic='Guide',boldItalic='GuideBold')
RED=colors.HexColor('#a51d32');INK=colors.HexColor('#19313d');GREY=colors.HexColor('#52636c');PALE=colors.HexColor('#fbf0f2');LINE=colors.HexColor('#dce3e7')
styles={
 'body':ParagraphStyle('body',fontName='Guide',fontSize=10.2,leading=14.3,textColor=INK,spaceAfter=8),
 'step':ParagraphStyle('step',fontName='Guide',fontSize=10.2,leading=14.3,textColor=INK,spaceAfter=0),
 'title':ParagraphStyle('title',fontName='GuideBold',fontSize=25,leading=29,textColor=RED,spaceAfter=7),
 'sub':ParagraphStyle('sub',fontName='Guide',fontSize=11,leading=15,textColor=GREY,spaceAfter=18),
 'h':ParagraphStyle('h',fontName='GuideBold',fontSize=13,leading=17,textColor=RED,spaceBefore=8,spaceAfter=8,keepWithNext=True),
 'note':ParagraphStyle('note',fontName='Guide',fontSize=9,leading=12.5,textColor=GREY,spaceAfter=8),
 'cell':ParagraphStyle('cell',fontName='Guide',fontSize=9.3,leading=12.8,textColor=INK),
 'th':ParagraphStyle('th',fontName='GuideBold',fontSize=9.3,leading=12.8,textColor=colors.white),
 'cap':ParagraphStyle('cap',fontName='Guide',fontSize=8,leading=11,textColor=GREY,spaceAfter=12),
}
def p(text,style='body'):
 safe=escape(str(text)).replace('\n','<br/>')
 safe=safe.replace('github.com/LaurentN1305/libre-erp/releases/latest','<link href="https://github.com/LaurentN1305/libre-erp/releases/latest" color="#a51d32"><u>github.com/LaurentN1305/libre-erp/releases/latest</u></link>')
 return Paragraph(safe,styles[style])
W=511.28
story=[];md=['# Libre ERP - Mode d’emploi','',f'Version {data["version"]} · {data["date"]}','','[Télécharger la dernière version Windows](https://github.com/LaurentN1305/libre-erp/releases/latest)','','Ce guide concerne la version installée. Les exemples d’écran sont fictifs.','']

def table(rows,widths=None,header=True):
 cells=[[p(v,'th' if r==0 and header else 'cell') for v in row] for r,row in enumerate(rows)]
 t=Table(cells,colWidths=widths or [W*.36,W*.64],repeatRows=1 if header else 0,hAlign='LEFT')
 cmds=[('VALIGN',(0,0),(-1,-1),'TOP'),('LEFTPADDING',(0,0),(-1,-1),8),('RIGHTPADDING',(0,0),(-1,-1),8),('TOPPADDING',(0,0),(-1,-1),7),('BOTTOMPADDING',(0,0),(-1,-1),7),('LINEBELOW',(0,0),(-1,-1),.4,LINE)]
 if header:cmds += [('BACKGROUND',(0,0),(-1,0),RED),('ROWBACKGROUNDS',(0,1),(-1,-1),[colors.white,colors.HexColor('#f8f5f6')])]
 t.setStyle(TableStyle(cmds));return t

for n,page in enumerate(data['pages'],1):
 if n>1:story.append(PageBreak())
 if n==1:
  story += [p('LIBRE ERP','sub'),p('Mode d’emploi','title'),p('Installer • Démarrer • Utiliser • Sauvegarder','sub')]
 story += [p(f'{n:02}  {page["title"]}','title'),p(page['subtitle'],'sub')]
 md += [f'## {n}. {page["title"]}','',page['subtitle'],'']
 for b in page['blocks']:
  kind=b[0]
  if kind in ('intro','p','note'):
   story.append(p(b[1],'note' if kind=='note' else 'body'));md += [b[1],'']
  elif kind=='h':story.append(p(b[1],'h'));md += [f'### {b[1]}','']
  elif kind=='steps':
   rows=[[Paragraph(f'<b>{i}</b>',styles['step']),p(s,'step')] for i,s in enumerate(b[1:],1)]
   t=Table(rows,colWidths=[23,W-23],hAlign='LEFT');t.setStyle(TableStyle([('VALIGN',(0,0),(-1,-1),'TOP'),('TEXTCOLOR',(0,0),(0,-1),RED),('LEFTPADDING',(0,0),(-1,-1),0),('RIGHTPADDING',(0,0),(-1,-1),7),('BOTTOMPADDING',(0,0),(-1,-1),8)]));story.append(t);story.append(Spacer(1,3));md += [f'{i}. {s}' for i,s in enumerate(b[1:],1)]+['']
  elif kind=='callout':
   inner=[p(b[1],'h'),p(b[2],'body')];t=Table([[inner]],colWidths=[W]);t.setStyle(TableStyle([('BACKGROUND',(0,0),(-1,-1),PALE),('BOX',(0,0),(-1,-1),.6,colors.HexColor('#e6c6cf')),('LEFTPADDING',(0,0),(-1,-1),13),('RIGHTPADDING',(0,0),(-1,-1),13),('TOPPADDING',(0,0),(-1,-1),2),('BOTTOMPADDING',(0,0),(-1,-1),6)]));story += [KeepTogether([t]),Spacer(1,12)];md += [f'> **{b[1]}**',f'> {b[2]}','']
  elif kind=='table':
   story += [table(b[1]),Spacer(1,10)];md += ['| '+' | '.join(b[1][0])+' |','| '+' | '.join('---' for _ in b[1][0])+' |']+['| '+' | '.join(row)+' |' for row in b[1][1:]]+['']
  elif kind=='contents':
   rows=[[title,'page '+num] for title,num in b[1]];toc=table(rows,[W*.85,W*.15],False);toc.setStyle(TableStyle([('TOPPADDING',(0,0),(-1,-1),4),('BOTTOMPADDING',(0,0),(-1,-1),4)]));story += [toc,Spacer(1,12)];md += [f'- {title} : chapitre {num}.' for title,num in b[1]]+['']
  elif kind=='image':
   image=ROOT/'docs/media/guide-tableau-bord.png';story += [Image(str(image),width=W,height=W*376/1440),p(b[2],'cap')];md += ['![Repères du tableau de bord](media/guide-tableau-bord.png)','',b[2],'']

class NumberedCanvas(canvas.Canvas):
 def __init__(self,*a,**kw):super().__init__(*a,**kw);self.states=[]
 def showPage(self):self.states.append(dict(self.__dict__));self._startPage()
 def save(self):
  total=len(self.states)
  for state in self.states:
   self.__dict__.update(state);self.setStrokeColor(RED);self.setLineWidth(1);self.line(42,806,553,806)
   self.setFont('GuideBold',8);self.setFillColor(RED);self.drawString(42,818,'LIBRE ERP  /  GUIDE UTILISATEUR')
   self.setFont('Guide',8);self.setFillColor(GREY);self.drawRightString(553,818,'Version '+data['version']);self.drawString(42,25,'Installation et utilisation · '+data['date']);self.drawRightString(553,25,f'{self._pageNumber} / {total}')
   super().showPage()
  super().save()

pdf=out/'LibreERP-Mode-emploi.pdf'
doc=SimpleDocTemplate(str(pdf),pagesize=(595.28,841.89),rightMargin=42,leftMargin=42,topMargin=62,bottomMargin=46,title='Libre ERP - Mode d’emploi',author='Libre ERP',subject='Installation et utilisation - version '+data['version'])
doc.build(story,canvasmaker=NumberedCanvas)
(ROOT/'docs/MODE-EMPLOI.md').write_text('\n'.join(md),encoding='utf-8')
print(pdf)
