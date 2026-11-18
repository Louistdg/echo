import type {DraftStorage} from './notes/save-session';
export function dockDraftStorage(storage:DraftStorage):DraftStorage{
 const key=(value:string)=>'echo.v3.dock-draft:'+value;
 return{getItem:value=>storage.getItem(key(value)),setItem:(value,text)=>storage.setItem(key(value),text),removeItem:value=>storage.removeItem(key(value))};
}
