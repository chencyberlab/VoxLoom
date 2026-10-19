export interface Meeting { id:string; title:string; created_at:number; duration_ms:number; status:string; notes:string; summary:string }
export interface Track { id:string; meeting_id:string; path:string; source:string; offset_ms:number }
export interface Segment { id:number; start_ms:number; end_ms:number; text:string; source:string }
export interface Detail { meeting:Meeting; tracks:Track[]; segments:Segment[] }
export interface Device { name:string; label:string; monitor:boolean; default:boolean; state:string }
export interface Capture { meeting_id:string; elapsed_ms:number; paused:boolean; failed:boolean; levels:number[]; warning?:string }
export interface Job { id:string; meeting_id:string; kind:string; status:string; progress:number; error?:string }
export interface Model { id:string; name:string; filename:string; bytes:number; present:boolean; license:string; license_url:string; manual_only?:boolean; disk_bytes?:number; retired?:boolean }
export interface Hit { meeting_id:string; title:string; created_at:number; start_ms:number; end_ms:number; text:string; modality:string; source:string; cosine?:number; fusion_rank?:number }
export const time = (ms:number) => { const n=Math.max(0,Math.floor(ms/1000)); return `${Math.floor(n/3600).toString().padStart(2,'0')}:${Math.floor(n/60%60).toString().padStart(2,'0')}:${(n%60).toString().padStart(2,'0')}`; };
