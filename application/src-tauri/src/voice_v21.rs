//! Voice sessions for Écho V3. One microphone, one resident local worker and a
//! bounded FIFO. Destinations are frozen before capture; delivery follows a
//! durable result write. Cancellation invalidates every late worker response.
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Emitter, Manager};
use crate::{capture, workspace};

#[path = "voice_pipeline.rs"]
mod pipeline;
use pipeline::{InlinePreview, InlineTicket, TextPresentation};

const MAX_JOBS: usize = 4;
const DICTATION_LIMIT_SECS: u64 = 300;
static EPOCH: AtomicU64 = AtomicU64::new(1);
static START_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static KEY_DOWN: AtomicBool = AtomicBool::new(false);
static SHUTDOWN: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();
static CONTROL: Mutex<()> = Mutex::new(());
static DELIVERY: Mutex<()> = Mutex::new(());
static PERSIST: Mutex<()> = Mutex::new(());
static RUNTIME: LazyLock<Mutex<Runtime>> = LazyLock::new(|| Mutex::new(Runtime::default()));
static WORKER: Mutex<Option<Arc<Worker>>> = Mutex::new(None);
static WORKER_ID: AtomicU64 = AtomicU64::new(0);
static CANCEL_WAIT: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static PREVIEW_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceState {
    #[serde(default)]
    pub microphone_open: bool,
    pub phase: String,
    pub mode: String,
    pub kind: String,
    pub session_id: Option<String>,
    pub text: String,
    pub partial: String,
    #[serde(default)]
    pub finalizing_text: String,
    #[serde(default)]
    pub finalizing_id: Option<String>,
    #[serde(default)]
    pub last_sent_text: String,
    #[serde(default)]
    pub sent_count: usize,
    #[serde(default)]
    pub inline_preview_supported: bool,
    #[serde(default)]
    pub inline_preview_active: bool,
    pub error: Option<String>,
    pub audio_path: Option<String>,
    pub duration_secs: f64,
    pub level: f32,
    pub progress: Option<f64>,
    pub engine: String,
    pub message: String,
    pub target_label: String,
    pub queued: usize,
}
impl Default for VoiceState {
    fn default() -> Self {
        Self { microphone_open: false, phase: "idle".into(), mode: "hold".into(), kind: "dictation".into(),
            session_id: None, text: String::new(), partial: String::new(), error: None,
            finalizing_text: String::new(), finalizing_id: None,
            last_sent_text: String::new(), sent_count: 0,
            inline_preview_supported: false, inline_preview_active: false,
            audio_path: None, duration_secs: 0.0, level: 0.0, progress: None,
            engine: "cold".into(), message: "Prêt à dicter".into(),
            target_label: "Note Écho".into(), queued: 0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FrozenNote { workspace: String, path: String }
impl FrozenNote {
    fn target(&self) -> workspace::NoteTarget {
        workspace::NoteTarget { workspace: self.workspace.clone(), path: self.path.clone() }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Target { Note { note: FrozenNote }, Application { handle: String } }

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionSpec {
    id: String, mode: String, kind: String, workspace: String,
    target: Target, fallback: FrozenNote, target_label: String,
    python: String, model: String, language: String, keep_audio: bool,
    #[serde(default)] epoch: u64,
    #[serde(default)] hotwords: String,
}
struct Active {
    spec: SessionSpec, started: Instant, stream: Option<capture::StreamCapture>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Job {
    id: String, spec: SessionSpec, audio_path: String, duration_secs: f64,
    status: String, text: String, error: Option<String>,
    #[serde(default)]
    provisional: bool,
}
impl Job {
    fn blocked(&self) -> bool { matches!(self.status.as_str(), "result" | "error" | "delivering") }
}
fn restore_job(mut job:Job,epoch:u64)->Job{
    job.spec.epoch=epoch;
    job.status=if job.text.is_empty(){"error"}else{"result"}.into();
    job.error=Some(if job.text.is_empty(){"Prise retrouvée après redémarrage. Réessaie la transcription."}
        else if job.provisional{"Brouillon provisoire retrouvé, non validé par la transcription finale. Réessaie avec l’audio conservé ou vérifie le texte avant de le recopier."}
        else{"Texte retrouvé. Vérifie la destination avant de le recopier."}.into());
    job
}
#[derive(Clone)]
struct InlineContext { ticket: InlineTicket, spec: SessionSpec, audio_path: String }
#[derive(Default)]
struct Runtime {
    state: VoiceState, active: Option<Active>, arming: bool,
    presentation: TextPresentation,
    inline: InlinePreview, inline_context: Option<InlineContext>,
    phrase_audio: Option<(String, String)>,
    jobs: VecDeque<Job>, current: Option<String>, sessions: HashMap<String, SessionSpec>,
    vad_ready: Option<String>, speech: bool, last_error: Option<String>,
}

fn job_dir() -> PathBuf { workspace::portable_root().join("configuration").join("voice-jobs") }
fn persist(job: &Job) -> Result<(), String> {
    let _lock = PERSIST.lock().map_err(|_| "Journal vocal verrouillé")?;
    let dir = job_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("Journal vocal inaccessible : {e}"))?;
    let bytes = serde_json::to_vec(job).map_err(|e| e.to_string())?;
    crate::atomic::write_atomic(&dir.join(format!("{}.json", job.id)), &bytes)
        .map_err(|e| format!("Résultat non sauvegardé : {e}"))
}
fn refresh(runtime: &mut Runtime) {
    runtime.state.partial = runtime.presentation.partial.clone();
    runtime.state.finalizing_text = runtime.presentation.finalizing_text.clone();
    runtime.state.finalizing_id = runtime.presentation.finalizing_id.clone();
    runtime.state.last_sent_text = runtime.presentation.last_sent_text.clone();
    runtime.state.sent_count = runtime.presentation.sent_count;
    runtime.state.inline_preview_supported = runtime.inline.supported;
    runtime.state.inline_preview_active = runtime.inline.active();
    // A retained result can take display priority while the live stream is closing.
    // Keep microphone control independent from the result/error phase.
    runtime.state.microphone_open = runtime.arming || runtime.active.is_some();
    runtime.state.queued = runtime.jobs.iter().filter(|j| j.status == "queued").count();
    if runtime.arming {
        runtime.state.phase = "arming".into();
        return;
    }
    if let Some(job) = runtime.jobs.iter().find(|j| j.blocked()) {
        runtime.state.phase = if job.text.is_empty() { "error" } else { "result" }.into();
        runtime.state.text = job.text.clone();
        runtime.state.partial.clear();
        runtime.state.error = job.error.clone();
        runtime.state.audio_path = Some(job.audio_path.clone());
        runtime.state.session_id = Some(job.id.clone());
        runtime.state.duration_secs = job.duration_secs;
        runtime.state.target_label = job.spec.target_label.clone();
        runtime.state.progress = None;
        return;
    }
    runtime.state.error = runtime.last_error.clone();
    if let Some(active) = runtime.active.as_ref() {
        runtime.state.phase = if streams_phrases(&active.spec.kind,&active.spec.mode) && !runtime.speech { "listening" } else { "recording" }.into();
        runtime.state.mode = active.spec.mode.clone();
        runtime.state.kind = active.spec.kind.clone();
        runtime.state.session_id = Some(active.spec.id.clone());
        runtime.state.duration_secs = active.started.elapsed().as_secs_f64();
        runtime.state.target_label = active.spec.target_label.clone();
    } else if let Some(job) = runtime.current.as_ref().and_then(|id| runtime.jobs.iter().find(|j| &j.id == id)) {
        runtime.state.phase = "transcribing".into();
        runtime.state.session_id = Some(job.id.clone());
        runtime.state.audio_path = Some(job.audio_path.clone());
        runtime.state.duration_secs = job.duration_secs;
        runtime.state.target_label = job.spec.target_label.clone();
        runtime.state.level = 0.0;
    } else {
        runtime.state.phase = if runtime.last_error.is_some() { "error" } else { "idle" }.into();
        runtime.state.session_id = None;
        runtime.state.level = 0.0;
        runtime.state.progress = None;
    }
}
fn publish(app: &AppHandle) {
    let state = { let mut runtime = RUNTIME.lock().unwrap(); refresh(&mut runtime); runtime.state.clone() };
    let _ = app.emit("voice:state", state);
}
fn fail(app: &AppHandle, message: String) {
    { let mut runtime = RUNTIME.lock().unwrap(); runtime.arming = false; runtime.last_error = Some(message.clone()); runtime.state.message = message; }
    publish(app);
}
pub fn is_busy() -> bool {
    // Contention means work is changing: conservatively pause translation.
    RUNTIME.try_lock().map(|r| r.arming || r.active.is_some() || r.current.is_some() || r.jobs.iter().any(|j| matches!(j.status.as_str(),"queued"|"transcribing"|"delivering"))).unwrap_or(true)
}
fn snapshot() -> VoiceState {
    let mut runtime = RUNTIME.lock().unwrap(); refresh(&mut runtime); runtime.state.clone()
}

// Compatibility notification from text views. Preview production deliberately
// continues while collapsed or hidden; final decoding takes priority.
#[tauri::command]
pub fn voice_preview_interest(window:tauri::WebviewWindow,enabled:bool)->Result<bool,String>{
 let label=window.label();
 if !matches!(label,"main"|"dock-panels"){return Err("Cette fenêtre ne présente pas le texte en direct.".into())}
 Ok(enabled)
}
const PREVIEW_SAMPLES:usize=12*16_000;
const PREVIEW_INTERVAL: Duration = Duration::from_millis(400);
#[derive(Clone,Debug)]
struct PreviewWord {word:String,start:f64,end:f64}
struct PreviewCapture {spec:SessionSpec,generation:u64,audio:VecDeque<i16>,samples:u64,last_sent:Instant,inflight:Option<String>,prefix:String,prefix_end:f64,words:Vec<PreviewWord>}
#[derive(Clone)]
struct PreviewJob {session:String,epoch:u64,generation:u64,speech_phrase:Option<String>,start:f64,path:PathBuf}
#[derive(Default)]
struct PreviewRuntime {active:Option<PreviewCapture>,jobs:HashMap<String,PreviewJob>}
static PREVIEW:LazyLock<Mutex<PreviewRuntime>>=LazyLock::new(||Mutex::new(PreviewRuntime::default()));
fn preview_begin(spec:&SessionSpec){
 let old={
  let mut runtime=RUNTIME.lock().unwrap();
  if !runtime.active.as_ref().is_some_and(|a|a.spec.id==spec.id)&&!(runtime.arming&&runtime.sessions.contains_key(&spec.id)){return}
  runtime.presentation.clear_preview();
  let generation=PREVIEW_SEQUENCE.fetch_add(1,Ordering::SeqCst)+1;
  runtime.inline.next_phrase(&spec.id,spec.epoch,generation);
  runtime.phrase_audio=None;
  let mut p=PREVIEW.lock().unwrap();
  let old=p.active.take().and_then(|a|a.inflight);
  p.active=Some(PreviewCapture{spec:spec.clone(),generation,audio:VecDeque::with_capacity(PREVIEW_SAMPLES),samples:0,last_sent:Instant::now(),inflight:None,prefix:String::new(),prefix_end:0.0,words:vec![]});
  old
 };
 if let Some(id)=old{if let Some(worker)=WORKER.lock().unwrap().clone(){let _=worker.send(json!({"op":"cancel","id":id}));}}
}
fn remove_preview_file(path:&Path){
 let _=fs::remove_file(path);let _=fs::remove_file(path.with_extension("wav.part"));
}
fn preview_cleanup_all(){
 let files={let mut p=PREVIEW.lock().unwrap();p.active=None;p.jobs.drain().map(|(_,job)|job.path).collect::<Vec<_>>()};
 for path in files{remove_preview_file(&path);}
}
fn preview_sweep_abandoned(){
 let dir=workspace::portable_root().join("configuration/voice-preview");
 if let Ok(files)=fs::read_dir(dir){for file in files.flatten(){
  let path=file.path();let name=path.file_name().and_then(|s|s.to_str()).unwrap_or("");
  if name.starts_with("preview-")&&(name.ends_with(".wav")||name.ends_with(".wav.part")){let _=fs::remove_file(path);}
 }}
}
fn preview_stop(session:&str){
 let cancel={let mut p=PREVIEW.lock().unwrap();if p.active.as_ref().is_some_and(|a|a.spec.id==session){p.active.take().and_then(|a|a.inflight)}else{None}};
 if let Some(id)=cancel{if let Some(worker)=WORKER.lock().unwrap().clone(){let _=worker.send(json!({"op":"cancel","id":id}));}}
}
fn preview_ready(active: &mut PreviewCapture, samples: &[i16], now: Instant, final_pending: bool) -> bool {
    active.audio.extend(samples.iter().copied());
    active.samples += samples.len() as u64;
    while active.audio.len() > PREVIEW_SAMPLES { active.audio.pop_front(); }
    if final_pending || active.audio.len() < 9_600 || active.inflight.is_some()
        || now.saturating_duration_since(active.last_sent) < PREVIEW_INTERVAL
    { return false; }
    active.last_sent = now;
    true
}
fn preview_matches(active:&PreviewCapture,job:&PreviewJob,id:&str,epoch:u64)->bool{
 active.spec.id==job.session&&active.spec.epoch==job.epoch&&active.spec.epoch==epoch&&active.generation==job.generation&&active.inflight.as_deref()==Some(id)
}
fn preview_feed(app:&AppHandle,session:&str,samples:&[i16]){
 // Buffer all audio. Avoid generating optional WAVs while a durable final is
 // waiting, decoding or being delivered. Visibility never disables the preview.
 let (final_pending,speech_phrase)=RUNTIME.try_lock().map(|r|(r.current.is_some()||r.jobs.iter().any(|j|matches!(j.status.as_str(),"queued"|"transcribing"|"delivering")),r.phrase_audio.as_ref().map(|(id,_)|id.clone()))).unwrap_or((true,None));
 let due={
  let mut p=PREVIEW.lock().unwrap();let Some(active)=p.active.as_mut().filter(|a|a.spec.id==session&&a.spec.epoch==EPOCH.load(Ordering::SeqCst))else{return};
  preview_ready(active,samples,Instant::now(),final_pending)
 };
 if !due{return}
 let ready={
  let mut p=PREVIEW.lock().unwrap();let Some(active)=p.active.as_mut().filter(|a|a.spec.id==session&&a.spec.epoch==EPOCH.load(Ordering::SeqCst)&&a.inflight.is_none())else{return};
  let id=format!("preview-{}-{}",session,uuid::Uuid::new_v4());
  let start=(active.samples-active.audio.len() as u64) as f64/16000.0;
  let data:Vec<i16>=active.audio.iter().copied().collect();let spec=active.spec.clone();active.inflight=Some(id.clone());
  let path=workspace::portable_root().join("configuration/voice-preview").join(format!("{id}.wav"));
  let job=PreviewJob{session:session.into(),epoch:spec.epoch,generation:active.generation,speech_phrase,start,path:path.clone()};p.jobs.insert(id.clone(),job);
  (id,path,data,spec)
 };
 let (id,path,data,spec)=ready;let handle=app.clone();
 std::thread::spawn(move||{
  let send=(||->Result<(),String>{
   fs::create_dir_all(path.parent().unwrap()).map_err(|e|e.to_string())?;
   let temporary=path.with_extension("wav.part");
   let wav_spec=hound::WavSpec{channels:1,sample_rate:16000,bits_per_sample:16,sample_format:hound::SampleFormat::Int};
   let mut wav=hound::WavWriter::create(&temporary,wav_spec).map_err(|e|e.to_string())?;
   for sample in data{wav.write_sample(sample).map_err(|e|e.to_string())?}
   wav.finalize().map_err(|e|e.to_string())?;fs::rename(&temporary,&path).map_err(|e|e.to_string())?;
   let valid=PREVIEW.lock().unwrap().active.as_ref().is_some_and(|a|a.spec.id==spec.id&&a.spec.epoch==EPOCH.load(Ordering::SeqCst)&&a.inflight.as_deref()==Some(id.as_str()));
   if !valid{return Err("Aperçu devenu obsolète".into())}
   let worker=ensure_worker(&handle,&spec.python)?;
   worker.send(json!({"op":"transcribe","id":id,"path":path,"model":spec.model,"language":spec.language,"hotwords":spec.hotwords,"preview":true}))?;
   let still_live=PREVIEW.lock().unwrap().active.as_ref().is_some_and(|a|a.spec.id==spec.id&&a.spec.epoch==EPOCH.load(Ordering::SeqCst)&&a.inflight.as_deref()==Some(id.as_str()));
   if !still_live{let _=worker.send(json!({"op":"cancel","id":id}));}
   Ok(())
  })();
  if send.is_err(){preview_event(&handle,&json!({"event":"cancelled","id":id}));remove_preview_file(&path);}
 });
}
fn combine_preview(active:&mut PreviewCapture,start:f64,event:&Value)->String{
 // Freeze words by their audio timestamps as the bounded context advances.
 // No lexical deduplication: intentional repeated words keep their timestamps.
 let cutoff=start+0.6;
 for word in &active.words{
  if word.end<=cutoff&&word.end>active.prefix_end{
   active.prefix.push_str(&word.word);active.prefix_end=word.end;
  }
 }
 if let Some(words)=event["words"].as_array(){
  active.words=words.iter().filter_map(|word|{
   let value=PreviewWord{word:word["word"].as_str()?.into(),start:start+word["start"].as_f64()?,end:start+word["end"].as_f64()?};
   if !value.start.is_finite()||!value.end.is_finite()||value.end<value.start||value.word.len()>1000{return None}
   ((value.start+value.end)/2.0>=active.prefix_end+0.005).then_some(value)
  }).collect();
  format!("{}{}",active.prefix,active.words.iter().map(|w|w.word.as_str()).collect::<String>()).trim().to_string()
 }else{
  // Older workers still produce a usable current-window hypothesis.
  format!("{} {}",active.prefix,event["text"].as_str().unwrap_or("")).trim().to_string()
 }
}
fn preview_event(app:&AppHandle,event:&Value)->bool{
 let id=event["id"].as_str().unwrap_or("");if !id.starts_with("preview-"){return false}
 let name=event["event"].as_str().unwrap_or("");let terminal=matches!(name,"result"|"error"|"cancelled");
 let (text,cleanup,session,epoch,generation,speech_phrase)={
  let mut p=PREVIEW.lock().unwrap();let Some(job)=p.jobs.get(id).cloned()else{return true};
  let mut text=None;
  if let Some(active)=p.active.as_mut().filter(|a|preview_matches(a,&job,id,EPOCH.load(Ordering::SeqCst))){
   if matches!(name,"progress"|"result"){text=Some(combine_preview(active,job.start,event));}
   if terminal{active.inflight=None;}
  }
  let cleanup=if terminal{p.jobs.remove(id).map(|j|j.path)}else{None};(text,cleanup,job.session,job.epoch,job.generation,job.speech_phrase)
 };
 if let Some(path)=cleanup{remove_preview_file(&path); }
 if let Some(text)=text{
  {let mut r=RUNTIME.lock().unwrap();if epoch!=EPOCH.load(Ordering::SeqCst)||r.active.as_ref().is_none_or(|a|a.spec.id!=session||a.spec.epoch!=epoch){return true}r.presentation.preview(&session,text.clone());}
  inline_preview_write(app,&session,epoch,generation,speech_phrase.as_deref(),&text);
  publish(app);
 }
 true
}


// DELIVERY serializes preview, final replacement and removal. Native UIA calls
// must never hold RUNTIME or PREVIEW, including when they wait for focus.
fn inline_preview_write(app:&AppHandle,session:&str,epoch:u64,generation:u64,speech_phrase:Option<&str>,text:&str){
    let Ok(delivery)=DELIVERY.lock() else{return};
    let context={
        let runtime=RUNTIME.lock().unwrap();
        let Some(active)=runtime.active.as_ref().filter(|a|a.spec.id==session&&a.spec.epoch==epoch)else{return};
        if !streams_phrases(&active.spec.kind,&active.spec.mode)||STOP_REQUESTED.load(Ordering::SeqCst){return}
        let pending=runtime.current.is_some()||!runtime.jobs.is_empty();
        let Some(ticket)=runtime.inline.propose(session,epoch,generation,text,pending,speech_phrase)else{return};
        if !runtime.inline.accepts(&ticket,EPOCH.load(Ordering::SeqCst),pending){return}
        let Some((_,path))=runtime.phrase_audio.as_ref().filter(|(phrase,_)|phrase==&ticket.phrase)else{return};
        let Target::Application{handle}= &active.spec.target else{return};
        if handle.is_empty(){return}
        InlineContext{ticket,spec:active.spec.clone(),audio_path:path.clone()}
    };
    let Target::Application{handle}= &context.spec.target else{return};
    let outcome=crate::dictation_target::preview_at_cursor(handle.clone(),context.ticket.text.clone());
    let failed={
        let mut runtime=RUNTIME.lock().unwrap();
        match outcome{
            Ok(true)=>{runtime.inline.applied(context.ticket.clone());runtime.inline_context=Some(context);false},
            Ok(false)=>{runtime.inline.unsupported();false},
            Err(error)=>{
                runtime.inline.rejected(context.ticket.clone());runtime.inline_context=Some(context);
                runtime.last_error=Some(error.clone());
                runtime.state.message=format!("Aperçu interrompu · texte conservé : {error}");true
            }
        }
    };
    drop(delivery);
    if failed{
        STOP_REQUESTED.store(true,Ordering::SeqCst);
        let handle=app.clone();std::thread::spawn(move||{let _=stop_impl(&handle,false);});
    }
}
fn clear_inline_context(runtime:&mut Runtime,session:&str,phrase:&str){
    runtime.inline.resolved(session,phrase);
    if runtime.inline_context.as_ref().is_some_and(|c|c.ticket.session==session&&c.ticket.phrase==phrase){runtime.inline_context=None;}
}
// Caller holds DELIVERY. False from the native API means no owned composition;
// tainted/uncertain writes are errors and must remain recoverable.
fn discard_inline_locked(expected:Option<(&str,&str)>)->Result<(),String>{
    let context={
        let runtime=RUNTIME.lock().unwrap();
        runtime.inline_context.as_ref().filter(|c|runtime.inline.matches(&c.ticket.session,&c.ticket.phrase)
            &&expected.is_none_or(|(session,phrase)|c.ticket.session==session&&c.ticket.phrase==phrase)).cloned()
    };
    let Some(context)=context else{return Ok(())};
    let Target::Application{handle}= &context.spec.target else{return Ok(())};
    let result=crate::dictation_target::discard_preview_at_cursor(handle.clone());
    if let Err(error)=result{
        RUNTIME.lock().unwrap().inline.rejected(context.ticket.clone());
        return Err(error);
    }
    clear_inline_context(&mut RUNTIME.lock().unwrap(),&context.ticket.session,&context.ticket.phrase);
    Ok(())
}
fn inline_recovery_job(context:&InlineContext,existing:Option<&Job>,error:&str,epoch:u64)->Job{
    let mut job=existing.cloned().unwrap_or_else(||Job{
        id:context.ticket.phrase.clone(),spec:context.spec.clone(),audio_path:context.audio_path.clone(),
        duration_secs:0.0,status:"result".into(),text:String::new(),error:None,provisional:true,
    });
    let provisional=job.provisional||job.text.is_empty();
    if job.text.is_empty(){job.text=context.ticket.text.clone();}
    job.provisional=provisional;
    job.status=if job.text.is_empty(){"error"}else{"result"}.into();
    job.spec.epoch=epoch;
    job.error=Some(format!("{} Vérifie le contenu du champ avant de recopier. {error}",
        if provisional{"Brouillon provisoire conservé, non validé par la transcription finale."}else{"Texte final conservé."}));
    job
}
fn retain_inline_locked(error:&str)->Result<(),String>{
    let candidate={
        let runtime=RUNTIME.lock().unwrap();
        runtime.inline_context.as_ref().map(|context|inline_recovery_job(context,
            runtime.jobs.iter().find(|j|j.id==context.ticket.phrase),error,EPOCH.load(Ordering::SeqCst)))
    };
    let Some(mut job)=candidate else{return Ok(())};
    let saved=persist(&job);
    if let Err(ref error)=saved{job.error=Some(format!("{} {error}",job.error.as_deref().unwrap_or("Texte conservé.")));}
    let mut runtime=RUNTIME.lock().unwrap();
    if runtime.current.as_deref()==Some(job.id.as_str()){runtime.current=None;}
    runtime.presentation.end_final(&job.id);
    if let Some(existing)=runtime.jobs.iter_mut().find(|j|j.id==job.id){*existing=job;}else{runtime.jobs.push_back(job);}
    saved
}
// An unexpected engine loss is not a user discard: keep recognized draft
// text durably even when the native span can be safely removed.
fn preserve_inline_on_engine_failure(session:Option<&str>,reason:&str){
    let Ok(_delivery)=DELIVERY.lock()else{return};
    let relevant=RUNTIME.lock().unwrap().inline_context.as_ref().is_some_and(|c|session.is_none_or(|id|c.ticket.session==id));
    if !relevant{return}
    if let Err(error)=pipeline::preserve_before_removal(||retain_inline_locked(reason),||discard_inline_locked(None)){
        let _=retain_inline_locked(&format!("{reason} {error}"));
    }
}
// Copy/note/discard is an explicit resolution. If the user edited the draft,
// archive the result before relinquishing tracking; never erase unknown text.
fn settle_manual_inline_locked(job:&Job)->Result<(),String>{
    if discard_inline_locked(Some((&job.spec.id,&job.id))).is_err(){
        let state=snapshot();
        save_recovery(&job_dir().join("prises-conservees"),&VecDeque::from([job.clone()]),&state)?;
        clear_inline_context(&mut RUNTIME.lock().unwrap(),&job.spec.id,&job.id);
    }
    Ok(())
}
fn clear_empty_inline(app:&AppHandle,session:&str,phrase:&str){
    let Ok(delivery)=DELIVERY.lock()else{return};
    let failed=if let Err(error)=discard_inline_locked(Some((session,phrase))){
        let _=retain_inline_locked(&error);true
    }else{false};
    drop(delivery);
    if failed{STOP_REQUESTED.store(true,Ordering::SeqCst);let handle=app.clone();std::thread::spawn(move||{let _=stop_impl(&handle,false);});}
}

struct Worker {
    id: u64, python: String, stdin: Mutex<ChildStdin>, child: Mutex<Child>, alive: AtomicBool,
}
impl Worker {
    fn send(&self, value: Value) -> Result<(), String> {
        if !self.alive.load(Ordering::SeqCst) { return Err("Le moteur local s'est arrêté. Réessaie la transcription.".into()); }
        let mut input = self.stdin.lock().map_err(|_| "Entrée du moteur verrouillée")?;
        let mut bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        input.write_all(&bytes).and_then(|_| input.flush()).map_err(|e| format!("Moteur local déconnecté : {e}"))
    }
    fn kill(&self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock() { let _ = child.kill(); let _ = child.wait(); }
    }
}
fn ensure_worker(app: &AppHandle, python: &str) -> Result<Arc<Worker>, String> {
    let mut slot = WORKER.lock().map_err(|_| "Moteur verrouillé")?;
    if let Some(worker) = slot.as_ref() {
        if worker.python == python && worker.alive.load(Ordering::SeqCst) { return Ok(worker.clone()); }
        worker.kill();
    }
    let script = workspace::portable_root().join("moteur").join("echo_worker.py");
    if !script.is_file() { return Err(format!("Moteur introuvable : {}", script.display())); }
    let mut command = Command::new(python);
    command.arg("-u").arg(&script).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
        .env("PYTHONIOENCODING", "utf-8").env("HF_HUB_OFFLINE", "1").env("TRANSFORMERS_OFFLINE", "1");
    #[cfg(windows)] { use std::os::windows::process::CommandExt; command.creation_flags(0x08000000); }
    let mut child = command.spawn().map_err(|e| format!("Impossible de lancer Python. Choisis l'interpréteur du moteur dans les réglages. ({e})"))?;
    let stdout = child.stdout.take().ok_or("Sortie du moteur absente")?;
    let stdin = child.stdin.take().ok_or("Entrée du moteur absente")?;
    let id = WORKER_ID.fetch_add(1, Ordering::SeqCst) + 1;
    let worker = Arc::new(Worker { id, python: python.into(), stdin: Mutex::new(stdin), child: Mutex::new(child), alive: AtomicBool::new(true) });
    let reader_worker = worker.clone(); let handle = app.clone();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if line.len() > 2_000_000 { break; }
            if let Ok(event) = serde_json::from_str::<Value>(&line) {
                if WORKER_ID.load(Ordering::SeqCst) == id { worker_event(&handle, event); }
            }
        }
        reader_worker.alive.store(false, Ordering::SeqCst);
        if WORKER_ID.load(Ordering::SeqCst) == id && !SHUTDOWN.load(Ordering::SeqCst) {
            worker_lost(&handle);
        }
    });
    *slot = Some(worker.clone());
    Ok(worker)
}
fn worker_lost(app: &AppHandle) {
    preserve_inline_on_engine_failure(None,"Le moteur s’est arrêté avant la validation finale.");
    preview_cleanup_all();
    { let mut runtime = RUNTIME.lock().unwrap();
        runtime.state.engine = "error".into(); runtime.vad_ready = None;
        runtime.last_error = Some("Le moteur local s'est arrêté. Les enregistrements sont conservés.".into());
        runtime.current = None;
        runtime.presentation.cancel();
        for job in runtime.jobs.iter_mut().filter(|j| j.status == "transcribing" || j.status == "queued") {
            job.status = "error".into(); job.error = Some("Moteur arrêté. Réessaie avec l'audio conservé.".into()); let _ = persist(job);
        }
    }
    STOP_REQUESTED.store(true, Ordering::SeqCst);
    let handle = app.clone(); std::thread::spawn(move || { let _ = stop_impl(&handle, true); });
    publish(app);
}

fn session_mode(kind:&str, configured:&str, requested:Option<&str>)->Result<String,String>{
 let mode=if kind=="recording"{"toggle"}else{requested.unwrap_or(configured)};
 if !matches!(mode,"hold"|"toggle"|"handsfree"){return Err("Mode de dictée inconnu".into())}
 Ok(mode.into())
}
fn streams_phrases(kind:&str,mode:&str)->bool{
    kind=="dictation" && matches!(mode,"toggle"|"handsfree")
}
fn capture_limit_secs(kind:&str)->u64{
    if kind=="recording"{3600}else{DICTATION_LIMIT_SECS}
}
#[cfg(test)]
mod direct_click_tests{
 use super::{session_mode,streams_phrases,capture_limit_secs};
 #[test]fn click_can_listen_continuously_even_with_legacy_hold_preference(){
  assert_eq!(session_mode("dictation","hold",Some("handsfree")).unwrap(),"handsfree");
  assert_eq!(session_mode("dictation","hold",None).unwrap(),"hold");
 }
 #[test]fn audio_recording_and_invalid_overrides_have_defined_modes(){
  assert_eq!(session_mode("recording","handsfree",Some("handsfree")).unwrap(),"toggle");
  assert!(session_mode("dictation","hold",Some("invalid")).is_err());
 }
 #[test]fn toggle_and_handsfree_stream_phrases_without_changing_the_gesture(){
  for mode in ["toggle","handsfree"]{
   assert!(streams_phrases("dictation",mode));
   assert_eq!(session_mode("dictation","hold",Some(mode)).unwrap(),mode);
   assert!(!streams_phrases("recording",mode));
  }
  assert!(!streams_phrases("dictation","hold"));
  assert!(!streams_phrases("recording","toggle"));
 }
 #[test]fn dictation_is_bounded_to_five_minutes_and_recording_keeps_one_hour(){
  assert_eq!(capture_limit_secs("dictation"),300);
  assert_eq!(capture_limit_secs("recording"),3600);
 }
}
fn session_target(kind: &str, fallback: &FrozenNote) -> Result<(Target, String), String> {
    if kind != "recording" {
        Ok((Target::Application { handle: crate::dictation_target::freeze_cursor()? }, "Champ où tu écris".into()))
    } else {
        Ok((Target::Note { note: fallback.clone() }, format!("Note · {}", fallback.path)))
    }
}
fn session_spec(kind: String, mode_override: Option<String>) -> Result<(SessionSpec, Option<String>, u64), String> {
    let settings = workspace::settings()?;
    let note = workspace::freeze_note()?;
    let fallback = FrozenNote { workspace: note.workspace, path: note.path };
    let mode = session_mode(&kind, &settings.voice_mode, mode_override.as_deref())?;
    let (target, label) = session_target(&kind, &fallback)?;
    Ok((SessionSpec { id: uuid::Uuid::new_v4().to_string(), mode, kind,
        workspace: settings.workspace, target, fallback, target_label: label,
        python: settings.python, model: settings.model, language: settings.language,
        keep_audio: settings.keep_audio, epoch: EPOCH.load(Ordering::SeqCst), hotwords: settings.personal_words.join(", ") }, settings.input_device, settings.vad_silence_ms))
}
fn level(app: &AppHandle, session: &str, rms: f32) {
    { let mut runtime = RUNTIME.lock().unwrap();
        if runtime.active.as_ref().is_none_or(|a| a.spec.id != session) { return; }
        runtime.state.level = (rms * 5.0).clamp(0.0, 1.0);
    }
    publish(app);
}
fn begin_start() -> u64 {
    STOP_REQUESTED.store(false, Ordering::SeqCst);
    START_SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1
}
// Called with CONTROL held and no active capture. A new click never delivers an
// old result to the newly selected field. Recovery is durable before dequeueing.
fn preserve_pending_before_start() -> Result<(), String> {
    let _delivery = DELIVERY.lock().map_err(|_| "Restitution verrouillée")?;
    if !RUNTIME.lock().unwrap().jobs.iter().any(Job::blocked) { return Ok(()); }
    let _=discard_inline_locked(None);
    let mut runtime = RUNTIME.lock().unwrap();
    refresh(&mut runtime);
    save_recovery(&job_dir().join("prises-conservees"), &runtime.jobs, &runtime.state)?;
    if let Some(context)=runtime.inline_context.clone(){clear_inline_context(&mut runtime,&context.ticket.session,&context.ticket.phrase);}
    runtime.inline.end_capture();
    EPOCH.fetch_add(1, Ordering::SeqCst);
    runtime.current = None;
    runtime.presentation.cancel();
    let jobs = runtime.jobs.drain(..).collect::<Vec<_>>();
    for job in &jobs {
        runtime.sessions.remove(&job.spec.id);
        // The recovery bundle and the original WAV are retained independently
        // of the live journal, including untranscribed queued takes.
        let mut completed = job.clone(); completed.status = "cancelled".into();
        if persist(&completed).is_ok() { let _ = fs::remove_file(job_dir().join(format!("{}.json", job.id))); }
    }
    runtime.state.message = "Anciennes prises conservées · nouvelle dictée".into();
    drop(runtime);
    for job in jobs {
        if let Some(worker) = WORKER.lock().unwrap().clone() {
            let _ = worker.send(json!({"op":"cancel","id":job.id}));
        }
    }
    Ok(())
}
fn save_recovery(directory: &Path, jobs: &VecDeque<Job>, state: &VoiceState) -> Result<PathBuf, String> {
    fs::create_dir_all(directory).map_err(|e| format!("Conservation des prises impossible : {e}"))?;
    let name = uuid::Uuid::new_v4().to_string();
    let bundle = serde_json::to_vec_pretty(&json!({"version":1,"jobs":jobs,"state":state})).map_err(|e|e.to_string())?;
    let mut text = String::from("PRISES CONSERVÉES — ÉCHO\n\nCes prises n’ont pas été envoyées au nouveau curseur.\nLes fichiers audio d’origine sont conservés aux chemins indiqués.\n\n");
    for job in jobs { text.push_str(&format!("Prise : {}\nTexte : {}\nAudio : {}\nÉtat : {}\n\n",job.id,job.text,job.audio_path,job.status)); }
    let path = directory.join(format!("{name}.json"));
    crate::atomic::write_atomic(&path, &bundle).map_err(|e|format!("Prises non sauvegardées : {e}"))?;
    crate::atomic::write_atomic(&directory.join(format!("{name}.txt")), text.as_bytes()).map_err(|e|format!("Prises non sauvegardées : {e}"))?;
    Ok(path)
}
fn start_impl(app: &AppHandle, kind: String, request: u64, mode_override: Option<String>) -> Result<bool, String> {
    crate::translation::shutdown(app);
    let control = CONTROL.lock().map_err(|_| "Commande vocale verrouillée")?;
    { let runtime = RUNTIME.lock().unwrap();
        if runtime.active.is_some() || runtime.arming { return Ok(false); }
        if !runtime.jobs.iter().any(Job::blocked) && runtime.jobs.len() >= MAX_JOBS { return Err("La file est pleine. Attends la fin de la transcription.".into()); }
    }
    if START_SEQUENCE.load(Ordering::SeqCst) != request { return Ok(false); }
    let (mut spec, device, silence_ms) = session_spec(kind, mode_override)?;
    if START_SEQUENCE.load(Ordering::SeqCst) != request { return Ok(false); }
    preserve_pending_before_start()?;
    spec.epoch = EPOCH.load(Ordering::SeqCst);
    { let mut runtime = RUNTIME.lock().unwrap(); runtime.arming = true; runtime.last_error = None;
        runtime.state.text.clear(); runtime.presentation.clear_preview(); runtime.state.audio_path = None;
        runtime.state.mode = spec.mode.clone(); runtime.state.kind = spec.kind.clone();
        runtime.speech = false; runtime.inline.begin_session(&spec.id, spec.epoch); runtime.phrase_audio = None;
        runtime.state.target_label = spec.target_label.clone(); runtime.state.message = "Ouverture du micro…".into();
        runtime.sessions.insert(spec.id.clone(), spec.clone());
    }
    publish(app);
    if spec.kind == "dictation" { preview_begin(&spec); }
    let result: Result<Option<capture::StreamCapture>, String> = (|| {
        if streams_phrases(&spec.kind,&spec.mode) {
            let worker = ensure_worker(app, &spec.python)?;
            worker.send(json!({"op":"listen_start","id":spec.id,"directory":capture::recordings_dir(Path::new(&spec.workspace)),"silence_ms":silence_ms,"max_phrase_secs":DICTATION_LIMIT_SECS}))?;
            let until = Instant::now() + Duration::from_secs(12);
            loop {
                let (ready, error) = { let runtime = RUNTIME.lock().unwrap(); (runtime.vad_ready.as_ref() == Some(&spec.id), runtime.last_error.clone()) };
                if ready { break; }
                if let Some(error) = error { return Err(error); }
                if STOP_REQUESTED.load(Ordering::SeqCst) { return Err("Écoute annulée.".into()); }
                if Instant::now() > until { return Err("La détection de parole n'a pas répondu. Vérifie le moteur dans les réglages.".into()); }
                std::thread::sleep(Duration::from_millis(25));
            }
            let id = spec.id.clone(); let handle = app.clone(); let last_emit = Mutex::new(Instant::now());
            let stream = capture::start_stream(device, move |samples| {
                if last_emit.lock().unwrap().elapsed() > Duration::from_millis(100) {
                    *last_emit.lock().unwrap() = Instant::now(); level(&handle, &id, capture::rms(&samples));
                }
                preview_feed(&handle, &id, &samples);
                let bytes: Vec<u8> = samples.iter().flat_map(|sample| sample.to_le_bytes()).collect();
                worker.send(json!({"op":"pcm","id":id,"data":base64::engine::general_purpose::STANDARD.encode(bytes)}))
            })?;
            Ok(Some(stream))
        } else {
            let handle = app.clone(); let id = spec.id.clone();
            let preview_app=app.clone();let preview_id=spec.id.clone();let dictation=spec.kind=="dictation";
            capture::start_with_preview(capture::recordings_dir(Path::new(&spec.workspace)), device,
                move |rms| level(&handle, &id, rms),
                move |samples|{if dictation{preview_feed(&preview_app,&preview_id,samples)}})?;
            Ok(None)
        }
    })();
    match result {
        Ok(stream) => { let mut runtime = RUNTIME.lock().unwrap();
            runtime.arming = false;
            runtime.presentation.begin_session(&spec.id);
            runtime.state.message = if streams_phrases(&spec.kind,&spec.mode) { "Écoute active · une pause transmet chaque phrase" } else if spec.kind == "recording" { "Enregistrement en cours" } else { "Parle, puis relâche ou arrête" }.into();
            runtime.active = Some(Active { spec, started: Instant::now(), stream });
        }
        Err(error) => {
            preview_stop(&spec.id);
            { let mut runtime = RUNTIME.lock().unwrap(); runtime.arming = false; runtime.inline.end_capture(); runtime.sessions.remove(&spec.id); }
            if streams_phrases(&spec.kind,&spec.mode) {
                if let Some(worker) = WORKER.lock().unwrap().clone() {
                    let _ = worker.send(json!({"op":"listen_stop","id":spec.id,"cancelled":true}));
                }
            }
            if STOP_REQUESTED.load(Ordering::SeqCst) || START_SEQUENCE.load(Ordering::SeqCst) != request {
                RUNTIME.lock().unwrap().state.message = "Écoute annulée · micro fermé".into(); publish(app); return Ok(false);
            }
            return Err(error);
        }
    }
    publish(app);
    drop(control);
    if STOP_REQUESTED.load(Ordering::SeqCst) || START_SEQUENCE.load(Ordering::SeqCst) != request { stop_impl(app, false)?; return Ok(false); }
    Ok(true)
}

