# Prototype only. One JSON request on stdin -> one JSON reply. No screen capture, files, network or clipboard.
$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
[Console]::InputEncoding=[Text.UTF8Encoding]::new($false)
$taskClock=[Diagnostics.Stopwatch]::StartNew()
$taskReply=@{ok=$false;engine='Windows.Media.Ocr';image_saved=$false;request_id=$null}
$taskStream=$null;$taskWriter=$null;$taskBitmap=$null
$taskErrorCode='INVALID_REQUEST'
try {
 $taskBuilder=[Text.StringBuilder]::new()
 $taskChars=New-Object char[] 4096
 while(($taskCount=[Console]::In.Read($taskChars,0,$taskChars.Length)) -gt 0){
  if($taskBuilder.Length+$taskCount -gt 8*1024*1024){throw 'Requête OCR trop volumineuse.'}
  [void]$taskBuilder.Append($taskChars,0,$taskCount)
 }
 $taskRequest=$taskBuilder.ToString().TrimStart([char]0xfeff)|ConvertFrom-Json
 if($taskRequest -isnot [Management.Automation.PSCustomObject] -or $taskRequest.op -cnotin @('ocr','catalog')){throw 'Opération OCR attendue.'}
 $taskReply.request_id=$taskRequest.request_id
 $taskLanguage=[string]$taskRequest.language
 if(-not $taskLanguage){$taskLanguage='fr-FR'}
 Add-Type -AssemblyName System.Runtime.WindowsRuntime
 [Windows.Media.Ocr.OcrEngine,Windows.Foundation,ContentType=WindowsRuntime]|Out-Null
 [Windows.Globalization.Language,Windows.Globalization,ContentType=WindowsRuntime]|Out-Null
 [Windows.Graphics.Imaging.BitmapDecoder,Windows.Foundation,ContentType=WindowsRuntime]|Out-Null
 [Windows.Graphics.Imaging.SoftwareBitmap,Windows.Foundation,ContentType=WindowsRuntime]|Out-Null
 [Windows.Storage.Streams.InMemoryRandomAccessStream,Windows.Foundation,ContentType=WindowsRuntime]|Out-Null
 [Windows.Storage.Streams.DataWriter,Windows.Foundation,ContentType=WindowsRuntime]|Out-Null
 $taskReply.available_languages=@([Windows.Media.Ocr.OcrEngine]::AvailableRecognizerLanguages|ForEach-Object {$_.LanguageTag})
 if($taskRequest.op -ceq 'catalog'){
  $taskReply.ok=$true
  $taskReply.available=($taskReply.available_languages.Count -gt 0)
  $taskReply.max_image_dimension=[Windows.Media.Ocr.OcrEngine]::MaxImageDimension
  $taskReply.elapsed_ms=[math]::Round($taskClock.Elapsed.TotalMilliseconds,2)
  $taskReply|ConvertTo-Json -Depth 8 -Compress
  exit 0
 }
 if($taskRequest.image_base64 -isnot [string] -or $taskRequest.image_base64.Length -gt 8*1024*1024){throw 'Image base64 manquante ou trop volumineuse.'}
 $taskErrorCode='OCR_LANGUAGE_UNAVAILABLE'
 $taskLang=[Windows.Globalization.Language]::new($taskLanguage)
 $taskReply.requested_language=$taskLanguage
 $taskReply.warnings=@()
 if(-not [Windows.Media.Ocr.OcrEngine]::IsLanguageSupported($taskLang)){
  $taskPrimary=$taskLang.LanguageTag.Split('-')[0]
  if($taskPrimary -cin @('fr','en','nl','de','es','it') -and $taskReply.available_languages -contains 'fr-FR'){
   $taskReply.warnings=@('OCR français utilisé faute de modèle Windows '+$taskLanguage+'. Les accents peuvent être imparfaits.')
   $taskLang=[Windows.Globalization.Language]::new('fr-FR')
  } else {throw ('Reconnaissance OCR Windows indisponible pour '+$taskLanguage+'.')}
 }
 $taskEngine=[Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage($taskLang)
 if($null -eq $taskEngine){throw 'Moteur OCR Windows non disponible.'}
 $taskErrorCode='INVALID_IMAGE'
 try {$taskBytes=[Convert]::FromBase64String($taskRequest.image_base64)} catch {throw 'Image base64 invalide.'}
 if($taskBytes.Length -eq 0 -or $taskBytes.Length -gt 6*1024*1024){throw 'Taille du fichier image refusée.'}
 $script:taskAsTask=@([System.WindowsRuntimeSystemExtensions].GetMethods()|Where-Object {
  $_.Name -eq 'AsTask' -and $_.IsGenericMethodDefinition -and $_.GetGenericArguments().Count -eq 1 -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq ('IAsyncOperation' + [char]96 + '1')
 })[0]
 function Await-PrototypeOperation($Operation,[Type]$ResultType){
  $taskAsync=$script:taskAsTask.MakeGenericMethod($ResultType).Invoke($null,@($Operation))
  if(-not $taskAsync.Wait(5000)){throw 'Délai OCR dépassé.'}
  return $taskAsync.GetAwaiter().GetResult()
 }
 $taskStream=[Windows.Storage.Streams.InMemoryRandomAccessStream]::new()
 $taskWriter=[Windows.Storage.Streams.DataWriter]::new($taskStream)
 $taskWriter.WriteBytes($taskBytes)
 [void](Await-PrototypeOperation ($taskWriter.StoreAsync()) ([uint32]))
 $taskStream.Seek(0)
 $taskDecoder=Await-PrototypeOperation ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($taskStream)) ([Windows.Graphics.Imaging.BitmapDecoder])
 if($taskDecoder.PixelWidth -gt 4096 -or $taskDecoder.PixelHeight -gt 4096 -or [int64]$taskDecoder.PixelWidth*$taskDecoder.PixelHeight -gt 4000000){throw 'Zone OCR trop grande (4 millions de pixels maximum).'}
 $taskBitmap=Await-PrototypeOperation ($taskDecoder.GetSoftwareBitmapAsync([Windows.Graphics.Imaging.BitmapPixelFormat]::Bgra8,[Windows.Graphics.Imaging.BitmapAlphaMode]::Premultiplied)) ([Windows.Graphics.Imaging.SoftwareBitmap])
 $taskErrorCode='OCR_FAILED'
 $taskRecognize=[Diagnostics.Stopwatch]::StartNew()
 $taskResult=Await-PrototypeOperation ($taskEngine.RecognizeAsync($taskBitmap)) ([Windows.Media.Ocr.OcrResult])
 $taskRecognize.Stop()
 $taskReply.ok=$true
 $taskReply.language=$taskEngine.RecognizerLanguage.LanguageTag
 $taskReply.text=(@($taskResult.Lines|ForEach-Object {$_.Text}) -join [char]10)
 $taskReply.lines=@($taskResult.Lines|ForEach-Object {@{text=$_.Text;words=@($_.Words|ForEach-Object {@{text=$_.Text;x=$_.BoundingRect.X;y=$_.BoundingRect.Y;width=$_.BoundingRect.Width;height=$_.BoundingRect.Height}})}})
 $taskReply.width=[int]$taskBitmap.PixelWidth;$taskReply.height=[int]$taskBitmap.PixelHeight
 $taskReply.recognize_ms=[math]::Round($taskRecognize.Elapsed.TotalMilliseconds,2)
} catch {
 $taskReply.error_code=$taskErrorCode
 $taskReply.error=$_.Exception.Message
} finally {
 if($taskBitmap){$taskBitmap.Dispose()}
 if($taskWriter){[void]$taskWriter.DetachStream();$taskWriter.Dispose()}
 if($taskStream){$taskStream.Dispose()}
}
$taskClock.Stop();$taskReply.elapsed_ms=[math]::Round($taskClock.Elapsed.TotalMilliseconds,2)
$taskReply|ConvertTo-Json -Depth 8 -Compress
if(-not $taskReply.ok){exit 1}