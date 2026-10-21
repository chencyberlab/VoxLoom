import { MutableRefObject, useCallback, useEffect, useRef, useState } from 'react';
import { api } from './api';
import { MeetingNotes } from './MeetingNotes';

// A summary added to empty notes starts with its title: keep it on lines of its own below the text.
const below=(text:string,added:string)=>{
  if(!added||added.startsWith('\n')||!text.trim())return text+added;
  return text+'\n'.repeat(Math.max(0,2-(text.length-text.replace(/\n+$/,'').length)))+added;
};

/** One recording's notes (give it `key={meetingId}`): what the person types, with every summary VoxLoom added below it.
 *  Saves a moment after typing stops, on leaving the field or the page, and before Summarise reads the notes (`flush`).
 *  A summary is only ever added at the end of the saved notes, so one that arrives while the person is typing is
 *  added below their text here too, and the host does the same if a save sends notes older than a summary. */
export function NotesEditor({meetingId,notes,editing,onEdit,onSaved,flush,value,onError,placeholder,label}:{
  meetingId:string; notes:string; editing:boolean; onEdit?:(editing:boolean)=>void; onSaved?:(notes:string)=>void;
  flush?:MutableRefObject<(()=>Promise<void>)|null>; value?:MutableRefObject<string>; onError:(message:string)=>void; placeholder:string; label:string;
}) {
  const [draft,setDraft]=useState(notes);
  // The saved notes the draft was written from.
  const base=useRef(notes);const current=useRef(notes);const report=useRef(onError);report.current=onError;
  const timer=useRef<number|undefined>(undefined);const saving=useRef<Promise<void>>(Promise.resolve());
  const field=useRef<HTMLTextAreaElement>(null);
  const shown=useRef(value);shown.current=value;const saved=useRef(onSaved);saved.current=onSaved;
  const change=(text:string)=>{current.current=text;if(shown.current)shown.current.current=text;setDraft(text);};
  useEffect(()=>{if(shown.current)shown.current.current=current.current;},[]);

  const save=useCallback(()=>{
    window.clearTimeout(timer.current);
    saving.current=saving.current.then(async()=>{
      const text=current.current,from=base.current;
      if(text===from)return;
      try{
        const stored=(await api<{notes:string}>('update',{id:meetingId,notes:text,notes_base:from})).notes;
        // The host kept a summary added after the notes this draft started from.
        const added=stored!==text&&stored.startsWith(text)?stored.slice(text.length):'';
        base.current=stored;
        change(current.current===text?stored:below(current.current,added));
        saved.current?.(stored);
      }catch(e){report.current(String(e));}
    });
    return saving.current;
  },[meetingId]);

  useEffect(()=>{if(flush)flush.current=save;return()=>{if(flush&&flush.current===save)flush.current=null;};},[flush,save]);
  useEffect(()=>()=>{void save();},[save]);

  // The saved notes changed underneath, such as a summary being added.
  useEffect(()=>{
    if(notes===base.current)return;
    if(notes.startsWith(base.current)){const added=notes.slice(base.current.length);base.current=notes;change(below(current.current,added));}
    else if(current.current===base.current){base.current=notes;change(notes);}
  },[notes]);

  useEffect(()=>{const node=field.current;if(editing&&node&&document.activeElement!==node){node.focus();node.setSelectionRange(node.value.length,node.value.length);}},[editing]);

  if(!editing&&draft.trim())return <div className="notes-view" role="button" tabIndex={0} aria-label="Edit notes"
    onClick={()=>onEdit?.(true)} onKeyDown={e=>{if(e.key==='Enter'){e.preventDefault();onEdit?.(true);}}}><MeetingNotes text={draft}/></div>;
  return <><textarea ref={field} className="notes-editor" aria-label={label} value={draft} placeholder={placeholder} rows={Math.max(8,draft.split('\n').length+2)}
    onChange={e=>{change(e.target.value);window.clearTimeout(timer.current);timer.current=window.setTimeout(()=>void save(),1200);}}
    onFocus={()=>onEdit?.(true)} onBlur={()=>void save()}/>
    {/* Copy notes reads the formatted notes, so they are there while editing too. */}
    <div hidden><MeetingNotes text={draft}/></div></>;
}
