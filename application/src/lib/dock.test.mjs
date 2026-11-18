import test from 'node:test';
import assert from 'node:assert/strict';
import {validateLayout,loadLayout,reorder,dockKey,resizeBlock} from './dock.ts';
test('dock starts with no demo blocks and tolerates inaccessible storage',()=>{
 assert.deepEqual(validateLayout(null),{version:1,width:360,blocks:[]});
 assert.deepEqual(loadLayout({getItem(){throw Error('denied')}}),validateLayout(null));
 assert.deepEqual(loadLayout({getItem(){return '{broken'}}),validateLayout(null));
});
test('persisted module layout is allowlisted, deduplicated and bounded',()=>{
 const layout=validateLayout({width:999999,blocks:[{id:'notes',height:10},{id:'notes',height:500},{id:'unknown'},{id:'translation',height:9000,collapsed:true},{id:'live',height:NaN},{id:'controls',height:Infinity}]});
 assert.equal(layout.width,640);
 assert.deepEqual(layout.blocks.map(b=>b.id),['notes','translation','live','controls']);
 assert.equal(layout.blocks[0].height,120);assert.equal(layout.blocks[1].height,640);
 assert.equal(layout.blocks[1].collapsed,true);assert.equal(layout.blocks[2].height,190);
 const stored=new Map([[dockKey,JSON.stringify(layout)]]);assert.deepEqual(loadLayout({getItem:k=>stored.get(k)??null}),layout);
 assert.equal(validateLayout({width:100,blocks:[]}).width,280);
});
test('reordering preserves each real module and its size without mutating the prior layout',()=>{
 const layout=validateLayout({blocks:[{id:'notes',height:320},{id:'live',height:170},{id:'translation',height:450}]});
 const moved=reorder(layout,'translation',-1);
 assert.deepEqual(moved.blocks.map(b=>b.id),['notes','translation','live']);
 assert.equal(moved.blocks[1].height,450);assert.equal(layout.blocks[1].id,'live');
 assert.equal(reorder(layout,'notes',-1),layout);assert.equal(reorder(layout,'controls',1),layout);
});

test('main Notes and dock preserve distinct drafts and refuse stale writes to the same real file',async()=>{
 const {SaveSession}=await import('./notes/save-session.ts');
 const {dockDraftStorage}=await import('./dock-notes.ts');
 const values=new Map();
 const storage={getItem:k=>values.get(k)??null,setItem:(k,v)=>values.set(k,v),removeItem:k=>values.delete(k)};
 let document={path:'même.md',workspace:'D:/QA',title:'Même note',text:'Départ',modified:'initial'};
 const save=async request=>{assert.equal(request.path,document.path);if(request.expected!==document.text)throw Error('Conflit : le fichier a changé');document={...document,text:request.text};return document};
 const main=new SaveSession({document,storage,save,changed:()=>{},debounceMs:60000});
 const dock=new SaveSession({document,storage:dockDraftStorage(storage),save,changed:()=>{},debounceMs:60000});
 main.edit('Brouillon Notes');dock.edit('Brouillon socle');
 assert.equal(values.size,2);
 await main.flush();assert.equal(values.size,1);
 await assert.rejects(dock.flush(),/Conflit/);
 assert.equal(document.text,'Brouillon Notes');assert.equal(dock.snapshot().text,'Brouillon socle');assert.equal(values.size,1);
 dock.dispose();main.dispose();
 const recovered=new SaveSession({document,storage:dockDraftStorage(storage),save,changed:()=>{},debounceMs:60000});
 assert.equal(recovered.snapshot().recoveryConflict,true);assert.equal(recovered.snapshot().text,'Brouillon socle');
 await assert.rejects(recovered.flush(),/fichier a changé/);recovered.dispose();
});


test('module height changes are bounded, persistent and preserve other modules',()=>{
 const layout=validateLayout({width:400,blocks:[{id:'notes',height:280},{id:'live',height:190,collapsed:true}]});
 const resized=resizeBlock(layout,'notes',520);
 assert.equal(resized.blocks[0].height,520);assert.equal(resized.width,400);
 assert.deepEqual(resized.blocks[1],layout.blocks[1]);assert.equal(layout.blocks[0].height,280);
 assert.deepEqual(validateLayout(JSON.parse(JSON.stringify(resized))),resized);
 assert.equal(resizeBlock(layout,'notes',9000).blocks[0].height,640);
 assert.equal(resizeBlock(layout,'notes',-1).blocks[0].height,120);
 assert.equal(resizeBlock(layout,'notes',NaN),layout);
 assert.equal(resizeBlock(layout,'translation',500),layout);
});
