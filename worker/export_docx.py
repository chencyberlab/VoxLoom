"""Write a normal Word document using Python's standard OOXML/ZIP tools.

No model, network, remote relationship, macro, or office suite is involved.
"""
import json
import os
from pathlib import Path
import sys
import tempfile
import xml.etree.ElementTree as ET
from zipfile import ZIP_DEFLATED, ZipFile

# The embeddable Python bundled on Windows leaves the script's own folder off sys.path.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from notes import blocks, clean_meeting_notes, inline_runs  # noqa: E402

W="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
ET.register_namespace("w",W)


def node(parent,name,attrs=None):
    return ET.SubElement(parent,f"{{{W}}}{name}",{f"{{{W}}}{key}":str(value) for key,value in (attrs or {}).items()})


def export_docx(target,title,text,summary=True):
    document=ET.Element(f"{{{W}}}document")
    body=node(document,"body")
    for kind,value in [("title",title),*blocks(clean_meeting_notes(text) if summary else text)]:
        paragraph=node(body,"p")
        properties=node(paragraph,"pPr")
        node(properties,"pStyle",{"val":"Title" if kind=="title" else "Heading1" if kind=="heading" else "Normal"})
        if kind=="bullet":
            numbering=node(properties,"numPr")
            node(numbering,"ilvl",{"val":0})
            node(numbering,"numId",{"val":1})
        for value,bold in inline_runs(value):
            run=node(paragraph,"r")
            if bold:
                node(node(run,"rPr"),"b")
            content=node(run,"t")
            content.set("{http://www.w3.org/XML/1998/namespace}space","preserve")
            content.text=value
    section=node(body,"sectPr")
    node(section,"pgSz",{"w":11906,"h":16838})
    node(section,"pgMar",{"top":1440,"right":1440,"bottom":1440,"left":1440})
    styles=ET.Element(f"{{{W}}}styles")
    for name,size,bold in [("Normal",22,False),("Title",34,True),("Heading1",26,True)]:
        style=node(styles,"style",{"type":"paragraph","styleId":name})
        node(style,"name",{"val":name})
        props=node(style,"pPr")
        node(props,"spacing",{"after":140,"before":200 if name=="Heading1" else 0})
        if name=="Heading1":node(props,"keepNext")
        run=node(style,"rPr")
        node(run,"rFonts",{"ascii":"Calibri","hAnsi":"Calibri"})
        node(run,"sz",{"val":size})
        node(run,"color",{"val":"000000"})
        if bold:node(run,"b")
    numbering=ET.Element(f"{{{W}}}numbering")
    abstract=node(numbering,"abstractNum",{"abstractNumId":0})
    level=node(abstract,"lvl",{"ilvl":0})
    node(level,"start",{"val":1})
    node(level,"numFmt",{"val":"bullet"})
    node(level,"lvlText",{"val":"•"})
    node(node(level,"pPr"),"ind",{"left":360,"hanging":180})
    node(node(numbering,"num",{"numId":1}),"abstractNumId",{"val":0})
    target=Path(target)
    temporary=None
    try:
        with tempfile.NamedTemporaryFile(prefix=".voxloom-word-",suffix=".tmp",dir=target.parent,delete=False) as output:
            temporary=Path(output.name)
            with ZipFile(output,"w",ZIP_DEFLATED) as archive:
                archive.writestr("[Content_Types].xml",'<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>')
                archive.writestr("_rels/.rels",'<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="document" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>')
                archive.writestr("word/_rels/document.xml.rels",'<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="styles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="numbering" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/></Relationships>')
                for name,value in [("document",document),("styles",styles),("numbering",numbering)]:
                    archive.writestr(f"word/{name}.xml",ET.tostring(value,encoding="utf-8",xml_declaration=True))
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary,target)
    finally:
        if temporary and temporary.exists():temporary.unlink()


if __name__=="__main__":
    try:
        request=json.load(sys.stdin.buffer)
        export_docx(sys.argv[1],request["title"],request["text"],request.get("summary",True))
    except Exception:
        raise SystemExit("The Word document could not be saved. Check the folder and available space.") from None
