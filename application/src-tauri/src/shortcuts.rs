//! Native pressed/released shortcuts. Register replacements before removing existing bindings.
use std::{str::FromStr,sync::{LazyLock,Mutex}};
use serde::Serialize;
use tauri::{AppHandle,Emitter};
use tauri_plugin_global_shortcut::{Code,GlobalShortcutExt,Modifiers,Shortcut,ShortcutState};
static REGISTERED:LazyLock<Mutex<Option<(Shortcut,Shortcut)>>>=LazyLock::new(||Mutex::new(None));
static ERROR:LazyLock<Mutex<Option<String>>>=LazyLock::new(||Mutex::new(None));
#[derive(Clone,Serialize)]pub struct Status{pub error:Option<String>}
pub fn analyser(text:&str)->Result<Shortcut,String>{
 parse(text,false)
}
pub fn analyser_dictee(text:&str)->Result<Shortcut,String>{parse(text,true)}
fn parse(text:&str,allow_single:bool)->Result<Shortcut,String>{
 let mut modifiers=Modifiers::empty();let mut code=None;
 for token in text.split('+').map(str::trim).filter(|s|!s.is_empty()){
  match token.to_ascii_lowercase().as_str(){
   "ctrl"|"control"|"cmdorctrl"=>modifiers|=Modifiers::CONTROL,"alt"=>modifiers|=Modifiers::ALT,"shift"=>modifiers|=Modifiers::SHIFT,"win"|"super"|"meta"=>modifiers|=Modifiers::SUPER,
   _=>{if code.is_some(){return Err("Un raccourci doit contenir une seule touche finale.".into())}let normalized=match token.to_ascii_lowercase().as_str(){"espace"|"space"=>"Space".into(),"escape"|"echap"=>"Escape".into(),"enter"|"entree"=>"Enter".into(),"tab"=>"Tab".into(),_ if token.len()==1&&token.chars().all(|c|c.is_ascii_alphabetic())=>format!("Key{}",token.to_uppercase()),_ if token.len()==1&&token.chars().all(|c|c.is_ascii_digit())=>format!("Digit{token}"),_ if token.starts_with(['f','F'])=>token.to_uppercase(),_=>token.to_string()};code=Some(Code::from_str(&normalized).map_err(|_|format!("Touche inconnue : {token}"))?);}
  }
 }
 if modifiers.is_empty()&&!allow_single{return Err("Ajoute Ctrl, Alt, Maj ou Win pour le raccourci de capture.".into())}
 Ok(Shortcut::new(Some(modifiers),code.ok_or("Il manque la touche finale du raccourci.")?))
}
fn bind(app:&AppHandle,shortcut:Shortcut,voice:bool)->Result<(),String>{
 app.global_shortcut().on_shortcut(shortcut,move|app,_,event|{
  if voice{if event.state==ShortcutState::Pressed{crate::ui::show_quick_quiet(app)}crate::voice_v21::shortcut(app,event.state==ShortcutState::Pressed)}
  else if event.state==ShortcutState::Pressed{let app=app.clone();tauri::async_runtime::spawn(async move{if let Err(e)=crate::capture_v21::capture_begin(app.clone()).await{let _=app.emit("app:notice",e);crate::ui::show_quick_quiet(&app);}});}
 }).map_err(|e|format!("Ce raccourci est déjà utilisé ou refusé par Windows : {e}"))
}
fn status(app:&AppHandle,error:Option<String>){if let Ok(mut s)=ERROR.lock(){*s=error.clone()}let _=app.emit("shortcuts:status",Status{error});}
pub fn configure(app:&AppHandle,dictation:&str,capture:&str)->Result<(),String>{
 let d=analyser_dictee(dictation)?;let c=analyser(capture)?;if d==c{return Err("La dictée et la capture doivent avoir des raccourcis différents.".into())}
 let mut registered=REGISTERED.lock().map_err(|_|"Raccourcis indisponibles")?;let old=*registered;
 if let Some((od,oc))=old{if (d==oc&&d!=od)||(c==od&&c!=oc){return Err("Choisis d’abord un raccourci libre avant de permuter les deux gestes.".into())}}
 let mut added=vec![];
 for (key,voice) in [(d,true),(c,false)]{
  let same=old.is_some_and(|(od,oc)|if voice{od==key}else{oc==key});if same{continue}
  if let Err(e)=bind(app,key,voice){for k in added{let _=app.global_shortcut().unregister(k);}status(app,Some(e.clone()));return Err(e)}added.push(key);
 }
 if let Some((od,oc))=old{for key in [od,oc]{if key!=d&&key!=c{let _=app.global_shortcut().unregister(key);}}}
 *registered=Some((d,c));status(app,None);Ok(())
}
pub fn init(app:&AppHandle){if let Ok(s)=crate::workspace::settings(){if let Err(e)=configure(app,&s.shortcut_dictation,&s.shortcut_capture){status(app,Some(format!("Raccourci indisponible. Ferme la V1 ou choisis une autre combinaison. {e}")));}}}
#[tauri::command]pub fn shortcuts_status()->Status{Status{error:ERROR.lock().ok().and_then(|e|e.clone())}}
#[cfg(test)]mod tests{use super::*;#[test]fn refuses_duplicate_final_keys(){assert!(analyser("Ctrl+A+B").is_err());assert!(analyser("A").is_err());assert_eq!(analyser("Ctrl+Alt+Espace").unwrap(),analyser("Ctrl+Alt+Space").unwrap());}}
#[cfg(test)]mod dictation_key_tests{use super::*;#[test]fn single_digit_letter_and_function_key_are_configurable_for_dictation(){assert_eq!(analyser_dictee("1").unwrap(),Shortcut::new(None,Code::Digit1));assert_eq!(analyser_dictee("a").unwrap(),Shortcut::new(None,Code::KeyA));assert_eq!(analyser_dictee("F8").unwrap(),Shortcut::new(None,Code::F8));assert!(analyser_dictee("").is_err());assert!(analyser_dictee("A+B").is_err());}}

#[tauri::command]pub async fn shortcuts_retry(app:AppHandle)->Result<Status,String>{crate::workspace::retry_shortcuts(app).await}