fn prepare_queued_job(mut job:Job,epoch:u64,known:bool,save:impl FnOnce(&Job)->Result<(),String>)->Option<Job>{
    // A late utterance must not overwrite a retained result under its UUID,
    // including a provisional recovery created before that utterance arrived.
    if known||job.spec.epoch!=epoch{return None}
    if let Err(error)=save(&job){job.status="error".into();job.error=Some(error);}
    Some(job)
}
fn queue_job(app: &AppHandle, job: Job) {
    // Cancel cannot drain between the epoch check, durable write and enqueue.
    let _delivery = DELIVERY.lock().unwrap();
    let known=RUNTIME.lock().unwrap().jobs.iter().any(|j|j.id==job.id);
    let Some(mut job)=prepare_queued_job(job,EPOCH.load(Ordering::SeqCst),known,persist)else{return};
    let full = { let mut runtime = RUNTIME.lock().unwrap();
        let full = runtime.jobs.len() >= MAX_JOBS;
        if full { job.status = "error".into(); job.error = Some("File pleine : audio conservé pour une reprise.".into()); let _ = persist(&job); }
        runtime.jobs.push_back(job); full
    };
    if full { STOP_REQUESTED.store(true, Ordering::SeqCst); let handle = app.clone(); std::thread::spawn(move || { let _ = stop_impl(&handle, false); }); }
    dispatch_next(app); publish(app);
}
fn stop_impl(app: &AppHandle, cancelled: bool) -> Result<(), String> {
    let control = CONTROL.lock().map_err(|_| "Commande vocale verrouillée")?;
    let active = { let mut runtime = RUNTIME.lock().unwrap(); runtime.speech = false; runtime.presentation.end_capture(); runtime.inline.end_capture(); runtime.active.take() };
    let Some(active) = active else { return Ok(()); };
    preview_stop(&active.spec.id);
    if let Some(stream) = active.stream {
        let stream_result = stream.stop();
        let worker = WORKER.lock().unwrap().clone();
        if let Some(worker) = worker {
            if worker.send(json!({"op":"listen_stop","id":active.spec.id,"cancelled":cancelled})).is_ok() {
                // A stop/quit does not overtake the final WAV being sealed.
                let until = Instant::now() + Duration::from_secs(3);
                while Instant::now() < until && worker.alive.load(Ordering::SeqCst) {
                    if !RUNTIME.lock().unwrap().sessions.contains_key(&active.spec.id) { break; }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        { let mut runtime = RUNTIME.lock().unwrap(); runtime.state.message = if cancelled { "Écoute annulée" } else { "Micro fermé" }.into(); }
        stream_result?;
    } else {
        let info = capture::stop()?;
        RUNTIME.lock().unwrap().sessions.remove(&active.spec.id);
        { let mut runtime = RUNTIME.lock().unwrap(); runtime.state.audio_path = Some(info.path.clone()); runtime.state.duration_secs = info.duration_secs; }
        if active.spec.kind == "recording" || cancelled {
            RUNTIME.lock().unwrap().state.message = if cancelled { "Prise annulée · audio conservé" } else { "Enregistrement rangé dans le dossier audio" }.into();
        } else if info.duration_secs >= 0.1 {
            let job = Job { id: active.spec.id.clone(), spec: active.spec, audio_path: info.path,
                duration_secs: info.duration_secs, status: "queued".into(), text: String::new(), error: None, provisional: false };
            drop(control); queue_job(app, job); let _ = app.emit("recordings:changed", ()); return Ok(());
        } else { RUNTIME.lock().unwrap().state.message = "Prise trop courte · recommence en maintenant la touche".into(); }
        let _ = app.emit("recordings:changed", ());
    }
    drop(control); publish(app); Ok(())
}
fn dispatch_next(app: &AppHandle) {
    let next = { let mut runtime = RUNTIME.lock().unwrap();
        if runtime.current.is_some() || runtime.jobs.iter().any(Job::blocked) { return; }
        let Some(job) = runtime.jobs.iter_mut().find(|j| j.status == "queued") else { return; };
        job.status = "transcribing".into(); let next = job.clone();
        runtime.current = Some(next.id.clone()); runtime.state.progress = None;
        runtime.presentation.begin_final(&next.id);
        // Keep the last hypothesis visible while the full durable WAV is decoded.
        runtime.state.message = "Transcription locale…".into(); next
    };
    if let Err(error) = persist(&next).and_then(|_| ensure_worker(app, &next.spec.python))
        .and_then(|worker| worker.send(json!({"op":"transcribe","id":next.id,"path":next.audio_path,"model":next.spec.model,"language":next.spec.language,"hotwords":next.spec.hotwords}))) {
        job_error(app, &next.id, error);
    }
    publish(app);
}
fn job_error(app: &AppHandle, id: &str, message: String) {
    { let mut runtime = RUNTIME.lock().unwrap();
        let inline=runtime.inline_context.as_ref().filter(|c|c.ticket.phrase==id).cloned();
        if let Some(job) = runtime.jobs.iter_mut().find(|j| j.id == id) {
            let error=if let Some(context)=inline{
                if job.text.is_empty(){job.text=context.ticket.text;job.provisional=true;format!("Brouillon provisoire conservé, non validé par la transcription finale. {message}")}
                else{format!("{} {message}",job.error.as_deref().unwrap_or("Texte conservé."))}
            }else{message.clone()};
            job.status = "error".into(); job.error = Some(error); let _ = persist(job);
        } else { runtime.last_error = Some(message.clone()); }
        if runtime.current.as_deref() == Some(id) { runtime.current = None; }
        runtime.presentation.end_final(id);
        runtime.state.message = message;
    }
    STOP_REQUESTED.store(true, Ordering::SeqCst);
    let handle = app.clone(); std::thread::spawn(move || { let _ = stop_impl(&handle, false); });
    publish(app);
}
fn finish_job(app: &AppHandle, id: &str, action: &str) {
    let discarded = action == "discard";
    let job = { let mut runtime = RUNTIME.lock().unwrap();
        let Some(index) = runtime.jobs.iter().position(|j| j.id == id) else { return; };
        let mut job = runtime.jobs.remove(index).unwrap();
        job.status = if discarded { "discarded" } else { "done" }.into();
        runtime.current = None; runtime.last_error = None; runtime.state.error = None;
        runtime.presentation.end_final(id);
        if !discarded { runtime.presentation.sent(&job.spec.id, action, &job.text); }
        if runtime.active.is_none(){runtime.presentation.clear_preview();} runtime.state.text = if discarded { String::new() } else { job.text.clone() };
        runtime.state.message = if discarded { "Résultat écarté · audio conservé" } else if runtime.active.is_some() { "Phrase transmise · écoute active" } else { "Texte transmis · micro prêt" }.into();
        job
    };
    // Mark completed before removing the journal: a failed remove is harmless.
    if persist(&job).is_ok() { let _ = fs::remove_file(job_dir().join(format!("{}.json", job.id))); }
    if !discarded && !job.spec.keep_audio && job.spec.kind == "dictation" {
        if audio_belongs_to_workspace(&job.audio_path, &job.spec.workspace) { let _ = fs::remove_file(&job.audio_path); }
    }
    workspace::emit_notes_changed();
    let _ = app.emit("recordings:changed", ());
    dispatch_next(app); publish(app);
}
fn audio_belongs_to_workspace(audio: &str, folder: &str) -> bool {
    let Ok(root) = capture::recordings_dir(Path::new(folder)).canonicalize() else { return false; };
    let Ok(path) = Path::new(audio).canonicalize() else { return false; };
    path.starts_with(root) && path.extension().is_some_and(|extension| extension == "wav")
}
fn deliver(app: &AppHandle, id: &str, action: &str) -> Result<(), String> {
    let _delivery = DELIVERY.lock().map_err(|_| "Restitution verrouillée")?;
    deliver_locked(app,id,action)
}
fn deliver_locked(app: &AppHandle, id: &str, action: &str) -> Result<(), String> {
    let mut job = { let runtime = RUNTIME.lock().unwrap(); runtime.jobs.iter().find(|j| j.id == id).cloned().ok_or("Résultat déjà traité")? };
    if job.spec.epoch != EPOCH.load(Ordering::SeqCst) { return Err("Cette transcription a été annulée.".into()); }
    if action == "discard" { settle_manual_inline_locked(&job)?; finish_job(app, id, action); return Ok(()); }
    if job.text.trim().is_empty() { return Err("Aucun texte à transmettre. Réessaie la transcription ou écarte la prise.".into()); }
    if job.provisional&&matches!(action,"auto"|"insert"){
        return Err("Ce texte est encore un brouillon provisoire. Réessaie la transcription finale avant de l’insérer, ou utilise Copier après vérification.".into());
    }
    job.status = "delivering".into(); persist(&job)?;
    let inline_allowed={let runtime=RUNTIME.lock().unwrap();runtime.inline.can_commit(&job.spec.id,&job.id)};
    let result = match action {
        "copy" => arboard::Clipboard::new().map_err(|e| e.to_string()).and_then(|mut clipboard| clipboard.set_text(job.text.clone()).map_err(|e| e.to_string())),
        "note" => workspace::append_transcript(&job.spec.fallback.target(), &job.text, &job.id).map(|_| ()),
        "auto" | "insert" => match &job.spec.target {
            Target::Note { note } => workspace::append_transcript(&note.target(), &job.text, &job.id).map(|_| ()),
            Target::Application { handle } => if handle.is_empty(){Err("Le champ de saisie n’a pas été confirmé. Ton texte est conservé : utilise Copier.".into())}
            else if !inline_allowed{Err("Le brouillon dans le champ a changé. La transcription finale est conservée ; vérifie le champ avant de recopier.".into())}
            else{crate::dictation_target::insert_at_cursor(handle.clone(), if streams_phrases(&job.spec.kind,&job.spec.mode){format!("{} ",job.text.trim_end())}else{job.text.clone()})},
        },
        _ => Err("Action de restitution inconnue".into()),
    };
    match result {
        Ok(()) => {
            if matches!(action,"auto"|"insert"){
                clear_inline_context(&mut RUNTIME.lock().unwrap(),&job.spec.id,&job.id);
            }else{settle_manual_inline_locked(&job)?;}
            finish_job(app, id, action); Ok(())
        }
        Err(error) => {
            { let mut runtime = RUNTIME.lock().unwrap();
                if matches!(action,"auto"|"insert"){
                    if let Some(context)=runtime.inline_context.as_ref().filter(|c|c.ticket.phrase==id).cloned(){runtime.inline.rejected(context.ticket);}
                }
                if let Some(job) = runtime.jobs.iter_mut().find(|j| j.id == id) {
                    job.status = "result".into(); job.error = Some(error.clone()); let _ = persist(job);
                }
                runtime.current = None; runtime.presentation.end_final(id); runtime.state.message = "Texte conservé · choisis Copier ou Note".into();
            }
            STOP_REQUESTED.store(true, Ordering::SeqCst);
            let handle = app.clone(); std::thread::spawn(move || { let _ = stop_impl(&handle, false); });
            publish(app); Err(error)
        }
    }
}

fn worker_event(app: &AppHandle, event: Value) {
    if preview_event(app,&event){return}
    let name = event["event"].as_str().unwrap_or("");
    let id = event["id"].as_str().unwrap_or("").to_owned();
    if matches!(name, "result" | "error" | "cancelled") { CANCEL_WAIT.lock().unwrap().remove(&id); }
    match name {
        "engine" => { let mut runtime = RUNTIME.lock().unwrap(); runtime.state.engine = event["status"].as_str().unwrap_or("cold").into(); }
        "preloaded" => { RUNTIME.lock().unwrap().state.engine = "ready".into(); }
        "listening" => { let mut runtime = RUNTIME.lock().unwrap(); runtime.vad_ready = Some(id); }
        "listen_stopped" => { let mut runtime = RUNTIME.lock().unwrap(); runtime.sessions.remove(&id); if runtime.vad_ready.as_ref() == Some(&id) { runtime.vad_ready = None; } }
        "speech_start" => {
            let session=event["session"].as_str().unwrap_or("");
            let mut runtime=RUNTIME.lock().unwrap();
            let spec=runtime.sessions.get(session).cloned();
            if let Some(spec)=spec.filter(|spec|spec.epoch==EPOCH.load(Ordering::SeqCst)
                &&(runtime.arming||runtime.active.as_ref().is_some_and(|a|a.spec.id==session))){
                runtime.speech=true;
                if uuid::Uuid::parse_str(&id).is_ok()&&runtime.inline.confirm_speech(session,spec.epoch,&id){
                    runtime.phrase_audio=Some((id.clone(),capture::recordings_dir(Path::new(&spec.workspace)).join(format!("{id}.wav")).to_string_lossy().into()));
                }
            }
        }
        "speech_empty" => {
            let session=event["session"].as_str().unwrap_or("");
            let spec={let mut runtime=RUNTIME.lock().unwrap();
                if runtime.active.as_ref().is_some_and(|a|a.spec.id==session){runtime.speech=false;}
                runtime.sessions.get(session).cloned()
            };
            clear_empty_inline(app,session,&id);
            if let Some(spec)=spec{preview_begin(&spec);}
        }
        "utterance" => {
            let session = event["session"].as_str().unwrap_or("");
            let spec = { let mut runtime = RUNTIME.lock().unwrap(); runtime.speech = false; runtime.sessions.get(session).cloned() };
            if let Some(spec) = spec {
                // Each durable hands-free phrase starts a fresh provisional view.
                if RUNTIME.lock().unwrap().active.as_ref().is_some_and(|a|a.spec.id==spec.id){preview_begin(&spec);}
                let path = event["path"].as_str().unwrap_or("").to_string();
                if !audio_belongs_to_workspace(&path, &spec.workspace) { job_error(app, &id, "Le moteur a renvoyé un chemin audio inattendu.".into()); return; }
                if event["cancelled"].as_bool().unwrap_or(false) { let _ = app.emit("recordings:changed", ()); return; }
                queue_job(app, Job { id, spec, audio_path: path,
                    duration_secs: event["duration_secs"].as_f64().unwrap_or(0.0), status: "queued".into(), text: String::new(), error: None, provisional: false });
            }
        }
        "started" | "progress" => {
            let mut runtime = RUNTIME.lock().unwrap();
            if runtime.jobs.iter().any(|j| j.id == id && j.spec.epoch == EPOCH.load(Ordering::SeqCst) && j.status == "transcribing") {
                runtime.state.progress = event["progress"].as_f64();
                runtime.presentation.final_progress(&id, event["text"].as_str().unwrap_or(""));
            }
        }
        "result" => {
            // Result journaling and final replacement form one cancellation
            // boundary; a late result cannot recreate a cancelled journal.
            let delivery=DELIVERY.lock().unwrap();
            let text = event["text"].as_str().unwrap_or("").trim().to_string();
            let job = { let mut runtime = RUNTIME.lock().unwrap();
                let Some(job) = runtime.jobs.iter_mut().find(|j| j.id == id && j.status == "transcribing" && j.spec.epoch == EPOCH.load(Ordering::SeqCst)) else { return; };
                job.text = text; job.provisional=false; job.status = "result".into();
                if job.text.is_empty() { job.error = Some("Aucune parole détectée. L'audio reste disponible.".into()); }
                let copy = job.clone(); runtime.current = None; copy
            };
            if let Err(error) = persist(&job) { drop(delivery); job_error(app, &id, error); return; }
            if !job.text.is_empty() {
                let outcome=deliver_locked(app,&id,"auto");
                drop(delivery);
                if let Err(error) = outcome {
                    // deliver has already exposed target failures. Journal
                    // failures occur before that boundary and need surfacing.
                    if RUNTIME.lock().unwrap().jobs.iter().any(|j| j.id == id && j.spec.epoch == EPOCH.load(Ordering::SeqCst) && j.error.is_none()) { job_error(app, &id, error); }
                }
            } else {
                drop(delivery);
                clear_empty_inline(app,&job.spec.id,&id);
                job_error(app, &id, "Aucune parole détectée. Réessaie ou écarte cette prise.".into());
            }
        }
        "error" => {
            let error = event["message"].as_str().unwrap_or("Le moteur a rencontré une erreur.").to_owned();
            let relevant = { let runtime = RUNTIME.lock().unwrap(); id.starts_with("preload-") || runtime.sessions.contains_key(&id) || runtime.jobs.iter().any(|j| j.id == id && j.spec.epoch == EPOCH.load(Ordering::SeqCst)) };
            if relevant {
                preserve_inline_on_engine_failure(Some(&id),&error);
                if id.starts_with("preload-") { RUNTIME.lock().unwrap().state.engine = "error".into(); }
                job_error(app, &id, error);
            }
        }
        _ => (),
    }
    publish(app);
}

#[tauri::command]
pub async fn voice_get() -> VoiceState { snapshot() }
#[tauri::command]
pub async fn voice_devices() -> Result<Vec<capture::InputDevice>, String> {
    tauri::async_runtime::spawn_blocking(capture::input_devices).await.map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn voice_start(app: AppHandle, kind: Option<String>, mode: Option<String>) -> Result<VoiceState, String> {
    let kind = kind.unwrap_or_else(|| "dictation".into());
    if !matches!(kind.as_str(), "dictation" | "recording") { return Err("Type de prise inconnu".into()); }
    let request = begin_start();
    tauri::async_runtime::spawn_blocking({ let app = app.clone(); move || start_impl(&app, kind, request, mode) }).await.map_err(|e| e.to_string())?
        .map_err(|error| { fail(&app, error.clone()); error })?;
    Ok(snapshot())
}
#[tauri::command]
pub async fn voice_stop(app: AppHandle) -> Result<VoiceState, String> {
    START_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    STOP_REQUESTED.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking({ let app = app.clone(); move || stop_impl(&app, false) }).await.map_err(|e| e.to_string())?
        .map_err(|error| { fail(&app, error.clone()); error })?;
    Ok(snapshot())
}
fn cancel_impl(app: &AppHandle) -> Result<(), String> {
    // An already executing native transaction may finish. Remove only its own
    // verified draft, then acknowledge cancellation; committed phrases remain.
    let delivery = DELIVERY.lock().map_err(|_| "Restitution verrouillée")?;
    let removal=discard_inline_locked(None);
    let (jobs,recovery)={
        let mut runtime=RUNTIME.lock().unwrap();
        let recovery=removal.as_ref().err().and_then(|error|runtime.inline_context.as_ref().map(|context|
            inline_recovery_job(context,runtime.jobs.iter().find(|j|j.id==context.ticket.phrase),error,EPOCH.load(Ordering::SeqCst))));
        runtime.current=None;runtime.last_error=None;runtime.state.text.clear();
        runtime.presentation.cancel();runtime.inline.end_capture();
        runtime.state.message=if recovery.is_some(){"Annulé · brouillon conservé pour vérification"}else{"Annulé · audio conservé"}.into();
        (runtime.jobs.drain(..).collect::<Vec<_>>(),recovery)
    };
    if let Some(worker)=WORKER.lock().unwrap().clone(){
        for job in &jobs{
            if job.status=="transcribing"{CANCEL_WAIT.lock().unwrap().insert(job.id.clone(),Instant::now());}
            let _=worker.send(json!({"op":"cancel","id":job.id}));
        }
    }
    for mut job in jobs{
        if recovery.as_ref().is_some_and(|kept|kept.id==job.id){continue}
        job.status="cancelled".into();
        if persist(&job).is_ok(){let _=fs::remove_file(job_dir().join(format!("{}.json",job.id)));}
    }
    if let Some(mut job)=recovery{
        if let Err(error)=persist(&job){job.error=Some(format!("{} {error}",job.error.as_deref().unwrap_or("Brouillon conservé.")));}
        RUNTIME.lock().unwrap().jobs.push_back(job);
    }
    drop(delivery);
    stop_impl(app,true)?;
    publish(app);Ok(())
}
#[tauri::command]
pub async fn voice_cancel(app: AppHandle) -> Result<VoiceState, String> {
    crate::dictation_target::cancel_pending();
    START_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    EPOCH.fetch_add(1, Ordering::SeqCst); STOP_REQUESTED.store(true, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking({ let app = app.clone(); move || cancel_impl(&app) }).await.map_err(|e| e.to_string())?
        .map_err(|error| { fail(&app, error.clone()); error })?;
    Ok(snapshot())
}
#[tauri::command]
pub async fn voice_retry(app: AppHandle) -> Result<VoiceState, String> {
    { let mut runtime = RUNTIME.lock().unwrap();
        let job = runtime.jobs.iter_mut().find(|j| j.blocked()).ok_or("Aucune prise à réessayer")?;
        if !audio_belongs_to_workspace(&job.audio_path, &job.spec.workspace) { return Err("L'enregistrement a été déplacé ou supprimé.".into()); }
        job.status = "queued".into(); job.error = None; job.text.clear(); job.spec.epoch = EPOCH.load(Ordering::SeqCst); persist(job)?;
        runtime.current = None; runtime.last_error = None;
    }
    dispatch_next(&app); Ok(snapshot())
}
#[tauri::command]
pub async fn voice_correct_result(app:AppHandle,expected:String,text:String)->Result<VoiceState,String>{
 if text.len()>2_000_000{return Err("Texte corrigé trop volumineux".into())}
 {let _delivery=DELIVERY.lock().map_err(|_|"Restitution verrouillée")?;
  let mut runtime=RUNTIME.lock().map_err(|_|"État vocal indisponible")?;
  let job=runtime.jobs.iter_mut().find(|j|j.blocked()&&!j.text.is_empty()).ok_or("Aucun résultat à corriger")?;
  if job.text!=expected{return Err("Le résultat a changé. Relance la vérification.".into())}
  let mut candidate=job.clone();candidate.text=text;candidate.status="result".into();persist(&candidate)?;*job=candidate;
 }
 publish(&app);Ok(snapshot())
}

#[tauri::command]
pub async fn voice_resolve(app: AppHandle, action: String) -> Result<VoiceState, String> {
    let id = { let runtime = RUNTIME.lock().unwrap(); runtime.jobs.iter().find(|j| j.blocked()).map(|j| j.id.clone()).ok_or("Aucun résultat en attente")? };
    tauri::async_runtime::spawn_blocking({ let app = app.clone(); move || deliver(&app, &id, &action) }).await.map_err(|e| e.to_string())??;
    Ok(snapshot())
}
#[tauri::command]
pub async fn voice_preload(app: AppHandle) -> Result<VoiceState, String> {
    let settings = workspace::settings()?;
    let worker = ensure_worker(&app, &settings.python)?;
    worker.send(json!({"op":"preload","id":format!("preload-{}", uuid::Uuid::new_v4()),"model":settings.model}))?;
    RUNTIME.lock().unwrap().state.engine = "loading".into(); publish(&app); Ok(snapshot())
}

fn shortcut_mode(configured:&str,key:&str)->String {
    if !key.contains('+') {"handsfree".into()} else {configured.into()}
}
pub fn shortcut(app: &AppHandle, pressed: bool) {
    if pressed {
        if KEY_DOWN.swap(true, Ordering::SeqCst) { return; }
    } else { KEY_DOWN.store(false, Ordering::SeqCst); }
    let mode = workspace::settings().map(|s| shortcut_mode(&s.voice_mode,&s.shortcut_dictation)).unwrap_or_else(|_| "handsfree".into());
    let active = { let runtime = RUNTIME.lock().unwrap(); runtime.active.is_some() || runtime.arming };
    let should_stop = (mode == "hold" && !pressed) || (mode != "hold" && pressed && active);
    if should_stop { START_SEQUENCE.fetch_add(1, Ordering::SeqCst); STOP_REQUESTED.store(true, Ordering::SeqCst); }
    let app = app.clone();
    if should_stop {
        std::thread::spawn(move || { if let Err(error) = stop_impl(&app, false) { fail(&app, error); } });
    } else if pressed {
        let request = begin_start();
        std::thread::spawn(move || {
            if let Err(error) = start_impl(&app, "dictation".into(), request, Some(mode.clone())) { fail(&app, error); }
            if mode == "hold" && !KEY_DOWN.load(Ordering::SeqCst) { let _ = stop_impl(&app, false); }
        });
    }
}

pub fn init(app: &AppHandle) {
    if APP.set(app.clone()).is_err() { return; }
    SHUTDOWN.store(false, Ordering::SeqCst);
    preview_sweep_abandoned();
    if let Ok(settings) = workspace::settings() { RUNTIME.lock().unwrap().state.mode = settings.voice_mode; }
    // Recovered results are never automatically inserted into an application.
    // Even 'delivering' may mean insertion happened immediately before a crash.
    if let Ok(entries) = fs::read_dir(job_dir()) {
        let mut recovered: Vec<Job> = entries.flatten().filter_map(|entry| {
            if entry.path().extension().is_none_or(|e| e != "json") { return None; }
            let bytes = fs::read(entry.path()).ok()?;
            if bytes.len() > 2_000_000 { return None; }
            serde_json::from_slice(&bytes).ok()
        }).collect();
        recovered.sort_by(|a, b| a.id.cmp(&b.id));
        for mut job in recovered {
            if uuid::Uuid::parse_str(&job.id).is_err() { continue; }
            if matches!(job.status.as_str(), "done" | "cancelled" | "discarded") { continue; }
            job=restore_job(job,EPOCH.load(Ordering::SeqCst));
            RUNTIME.lock().unwrap().jobs.push_back(job);
        }
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        while !SHUTDOWN.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(200));
            // CTranslate2 cannot always interrupt a native decode immediately.
            // After cancellation, discard a stuck process rather than leaving
            // the next recording behind that decode indefinitely.
            let stuck_cancel = CANCEL_WAIT.lock().unwrap().values().any(|since| since.elapsed() > Duration::from_secs(10));
            if stuck_cancel {
                CANCEL_WAIT.lock().unwrap().clear();
                let has_live_stream = RUNTIME.lock().unwrap().active.as_ref().is_some_and(|active| active.stream.is_some());
                if has_live_stream { let _ = stop_impl(&handle, false); }
                { let mut slot = WORKER.lock().unwrap();
                    WORKER_ID.fetch_add(1, Ordering::SeqCst);
                    if let Some(worker) = slot.take() { worker.kill(); }
                }
                { let mut runtime = RUNTIME.lock().unwrap();
                    runtime.state.engine = "cold".into();
                    for job in runtime.jobs.iter_mut().filter(|job| job.status == "transcribing") { job.status = "queued".into(); let _ = persist(job); }
                    runtime.current = None;
                    runtime.presentation.cancel();
                    runtime.state.message = "Moteur libéré après annulation".into();
                }
                dispatch_next(&handle); publish(&handle);
            }
            let stop = { let runtime = RUNTIME.lock().unwrap(); runtime.active.as_ref().map(|active| {
                let cap = capture_limit_secs(&active.spec.kind);
                active.started.elapsed().as_secs() >= cap || active.stream.as_ref().is_some_and(|stream| stream.finished()) || (active.stream.is_none() && !capture::is_running())
            }).unwrap_or(false) };
            if stop { if let Err(error) = stop_impl(&handle, false) { fail(&handle, error); } }
        }
    });
}
pub fn shutdown() {
    crate::dictation_target::cancel_pending();
    START_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    SHUTDOWN.store(true, Ordering::SeqCst); EPOCH.fetch_add(1, Ordering::SeqCst); STOP_REQUESTED.store(true, Ordering::SeqCst);
    if let Some(app) = APP.get() { let _ = cancel_impl(app); }
    if let Some(worker) = WORKER.lock().unwrap().take() { let _ = worker.send(json!({"op":"shutdown"})); worker.kill(); }
    preview_cleanup_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_preserves_completed_text_queued_audio_and_original_target() {
        let directory=std::env::temp_dir().join(format!("echo-recovery-{}",uuid::Uuid::new_v4()));
        let mut result=sample_job(); result.status="result".into();
        let mut queued=sample_job(); queued.id="queued-take".into();queued.text.clear();
        let jobs=VecDeque::from([result,queued]);
        let path=save_recovery(&directory,&jobs,&VoiceState::default()).unwrap();
        let saved:Value=serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["jobs"][0]["text"],"Écho, été, œuf.");
        assert_eq!(saved["jobs"][1]["audio_path"],"D:/original/.echo/audio/prise.wav");
        assert_eq!(saved["jobs"][0]["spec"]["fallback"]["path"],"premier.md");
        assert!(fs::read_to_string(path.with_extension("txt")).unwrap().contains("Écho, été, œuf."));
        let blocker=directory.join("not-a-directory");fs::write(&blocker,b"keep").unwrap();
        assert!(save_recovery(&blocker,&jobs,&VoiceState::default()).is_err());
        assert_eq!(jobs.len(),2);assert_eq!(jobs[0].status,"result");
    }
    #[test]fn single_key_dictation_toggles_even_with_old_hold_preference(){assert_eq!(shortcut_mode("hold","1"),"handsfree");assert_eq!(shortcut_mode("hold","a"),"handsfree");assert_eq!(shortcut_mode("hold","Ctrl+Alt+Space"),"hold");assert_eq!(shortcut_mode("handsfree","Ctrl+Alt+Space"),"handsfree");}
    fn sample_job() -> Job {
        let note = FrozenNote { workspace: "D:/original".into(), path: "premier.md".into() };
        Job { id: "operation-stable".into(), spec: SessionSpec { id: "session".into(), mode: "hold".into(), kind: "dictation".into(),
            workspace: note.workspace.clone(), target: Target::Note { note: note.clone() }, fallback: note,
            target_label: "premier.md".into(), python: "python".into(), model: "small".into(), language: "fr".into(), keep_audio: false, epoch: 7, hotwords: String::new() },
            audio_path: "D:/original/.echo/audio/prise.wav".into(), duration_secs: 2.0, status: "queued".into(), text: "Écho, été, œuf.".into(), error: None, provisional: false }
    }
    #[test]
    fn preview_buffers_audio_while_final_work_takes_priority(){
        let mut preview=preview_sample();let now=preview.last_sent;

        assert!(!preview_ready(&mut preview,&vec![100;32000],now+Duration::from_secs(2),true));
        assert_eq!(preview.samples,32000);assert_eq!(preview.audio.len(),32000);
        assert!(preview_ready(&mut preview,&[200;3200],now+Duration::from_secs(3),false));
    }
    fn preview_sample()->PreviewCapture{
        PreviewCapture{spec:sample_job().spec,generation:1,audio:VecDeque::new(),samples:0,last_sent:Instant::now(),inflight:None,prefix:String::new(),prefix_end:0.0,words:vec![]}
    }
    #[test]
    fn retained_result_or_error_does_not_hide_an_open_microphone() {
        for text in ["Texte à reprendre", ""] {
            let mut runtime=Runtime::default();
            let mut job=sample_job();job.status=if text.is_empty(){"error"}else{"result"}.into();job.text=text.into();
            runtime.active=Some(Active{spec:job.spec.clone(),started:Instant::now(),stream:None});
            runtime.jobs.push_back(job);refresh(&mut runtime);
            assert!(runtime.state.microphone_open,"Stop must remain available with a live stream");
            assert_eq!(runtime.state.phase,if text.is_empty(){"error"}else{"result"});
            runtime.active=None;refresh(&mut runtime);
            assert!(!runtime.state.microphone_open,"a retained result alone does not open the microphone");
        }
    }
    #[test]
    fn microphone_flag_includes_arming_and_clears_after_it_is_cancelled() {
        let mut runtime=Runtime::default();runtime.arming=true;refresh(&mut runtime);
        assert!(runtime.state.microphone_open);assert_eq!(runtime.state.phase,"arming");
        runtime.arming=false;refresh(&mut runtime);
        assert!(!runtime.state.microphone_open);assert_eq!(runtime.state.phase,"idle");
    }
    #[test]
    fn pcm_preview_is_ready_before_stop_and_only_one_decode_can_be_in_flight(){
        let mut preview=preview_sample();let now=preview.last_sent;

        assert!(!preview_ready(&mut preview,&vec![100;8000],now+Duration::from_millis(500),false));
        assert!(preview_ready(&mut preview,&vec![200;24000],now+Duration::from_secs(2),false));
        preview.inflight=Some("preview-test".into());
        assert!(!preview_ready(&mut preview,&vec![300;400000],now+Duration::from_secs(20),false));
        assert_eq!(preview.audio.len(),PREVIEW_SAMPLES);
        preview.inflight=None;
        assert!(preview_ready(&mut preview,&[400;3200],now+Duration::from_secs(21),false));
        assert_eq!(preview.audio.back(),Some(&400));
        assert_eq!(preview.spec.fallback.path,"premier.md");
    }
    #[test]
    fn rolling_preview_keeps_the_beginning_and_intentional_repeated_words(){
        let mut preview=preview_sample();
        let first=json!({"words":[{"word":"Bonjour","start":0.0,"end":0.4},{"word":" très","start":1.0,"end":1.2},{"word":" très","start":1.3,"end":1.5},{"word":" important","start":2.0,"end":2.5}]});
        assert_eq!(combine_preview(&mut preview,0.0,&first),"Bonjour très très important");
        let next=json!({"words":[{"word":" important","start":0.0,"end":0.5},{"word":" aujourd’hui.","start":1.0,"end":1.7}]});
        assert_eq!(combine_preview(&mut preview,2.0,&next),"Bonjour très très important aujourd’hui.");
    }
    #[test]
    fn previous_session_epoch_and_old_preview_revision_are_rejected(){
        let mut preview=preview_sample();preview.inflight=Some("preview-current".into());
        let job=PreviewJob{session:preview.spec.id.clone(),epoch:7,generation:1,speech_phrase:None,start:0.0,path:PathBuf::new()};
        assert!(preview_matches(&preview,&job,"preview-current",7));
        assert!(!preview_matches(&preview,&job,"preview-old",7));
        assert!(!preview_matches(&preview,&job,"preview-current",8));
        let old_phrase=PreviewJob{generation:0,..job.clone()};
        assert!(!preview_matches(&preview,&old_phrase,"preview-current",7));
        let wrong=PreviewJob{session:"new-session".into(),..job};
        assert!(!preview_matches(&preview,&wrong,"preview-current",7));
    }
    fn sample_inline_context()->InlineContext{
        let mut spec=sample_job().spec;spec.kind="dictation".into();spec.mode="toggle".into();
        spec.target=Target::Application{handle:"frozen-field-token".into()};
        InlineContext{ticket:InlineTicket{session:spec.id.clone(),epoch:7,generation:1,phrase:"phrase".into(),text:"Aperçu provisoire".into()},
            spec,audio_path:"D:/original/.echo/audio/phrase.wav".into()}
    }
    #[test]
    fn late_or_duplicate_utterance_never_overwrites_retained_recovery_journal(){
        let directory=std::env::temp_dir().join(format!("echo-inline-journal-{}",uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();let path=directory.join("phrase.json");
        let context=sample_inline_context();let kept=inline_recovery_job(&context,None,"Vérification",8);
        let saved=serde_json::to_vec(&kept).unwrap();fs::write(&path,&saved).unwrap();
        for (epoch,known) in [(7,true),(8,true),(7,false)]{
            let mut delayed=sample_job();delayed.id="phrase".into();delayed.text.clear();delayed.spec.epoch=epoch;
            let result=prepare_queued_job(delayed,8,known,|job|{
                fs::write(&path,serde_json::to_vec(job).unwrap()).map_err(|e|e.to_string())
            });
            assert!(result.is_none());assert_eq!(fs::read(&path).unwrap(),saved);
        }
        fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn cancellation_recovery_preserves_draft_identity_target_and_audio_without_calling_it_final(){
        let context=sample_inline_context();
        let kept=inline_recovery_job(&context,None,"Garde native",8);
        assert_eq!(kept.id,"phrase");assert_eq!(kept.text,"Aperçu provisoire");
        assert_eq!(kept.spec.epoch,8);assert_eq!(kept.status,"result");
        assert_eq!(kept.audio_path,context.audio_path);assert!(kept.provisional);assert!(kept.error.unwrap().contains("Brouillon provisoire"));
        assert!(matches!(kept.spec.target,Target::Application{handle} if handle=="frozen-field-token"));
    }
    #[test]
    fn inline_recovery_never_replaces_an_available_small_final_with_base_preview(){
        let context=sample_inline_context();let mut final_job=sample_job();
        final_job.id="phrase".into();final_job.text="Transcription finale précise.".into();
        let kept=inline_recovery_job(&context,Some(&final_job),"Garde native",8);
        assert_eq!(kept.text,final_job.text);assert_eq!(kept.audio_path,final_job.audio_path);
        assert!(!kept.provisional);assert!(kept.error.unwrap().starts_with("Texte final conservé."));
    }
    #[test]
    fn repeated_recovery_and_restart_never_promote_base_draft_to_final(){
        let context=sample_inline_context();
        let first=inline_recovery_job(&context,None,"Champ modifié",8);
        let second=inline_recovery_job(&context,Some(&first),"Nouvel essai",9);
        assert!(second.provisional);assert_eq!(second.text,"Aperçu provisoire");
        assert!(second.error.as_ref().unwrap().starts_with("Brouillon provisoire"));
        let encoded=serde_json::to_vec(&second).unwrap();
        let restored=restore_job(serde_json::from_slice(&encoded).unwrap(),10);
        assert!(restored.provisional);assert_eq!(restored.spec.epoch,10);
        assert!(restored.error.as_ref().unwrap().starts_with("Brouillon provisoire retrouvé"));
    }
    #[test]
    fn older_journals_default_to_final_text_and_keep_recovery_behavior(){
        let mut value=serde_json::to_value(sample_job()).unwrap();
        value.as_object_mut().unwrap().remove("provisional");
        let restored=restore_job(serde_json::from_value(value).unwrap(),8);
        assert!(!restored.provisional);assert_eq!(restored.text,"Écho, été, œuf.");
        assert_eq!(restored.status,"result");assert!(restored.error.unwrap().starts_with("Texte retrouvé."));
    }
    #[test]
    fn provisional_native_write_does_not_increment_finalized_phrase_count(){
        let mut runtime=Runtime::default();runtime.presentation.begin_session("session");
        runtime.presentation.sent("session","auto","Phrase finalisée.");
        runtime.inline.begin_session("session",7);runtime.inline.next_phrase("session",7,1);
        runtime.inline.confirm_speech("session",7,"phrase");
        let ticket=runtime.inline.propose("session",7,1,"Aperçu",false,Some("phrase")).unwrap();
        runtime.inline.applied(ticket);refresh(&mut runtime);
        assert!(runtime.state.inline_preview_supported);assert!(runtime.state.inline_preview_active);
        assert_eq!(runtime.state.sent_count,1);assert_eq!(runtime.state.last_sent_text,"Phrase finalisée.");
        runtime.inline.end_capture();runtime.presentation.cancel();refresh(&mut runtime);
        assert_eq!(runtime.state.sent_count,1);assert!(runtime.state.inline_preview_active);
        runtime.inline.resolved("session","phrase");refresh(&mut runtime);
        assert!(!runtime.state.inline_preview_active);assert_eq!(runtime.state.sent_count,1);
    }
    #[test]
    fn old_voice_journal_remains_readable_without_new_hotwords_field(){
        let mut value=serde_json::to_value(sample_job()).unwrap();value["spec"].as_object_mut().unwrap().remove("hotwords");
        let restored:Job=serde_json::from_value(value).unwrap();
        assert!(restored.spec.hotwords.is_empty());assert_eq!(restored.spec.fallback.path,"premier.md");
    }

    #[test]
    fn old_voice_state_remains_readable_without_presentation_fields(){
        let mut value=serde_json::to_value(VoiceState::default()).unwrap();
        for key in ["finalizing_text","finalizing_id","last_sent_text","sent_count","inline_preview_supported","inline_preview_active"] {
            value.as_object_mut().unwrap().remove(key);
        }
        let restored:VoiceState=serde_json::from_value(value).unwrap();
        assert!(restored.finalizing_text.is_empty());assert!(restored.finalizing_id.is_none());
        assert!(restored.last_sent_text.is_empty());assert_eq!(restored.sent_count,0);
        assert!(!restored.inline_preview_supported);assert!(!restored.inline_preview_active);
    }

    #[test]
    fn runtime_snapshot_keeps_preview_finalization_and_sent_text_separate(){
        let mut runtime=Runtime::default();
        runtime.presentation.begin_session("session");
        runtime.presentation.preview("session","Nouvelle phrase".into());
        runtime.presentation.begin_final("previous");
        runtime.presentation.final_progress("previous","Phrase précédente.");
        runtime.presentation.sent("session","auto","Déjà transmise.");
        refresh(&mut runtime);
        assert_eq!(runtime.state.partial,"Nouvelle phrase");
        assert_eq!(runtime.state.finalizing_text,"Phrase précédente.");
        assert_eq!(runtime.state.finalizing_id.as_deref(),Some("previous"));
        assert_eq!(runtime.state.last_sent_text,"Déjà transmise.");
        assert_eq!(runtime.state.sent_count,1);
    }

    #[test]
    fn journal_roundtrip_keeps_frozen_destination_and_accents() {
        let job = sample_job(); let encoded = serde_json::to_vec(&job).unwrap();
        let restored: Job = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(restored.spec.fallback.path, "premier.md");
        assert_eq!(restored.spec.fallback.workspace, "D:/original");
        assert_eq!(restored.text, "Écho, été, œuf.");
        assert_eq!(restored.id, "operation-stable");
    }
    #[test]
    fn unresolved_result_precedes_background_listening() {
        let mut runtime = Runtime::default(); let mut job = sample_job(); job.status = "result".into();
        runtime.active = Some(Active { spec: job.spec.clone(), started: Instant::now(), stream: None });
        runtime.jobs.push_back(job); refresh(&mut runtime);
        assert_eq!(runtime.state.phase, "result"); assert_eq!(runtime.state.text, "Écho, été, œuf.");
    }
    #[test]
    fn queued_result_is_not_presented_as_delivered() {
        let mut runtime = Runtime::default(); let mut job = sample_job(); job.status = "transcribing".into();
        runtime.current = Some(job.id.clone()); runtime.jobs.push_back(job); refresh(&mut runtime);
        assert_eq!(runtime.state.phase, "transcribing"); assert!(runtime.state.text.is_empty());
    }
    #[test]
    fn recording_limit_is_distinct_from_dictation_kind() {
        let mut runtime = Runtime::default(); let mut spec = sample_job().spec; spec.kind = "recording".into(); spec.mode = "toggle".into();
        runtime.active = Some(Active { spec, started: Instant::now(), stream: None }); refresh(&mut runtime);
        assert_eq!(runtime.state.phase, "recording"); assert_eq!(runtime.state.kind, "recording");
    }
}
