import { invoke, isTauri } from '@tauri-apps/api/core';
export const desktop=isTauri();
export async function api<T>(action:string,payload:unknown={}):Promise<T> {
  if (!desktop) throw new Error('Open the VoxLoom desktop application to access local recordings and devices.');
  return invoke<T>('dispatch',{action,payload});
}
