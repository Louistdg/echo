import { invoke } from '@tauri-apps/api/core';
import materials from './material-palettes.json';
export const materialPalettes=materials;
export const themeFamilies=[{id:'classiques',name:'Classiques'},{id:'galaxies',name:'Galaxies'},{id:'pigments',name:'Pigments'},{id:'reliefs',name:'Reliefs'},{id:'nacres',name:'Nacres'}];
export interface Settings {
  large_controls:boolean; reduce_motion:boolean;
  version:number; workspace:string; active_note:string|null; theme:string; theme_favorites:string[]; theme_intensity:number; personal_words:string[];
  quick_width:number; quick_height:number; quick_x:number|null; quick_y:number|null; python:string; model:string; language:string;
  voice_mode:'hold'|'toggle'|'handsfree'; destination:'note'|'application'; keep_audio:boolean; vad_silence_ms:number;
  input_device:string|null; shortcut_dictation:string; shortcut_capture:string; always_on_top:boolean; autostart:boolean;
}
export interface VoiceState {microphone_open:boolean; phase:string; mode:string; kind:string; session_id:string|null; text:string; partial:string; error:string|null; audio_path:string|null; duration_secs:number; level:number; progress:number|null; engine:string; message:string; target_label:string; queued:number;}
export const emptyVoice:VoiceState={microphone_open:false,phase:'idle',mode:'hold',kind:'dictation',session_id:null,text:'',partial:'',error:null,audio_path:null,duration_secs:0,level:0,progress:null,engine:'cold',message:'Prêt à écouter',target_label:'',queued:0};
export const legacyThemes=[{id:'clair',name:'Clair',color:'#5d4dba'},{id:'nuit',name:'Nuit',color:'#a798ee'},{id:'ocean',name:'Océan',color:'#126d91'},{id:'foret',name:'Forêt',color:'#267458'},{id:'violet',name:'Violet',color:'#7450b3'},{id:'rose',name:'Rose',color:'#a33e6a'},{id:'ambre',name:'Ambre',color:'#96601f'},{id:'aurore',name:'Aurore',color:'#b64c36'}];
export const themes=[...legacyThemes.map(t=>({...t,family:'classiques',background:t.color})),...materialPalettes.map(t=>({id:t.id,name:t.name,color:t.colors.accent,family:t.family,background:t.background}))];
export async function updateSettings(patch:Partial<Settings>){return invoke<Settings>('settings_update',{patch});}
export const timeLabel=(seconds:number)=>`${Math.floor(seconds/60).toString().padStart(2,'0')}:${Math.floor(seconds%60).toString().padStart(2,'0')}`;
