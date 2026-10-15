export async function copyMeetingNotes(element:HTMLElement):Promise<void> {
  const copy=element.cloneNode(true) as HTMLElement;
  copy.style.cssText='font-family:Calibri,Arial,sans-serif;font-size:11pt;line-height:1.4;color:#000;background:#fff';
  copy.querySelectorAll<HTMLElement>('h3').forEach(node=>node.style.cssText='font-size:13pt;font-weight:bold;margin:16pt 0 6pt;color:#000');
  copy.querySelectorAll<HTMLElement>('p,li').forEach(node=>node.style.cssText='font-size:11pt;color:#000;margin:0 0 6pt');
  const html=`<html><body><!--StartFragment-->${copy.outerHTML}<!--EndFragment--></body></html>`;
  const plain=Array.from(copy.children).map(node=>node.tagName==='UL'
    ?Array.from(node.children).map(item=>`• ${item.textContent||''}`).join('\n')
    :node.textContent||'').join('\n\n');

  // The native WebKit clipboard supports HTML through a normal copy event.
  // Keep that synchronous with the button gesture, and preserve user selection.
  const selection=window.getSelection();
  const ranges=selection?Array.from({length:selection.rangeCount},(_,i)=>selection.getRangeAt(i).cloneRange()):[];
  const focused=document.activeElement as HTMLElement|null;
  const onCopy=(event:ClipboardEvent)=>{
    if(event.clipboardData){event.clipboardData.setData('text/html',html);event.clipboardData.setData('text/plain',plain);event.preventDefault();}
  };
  const holder=document.createElement('div');
  holder.contentEditable='true';holder.style.cssText='position:fixed;left:-10000px;top:0';holder.append(copy);document.body.append(holder);
  let copied=false;
  try{
    const range=document.createRange();range.selectNodeContents(holder);selection?.removeAllRanges();selection?.addRange(range);
    document.addEventListener('copy',onCopy);
    copied=document.execCommand('copy');
  }catch{/* Try the modern clipboard interface below. */}
  finally{
    document.removeEventListener('copy',onCopy);holder.remove();selection?.removeAllRanges();
    ranges.filter(range=>range.startContainer.isConnected).forEach(range=>selection?.addRange(range));focused?.focus({preventScroll:true});
  }
  if(copied)return;
  if(navigator.clipboard?.write&&typeof ClipboardItem!=='undefined'){
    await navigator.clipboard.write([new ClipboardItem({'text/html':new Blob([html],{type:'text/html'}),'text/plain':new Blob([plain],{type:'text/plain'})})]);
    return;
  }
  if(navigator.clipboard?.writeText){await navigator.clipboard.writeText(plain);return;}
  throw new Error('Copy is unavailable. Choose Word document to save your notes.');
}
