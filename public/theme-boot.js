// Apply a saved theme before the stylesheet arrives, so the first paint matches.
// A file, not an inline script: the app's CSP allows only script-src 'self'. System leaves this unset.
try{var t=localStorage.getItem('voxloom-appearance');if(t==='light'||t==='dark')document.documentElement.setAttribute('data-theme',t);}catch(e){}
