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

export function MeetingNotes({text}:{text:string}) {
  const blocks:ReactNode[]=[];
  let paragraph:string[]=[];
  let items:string[]=[];
  const flush=()=>{
    if(paragraph.length){blocks.push(<p key={blocks.length}>{inline(paragraph.join(' '))}</p>);paragraph=[];}
    if(items.length){blocks.push(<ul key={blocks.length}>{items.map((item,i)=><li key={i}>{inline(item)}</li>)}</ul>);items=[];}
  };
  for(const raw of cleanMeetingNotes(text).split('\n')){
    const line=raw.trim();
    if(!line){flush();continue;}
    const heading=line.match(/^#{1,4}\s+(.+)$/)||line.match(/^\*\*([^*]+)\*\*:?$/)
      ||line.match(/^(Overview|Key discussion points|Discussion|Decisions|Action items|Next steps|Open questions):?$/i);
    if(heading){flush();blocks.push(<h3 key={blocks.length}>{heading[1].replace(/:$/,'')}</h3>);continue;}
    const item=line.match(/^(?:[-*•]|\d+[.)])\s+(.+)$/);
    if(item){if(paragraph.length)flush();items.push(item[1]);}
    else{if(items.length)flush();paragraph.push(line);}
  }
  flush();
  return <div className="meeting-notes">{blocks}</div>;
}
