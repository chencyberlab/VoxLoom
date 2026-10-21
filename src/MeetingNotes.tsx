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

type Block={title:string}|{heading:string}|{paragraph:string[]}|{items:string[]};

/** Titles, headings, paragraphs and lists of a recording's notes. The page and the Markdown copy share it, so both show the same notes.
 *  A "# " title starts a part of its own, such as each "# Summary" VoxLoom adds. Only those summaries are cleaned of recording times:
 *  what the person typed is shown as typed, line by line. */
function noteBlocks(text:string):Block[] {
  const blocks:Block[]=[];
  let paragraph:string[]=[];
  let items:string[]=[];
  let summary=false;
  const flush=()=>{
    if(paragraph.length){blocks.push({paragraph});paragraph=[];}
    if(items.length){blocks.push({items});items=[];}
  };
  for(const raw of text.split('\n')){
    const title=raw.trim().match(/^#\s+(.+)$/);
    if(title){flush();summary=/^summary\b/i.test(title[1]);blocks.push({title:title[1]});continue;}
    const line=(summary?cleanMeetingNotes(raw):raw).trim();
    if(!line){flush();continue;}
    const heading=line.match(/^#{2,4}\s+(.+)$/)||line.match(/^\*\*([^*]+)\*\*:?$/)
      ||(summary?line.match(/^(Overview|Key discussion points|Discussion|Decisions|Action items|Next steps|Open questions):?$/i):null);
    if(heading){flush();blocks.push({heading:heading[1].replace(/:$/,'')});continue;}
    const item=line.match(/^(?:[-*•]|\d+[.)])\s+(.+)$/);
    if(item){if(paragraph.length)flush();items.push(item[1]);}
    else{if(items.length)flush();paragraph.push(line);}
  }
  flush();
  return blocks;
}

export function MeetingNotes({text}:{text:string}) {
  return <div className="meeting-notes">{noteBlocks(text).map((block,i)=>'title' in block?<h2 key={i}>{block.title}</h2>
    :'heading' in block?<h3 key={i}>{block.heading}</h3>
    :'items' in block?<ul key={i}>{block.items.map((item,j)=><li key={j}>{inline(item)}</li>)}</ul>
    :<p key={i}>{block.paragraph.map((line,j)=><Fragment key={j}>{j>0&&<br/>}{inline(line)}</Fragment>)}</p>)}</div>;
}

/** The same notes as clean Markdown: `# ` titles, `## ` headings, `- ` bullets, **bold** kept, recording times removed from summaries. */
export function meetingNotesMarkdown(text:string):string {
  return noteBlocks(text).map(block=>'title' in block?`# ${block.title}`:'heading' in block?`## ${block.heading}`
    :'items' in block?block.items.map(item=>`- ${item}`).join('\n'):block.paragraph.join('\n')).join('\n\n');
}
