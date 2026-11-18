#!/usr/bin/env python3
"""Private local translation bridge. NDJSON stdin/stdout; no web API, screen access or content logging.
Prototype asset root is this folder; production uses sibling vendor/translation.
"""
from __future__ import annotations
import argparse,base64,gc,json,os,queue,re,subprocess,sys,threading,time
from pathlib import Path
sys.dont_write_bytecode=True
LANGUAGES=(("fr","Français"),("en","English"),("nl","Nederlands"),("de","Deutsch"),("es","Español"),("it","Italiano"))
CODES={code for code,_ in LANGUAGES}
MAX_TEXT=10000
MAX_REQUEST=9*1024*1024
MAX_PIECES=128
HERE=Path(__file__).resolve().parent
ASSETS=HERE/"vendor"/"translation" if (HERE/"vendor"/"translation"/"download-manifest.json").is_file() else HERE
class BridgeError(Exception):
 def __init__(self,code,message):self.code=code;super().__init__(message)

def bounded_root(base:Path,relative:str)->Path:
 if not isinstance(relative,str) or not relative or Path(relative).is_absolute():raise ValueError("Invalid model asset path")
 full=(base/relative).resolve()
 if not full.is_relative_to(base.resolve()):raise ValueError("Model asset path escapes private bundle")
 return full

