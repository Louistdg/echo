export type BlockId='live'|'notes'|'controls'|'translation';
export interface DockBlock {id:BlockId;collapsed:boolean;height:number}
export interface DockLayout {version:1;width:number;blocks:DockBlock[]}
export interface DockState {width:number;height:number;visible:boolean;action:string|null;revision:number}
export const blockNames:Record<BlockId,string>={live:'Texte en direct',notes:'Bloc-notes',controls:'Commandes de dictée',translation:'Traduire'};
export const blockIds:BlockId[]=['live','notes','controls','translation'];
export const dockKey='echo.v3.dock.layout';
export function validateLayout(value:unknown):DockLayout{
 const input=value&&typeof value==='object'?value as Record<string,unknown>:{};
 const width=typeof input.width==='number'&&Number.isFinite(input.width)?Math.max(280,Math.min(640,input.width)):360;
 const seen=new Set<string>();const blocks:DockBlock[]=[];
 if(Array.isArray(input.blocks))for(const item of input.blocks.slice(0,16)){
  if(!item||typeof item!=='object'||!blockIds.includes(item.id)||seen.has(item.id))continue;
  seen.add(item.id);blocks.push({id:item.id,collapsed:item.collapsed===true,height:typeof item.height==='number'&&Number.isFinite(item.height)?Math.max(120,Math.min(640,item.height)):item.id==='notes'?280:item.id==='translation'?350:190});
 }
 return{version:1,width,blocks};
}
export function loadLayout(storage:Pick<Storage,'getItem'>):DockLayout{try{return validateLayout(JSON.parse(storage.getItem(dockKey)||'null'))}catch{return validateLayout(null)}}
export function reorder(layout:DockLayout,id:BlockId,direction:number):DockLayout{
 const blocks=[...layout.blocks];const index=blocks.findIndex(b=>b.id===id);const next=index+direction;
 if(index<0||next<0||next>=blocks.length)return layout;
 [blocks[index],blocks[next]]=[blocks[next],blocks[index]];return{...layout,blocks};
}

export function resizeBlock(layout:DockLayout,id:BlockId,height:number):DockLayout{
 if(!Number.isFinite(height)||!layout.blocks.some(b=>b.id===id))return layout;
 return {...layout,blocks:layout.blocks.map(b=>b.id===id?{...b,height:Math.max(120,Math.min(640,Math.round(height)))}:b)};
}
