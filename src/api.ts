import { invoke, isTauri } from '@tauri-apps/api/core';
export const desktop=isTauri();
export async function api<T>(action:string,payload:unknown={}):Promise<T> {
  if (!desktop) throw new Error('Open the VoxLoom desktop application to access local recordings and devices.');
  return invoke<T>('dispatch',{action,payload});
}
// A track's playback file as bytes, for a webview that cannot stream it from asset: (WebKitGTK).
export async function mediaBytes(id:string):Promise<ArrayBuffer> {
  return invoke<ArrayBuffer>('playback_bytes',{id});
}