class TranslationManager:
 def __init__(self,assets=ASSETS,translator_factory=None,tokenizer_factory=None):
  self.assets=Path(assets).resolve()
  self.translator_factory=translator_factory
  self.tokenizer_factory=tokenizer_factory
  self.translator=None;self.tokenizer=None;self.loaded=None
  path=self.assets/"download-manifest.json"
  manifest=json.loads(path.read_text(encoding="utf-8-sig")) if path.is_file() else {"models":[]}
  self.models={}
  for row in manifest.get("models",[]):
   source=row.get("from");target=row.get("to")
   if source not in CODES or target not in CODES or source==target:raise ValueError("Unexpected model language")
   if (source,target) in self.models:raise ValueError("Duplicate model direction")
   root=bounded_root(self.assets,row.get("root"))
   self.models[(source,target)]={**row,"path":root}
  self.ocr_catalog_cache=None

 def unload(self):
  old=self.translator
  self.translator=None;self.tokenizer=None;self.loaded=None
  if old is not None:
   try:old.unload_model(to_cpu=False)
   finally:
    del old
    gc.collect()

 def _load(self,pair):
  if self.loaded==pair:return
  self.unload() # release before constructing a different model, including a pivot
  row=self.models.get(pair)
  if not row or not (row["path"]/"model").is_dir() or not (row["path"]/"sentencepiece.model").is_file():
   raise BridgeError("MODEL_NOT_INSTALLED","Modèle local non installé pour cette direction.")
  if self.translator_factory is None or self.tokenizer_factory is None:
   private=self.assets/"vendor"
   if private.is_dir() and str(private) not in sys.path:sys.path.insert(0,str(private))
  if self.translator_factory is None:
   import ctranslate2
   translator=ctranslate2.Translator(str(row["path"]/"model"),device="cpu",compute_type="int8",inter_threads=1,intra_threads=2,max_queued_batches=1)
  else:translator=self.translator_factory(row["path"]/"model")
  try:
   if self.tokenizer_factory is None:
    import sentencepiece
    tokenizer=sentencepiece.SentencePieceProcessor(model_proto=(row["path"]/"sentencepiece.model").read_bytes())
   else:tokenizer=self.tokenizer_factory(row["path"]/"sentencepiece.model")
  except Exception:
   translator.unload_model(to_cpu=False)
   del translator
   gc.collect()
   raise
  self.translator=translator;self.tokenizer=tokenizer;self.loaded=pair

 def route(self,source,target):
  if source==target:return [source]
  if (source,target) in self.models:return [source,target]
  return [source,"en",target] if source!="en" and target!="en" else [source,target]

 def route_installed(self,route):
  return all((a,b) in self.models and (self.models[(a,b)]["path"]/"model").is_dir() and (self.models[(a,b)]["path"]/"sentencepiece.model").is_file() for a,b in zip(route,route[1:]))

 def _translate_line(self,line,warnings):
  # Argos models decode a sentence at a time. Feeding an entire paragraph can
  # emit EOS after its first sentence and silently lose the rest of the note.
  # Keep separators and translate every sentence, without another NLP model.
  abbreviations={"mr.","mrs.","ms.","dr.","prof.","st.","mme.","mlle.","m.","etc.","e.g.","i.e.","z.b.","bzw.","dhr.","mevr."}
  parts=[];start=0
  for boundary in re.finditer(r'[.!?…]+[»”\"’\)\]]*[ \t]+(?=\S)',line):
   before=line[start:boundary.start()+1]
   last=before.rsplit(None,1)[-1].casefold() if before.strip() else ""
   if last in abbreviations or re.fullmatch(r'[a-zà-ÿ]\.',last):continue
   gap=re.search(r'[ \t]+$',boundary.group()).group()
   end=boundary.end()-len(gap)
   parts.append(self._translate_sentence(line[start:end],warnings));parts.append(gap)
   start=boundary.end()
  parts.append(self._translate_sentence(line[start:],warnings))
  return "".join(parts)

 def _translate_sentence(self,line,warnings):
  if not line.strip():return line
  lead=line[:len(line)-len(line.lstrip())]
  tail=line[len(line.rstrip()):]
  pieces=self.tokenizer.encode(line.strip(),out_type=str)
  if len(pieces)>MAX_PIECES and "Texte long segmenté en passages courts." not in warnings:
   warnings.append("Texte long segmenté en passages courts.")
  result=[];start=0;count=0
  while start<len(pieces):
   end=min(start+MAX_PIECES,len(pieces))
   if end<len(pieces):
    boundaries=[i for i in range(start+MAX_PIECES//2,end) if pieces[i].startswith("▁")]
    if boundaries:end=boundaries[-1]
   batch=pieces[start:end];count+=1
   if count>256:raise BridgeError("TEXT_TOO_COMPLEX","Texte trop complexe : traduire un passage plus court.")
   maximum=min(512,max(64,len(batch)*3))
   translated=self.translator.translate_batch([batch],beam_size=2,num_hypotheses=1,replace_unknowns=True,length_penalty=0.2,max_input_length=0,max_decoding_length=maximum,max_batch_size=1)[0].hypotheses[0]
   if len(translated)>=maximum:raise BridgeError("OUTPUT_LIMIT","La traduction a atteint sa limite ; réduire le passage.")
   result.append(self.tokenizer.decode(translated).replace("▁"," ").strip());start=end
  return lead+" ".join(result)+tail

 def translate(self,text,source,target):
  if source not in CODES or target not in CODES:raise BridgeError("INVALID_LANGUAGE","Choisir manuellement une langue source et une destination du catalogue.")
  if not isinstance(text,str):raise BridgeError("INVALID_TEXT","Le texte doit être une chaîne Unicode.")
  if len(text)>MAX_TEXT:raise BridgeError("TEXT_TOO_LONG",f"Texte limité à {MAX_TEXT} caractères.")
  route=self.route(source,target);warnings=[]
  if source==target or not text.strip():
   return {"text":text,"from":source,"to":target,"route":[source] if source==target else route,"models":[],"warnings":[]}
  if not self.route_installed(route):raise BridgeError("MODEL_NOT_INSTALLED","Cette traduction nécessite un modèle local qui n’est pas installé.")
  value=text;models=[]
  for pair in zip(route,route[1:]):
   self._load(pair);models.append(self.models[pair]["id"])
   # Preserve all original paragraph separators; never truncate at CTranslate2's default input limit.
   value="".join(part if re.fullmatch(r"\r\n|\n|\r",part) else self._translate_line(part,warnings) for part in re.split(r"(\r\n|\n|\r)",value))
  if len(route)>2:warnings.append("Traduction avec étape intermédiaire en anglais.")
  numbers=lambda s:set(re.findall(r'\d+(?:[.,:]\d+)*',s))
  if numbers(text)!=numbers(value):warnings.append("Vérifie les nombres et les heures : le modèle en a reformulé certains.")
  return {"text":value,"from":source,"to":target,"route":route,"models":models,"warnings":warnings}

 def _ocr(self,request):
  operation=request.get("op","ocr")
  script=next((p for p in (HERE/"echo_ocr.ps1",HERE/"ocr_memory_prototype.ps1",self.assets/"echo_ocr.ps1") if p.is_file()),None)
  if script is None:raise BridgeError("OCR_UNAVAILABLE","Le composant OCR Windows local n’est pas installé.")
  image=request.get("image_base64")
  if operation=="ocr":
   if not isinstance(image,str) or len(image)>8*1024*1024:raise BridgeError("INVALID_IMAGE","Image PNG base64 manquante ou trop volumineuse.")
   try:decoded=base64.b64decode(image,validate=True)
   except Exception:raise BridgeError("INVALID_IMAGE","Image PNG base64 invalide.")
   if len(decoded)>6*1024*1024 or not decoded.startswith(b"\x89PNG\r\n\x1a\n"):raise BridgeError("INVALID_IMAGE","Une image PNG de moins de 6 Mo est attendue.")
   del decoded
  flags=getattr(subprocess,"CREATE_NO_WINDOW",0)
  child=subprocess.run(["powershell.exe","-NoProfile","-NonInteractive","-ExecutionPolicy","Bypass","-File",str(script)],input=json.dumps(request,ensure_ascii=True).encode("ascii"),capture_output=True,timeout=12,creationflags=flags)
  try:reply=json.loads(child.stdout.decode("utf-8-sig"))
  except Exception:raise BridgeError("OCR_FAILED","Le composant OCR Windows n’a pas répondu correctement.")
  if not reply.get("ok"):raise BridgeError(reply.get("error_code","OCR_FAILED"),reply.get("error","Reconnaissance OCR impossible."))
  reply.pop("ok",None);reply.pop("request_id",None)
  return reply

 def catalog(self):
  # Metadata operation stays independent of CTranslate2/SentencePiece imports.
  if self.ocr_catalog_cache is None:
   try:self.ocr_catalog_cache=self._ocr({"op":"catalog"})
   except BridgeError as error:self.ocr_catalog_cache={"available":False,"available_languages":[],"error":str(error)}
  return {"languages":[{"code":code,"name":name} for code,name in LANGUAGES],
          "routes":[{"from":source,"to":target,"route":self.route(source,target),"installed":self.route_installed(self.route(source,target))} for source in sorted(CODES) for target in sorted(CODES)],
          "model_count":len(self.models),"loaded_model":self.models[self.loaded]["id"] if self.loaded else None,
          "threads":2,"max_loaded_models":1,"max_text":MAX_TEXT,"offline":True,"ocr":self.ocr_catalog_cache}

 def handle(self,request):
  started=time.perf_counter();request_id=request.get("id") if isinstance(request,dict) else None
  try:
   if not isinstance(request,dict):raise BridgeError("INVALID_REQUEST","Une requête JSON objet est attendue.")
   if isinstance(request_id,bool) or not isinstance(request_id,(str,int)) or (isinstance(request_id,str) and len(request_id)>128):raise BridgeError("INVALID_REQUEST","Identifiant de requête absent ou invalide.")
   operation=request.get("op")
   if operation=="translate":value=self.translate(request.get("text"),request.get("from"),request.get("to"))
   elif operation=="catalog":value=self.catalog()
   elif operation=="unload":self.unload();value={"unloaded":True}
   elif operation=="ocr":value=self._ocr(request)
   else:raise BridgeError("INVALID_REQUEST","Opération inconnue.")
   return {"id":request_id,"ok":True,**value,"elapsed_ms":round((time.perf_counter()-started)*1000,2)}
  except BridgeError as error:return {"id":request_id,"ok":False,"error_code":error.code,"error":str(error),"elapsed_ms":round((time.perf_counter()-started)*1000,2)}
  except subprocess.TimeoutExpired:return {"id":request_id,"ok":False,"error_code":"OCR_TIMEOUT","error":"Le délai de reconnaissance Windows est dépassé.","elapsed_ms":round((time.perf_counter()-started)*1000,2)}
  except Exception:return {"id":request_id,"ok":False,"error_code":"LOCAL_ENGINE_ERROR","error":"Le moteur local n’a pas pu terminer cette opération.","elapsed_ms":round((time.perf_counter()-started)*1000,2)}

def main():
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument("--server",action="store_true",help="One JSON object per input/output line, serial model execution")
 parser.add_argument("--json",action="store_true",help="One JSON object from stdin until EOF")
 args=parser.parse_args()
 if not(args.server or args.json):parser.error("--server or --json is required")
 if hasattr(sys.stdout,"reconfigure"):sys.stdout.reconfigure(encoding="utf-8",errors="strict",line_buffering=True,newline="\n")
 manager=TranslationManager()
 def respond(raw):
  try:
   if len(raw)>MAX_REQUEST:raise BridgeError("REQUEST_TOO_LARGE","Requête trop volumineuse.")
   request=json.loads(raw.decode("utf-8-sig"))
   reply=manager.handle(request)
  except BridgeError as error:reply={"id":None,"ok":False,"error_code":error.code,"error":str(error)}
  except (UnicodeError,json.JSONDecodeError):reply={"id":None,"ok":False,"error_code":"INVALID_JSON","error":"JSON UTF-8 invalide."}
  print(json.dumps(reply,ensure_ascii=False,separators=(",",":")),flush=True)
 try:
  if args.json:respond(sys.stdin.buffer.read(MAX_REQUEST+1))
  else:
   pending=queue.Queue(maxsize=1)
   def read_requests():
    while True:
     raw=sys.stdin.buffer.readline(MAX_REQUEST+1)
     if not raw:
      pending.put(None);return
     if len(raw)>MAX_REQUEST:
      while raw and not raw.endswith(b"\n"):raw=sys.stdin.buffer.readline(65536)
      raw=b" "*(MAX_REQUEST+1)
     pending.put(raw)
   threading.Thread(target=read_requests,name="local-translation-stdin",daemon=True).start()
   while True:
    try:raw=pending.get(timeout=15)
    except queue.Empty:
     manager.unload() # no translation model retained after fifteen idle seconds
     continue
    if raw is None:break
    respond(raw)
 finally:manager.unload()
if __name__=="__main__":main()
