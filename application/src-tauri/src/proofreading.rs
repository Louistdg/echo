//! Explicit local French proofreading. Never changes a note or a dictation.
use std::{io::{Read,Write},process::{Command,Stdio},time::{Duration,Instant}};
use serde_json::{json,Value};
#[tauri::command]
pub async fn check_spelling(text:String)->Result<Value,String>{
 if text.chars().count()>20_000{return Err("Le correcteur accepte 20 000 caractères maximum.".into())}
 let settings=crate::workspace::settings()?;
 tauri::async_runtime::spawn_blocking(move||{
  let script=crate::workspace::portable_root().join("moteur/echo_proofread.py");
  if !script.is_file(){return Err("Le correcteur local n’est pas installé dans cette version.".into())}
  let request_id=uuid::Uuid::new_v4().to_string();
  let body=serde_json::to_vec(&json!({"text":text,"personal_words":settings.personal_words,"request_id":request_id})).map_err(|e|e.to_string())?;
  let mut command=Command::new(settings.python);
  command.arg("-u").arg(script).arg("--json").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).env("PYTHONIOENCODING","utf-8");
  #[cfg(windows)]{use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
  let mut child=command.spawn().map_err(|e|format!("Le correcteur local ne démarre pas : {e}"))?;
  let output=child.stdout.take().ok_or("Sortie du correcteur absente")?;
  let reader=std::thread::spawn(move||{let mut bytes=Vec::new();output.take(2_000_001).read_to_end(&mut bytes).map(|_|bytes)});
  if let Some(mut input)=child.stdin.take(){if let Err(error)=input.write_all(&body){let _=child.kill();let _=child.wait();let _=reader.join();return Err(format!("Le correcteur est déconnecté : {error}"))}}
  let deadline=Instant::now()+Duration::from_secs(25);
  loop{match child.try_wait().map_err(|e|e.to_string())?{Some(_)=>break,None if Instant::now()>deadline=>{let _=child.kill();let _=child.wait();let _=reader.join();return Err("Le correcteur prend trop de temps. Réessaie sur un texte plus court.".into())},None=>std::thread::sleep(Duration::from_millis(25))}}
  let bytes=reader.join().map_err(|_|"Lecture du correcteur interrompue")?.map_err(|e|e.to_string())?;
  if bytes.len()>2_000_000{return Err("Réponse du correcteur trop volumineuse.".into())}
  let report:Value=serde_json::from_slice(&bytes).map_err(|_|"Réponse du correcteur illisible")?;
  if report["ok"].as_bool()!=Some(true){return Err(report["error"].as_str().unwrap_or("Le correcteur local est indisponible.").to_string())}
  if report["source"].as_str()!=Some(text.as_str())||report["request_id"].as_str()!=Some(request_id.as_str()){return Err("Le correcteur a répondu pour un autre texte.".into())}
  let utf16:Vec<u16>=text.encode_utf16().collect();
  if let Some(matches)=report["matches"].as_array(){for item in matches{
   let offset=item["offset"].as_u64().ok_or("Position de correction invalide")? as usize;
   let len=item["length"].as_u64().ok_or("Longueur de correction invalide")? as usize;
   let end=offset.checked_add(len).ok_or("Correction hors du texte")?;
   if end>utf16.len(){return Err("Correction hors du texte".into())}
   let original=String::from_utf16(&utf16[offset..end]).map_err(|_|"Position Unicode invalide")?;
   if item["original"].as_str()!=Some(original.as_str()){return Err("La correction ne correspond pas au texte original.".into())}
  }}
  Ok(report)
 }).await.map_err(|e|e.to_string())?
}
