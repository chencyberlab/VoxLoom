import { Fragment, ReactNode } from 'react';

export function cleanMeetingNotes(text:string):string {
  return text.replace(/\[\d{2}:\d{2}:\d{2}\]/g,'')
    .replace(/^(\s*(?:[-*]\s*)?)\d{2}:\d{2}:\d{2}\s*[-–—:]?\s*/gm,'$1')
    .split('\n').map(line=>line.replace(/[ \t]{2,}/g,' ').trimEnd()).join('\n').trim();
}

function inline(text:string):ReactNode[] {
  return text.split(/(\*\*[^*]+\*\*)/g).map((part,i)=>part.startsWith('**')&&part.endsWith('**')
    ?<strong key={i}>{part.slice(2,-2)}</strong>:<Fragment key={i}>{part}</Fragment>);
}

type Block={heading:string}|{paragraph:string}|{items:string[]};

/** Headings, paragraphs and lists of saved notes. The page and the Markdown copy share it, so both show the same notes. */
function noteBlocks(text:string):Block[] {
  const blocks:Block[]=[];
  let paragraph:string[]=[];
  let items:string[]=[];
  const flush=()=>{
    if(paragraph.length){blocks.push({paragraph:paragraph.join(' ')});paragraph=[];}
    if(items.length){blocks.push({items});items=[];}
  };
  for(const raw of cleanMeetingNotes(text).split('\n')){
    const line=raw.trim();
    if(!line){flush();continue;}
    const heading=line.match(/^#{1,4}\s+(.+)$/)||line.match(/^\*\*([^*]+)\*\*:?$/)
      ||line.match(/^(Overview|Key discussion points|Discussion|Decisions|Action items|Next steps|Open questions):?$/i);
    if(heading){flush();blocks.push({heading:heading[1].replace(/:$/,'')});continue;}
    const item=line.match(/^(?:[-*•]|\d+[.)])\s+(.+)$/);
    if(item){if(paragraph.length)flush();items.push(item[1]);}
    else{if(items.length)flush();paragraph.push(line);}
  }
  flush();
  return blocks;
}

export function MeetingNotes({text}:{text:string}) {
  return <div className="meeting-notes">{noteBlocks(text).map((block,i)=>'heading' in block?<h3 key={i}>{block.heading}</h3>
    :'items' in block?<ul key={i}>{block.items.map((item,j)=><li key={j}>{inline(item)}</li>)}</ul>
    :<p key={i}>{inline(block.paragraph)}</p>)}</div>;
}

/** The same notes as clean Markdown: `## ` headings, `- ` bullets, **bold** kept, recording times removed. */
export function meetingNotesMarkdown(text:string):string {
  return noteBlocks(text).map(block=>'heading' in block?`## ${block.heading}`
    :'items' in block?block.items.map(item=>`- ${item}`).join('\n'):block.paragraph).join('\n\n');
}
