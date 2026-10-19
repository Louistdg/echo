// Capture audio (§4.2 du plan).
//
// M2 : prouver l'enregistrement, rien d'autre. Aucune transcription, aucune VAD.
//
// Le périphérique est ouvert dans SON format natif (fréquence et canaux
// quelconques) ; la conversion vers WAV 16 kHz mono est faite en logiciel par
// `Converter` (moyenne des canaux puis interpolation linéaire). Le fichier
// temporaire vit dans `%TEMP%\Écho\` et est purgé au démarrage suivant.
//
// `capture:level` est émis à ~30 Hz par l'interface appelante (`start` reçoit un
// émetteur), ce qui garde ce module testable sans Tauri.

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use hound::{SampleFormat as WavSampleFormat, WavSpec, WavWriter};
use serde::Serialize;

/// Fréquence imposée au fichier produit (format natif de Whisper).
pub const TARGET_RATE: u32 = 16_000;

/// Période d'émission des niveaux (≈30 Hz).
const LEVEL_PERIOD: Duration = Duration::from_millis(33);

#[derive(Debug, Clone, Serialize)]
pub struct CaptureInfo {
    pub path: String,
    pub duration_secs: f64,
    pub samples: u64,
    pub sample_rate: u32,
    pub channels: u16,
}

type Writer = WavWriter<BufWriter<fs::File>>;

/// État partagé entre le fil de capture et les commandes.
static SESSION: LazyLock<Mutex<Option<Active>>> = LazyLock::new(|| Mutex::new(None));
static PAUSED: AtomicBool = AtomicBool::new(false);

struct Active {
    stop: Arc<AtomicBool>,
    join: std::thread::JoinHandle<Result<CaptureInfo, String>>,
}

/// Dossier des fichiers temporaires.
pub fn temp_dir() -> PathBuf {
    std::env::temp_dir().join("Écho")
}

/// Supprime tous les WAV temporaires d'un dossier. Retourne le nombre effacé.
/// Appelé au démarrage : une capture interrompue par un arrêt brutal ne doit
/// pas laisser de fichier derrière elle.
pub fn purge_dir(dir: &Path) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut n = 0;
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().map(|e| e == "wav").unwrap_or(false) && fs::remove_file(&p).is_ok() {
            n += 1;
        }
    }
    n
}

pub fn purge_temp() -> usize {
    purge_dir(&temp_dir())
}

/// Démarre une capture. `emit_level` reçoit le RMS (0..1) à ~30 Hz.
/// Retourne dès que le flux audio tourne réellement.
pub fn start(emit_level: impl Fn(f32) + Send + 'static) -> Result<(), String> {
    let mut slot = SESSION
        .lock()
        .map_err(|_| "état de capture corrompu".to_string())?;
    if slot.is_some() {
        return Err("une capture est déjà en cours".into());
    }

    let dir = temp_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("dossier temporaire inaccessible : {e}"))?;
    let path = dir.join(format!(
        "capture-{}.wav",
        chrono::Local::now().format("%Y%m%d-%H%M%S%.3f")
    ));

    PAUSED.store(false, Ordering::Relaxed);
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

    let stop_thread = stop.clone();
    let join = std::thread::spawn(move || run(path, stop_thread, emit_level, ready_tx));

    match ready_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => {
            *slot = Some(Active { stop, join });
            Ok(())
        }
        Ok(Err(e)) => {
            let _ = join.join();
            Err(e)
        }
        Err(_) => {
            stop.store(true, Ordering::Relaxed);
            let _ = join.join();
            Err("le micro n'a pas répondu en 5 s".into())
        }
    }
}

/// Arrête la capture, finalise le WAV et retourne son emplacement.
pub fn stop() -> Result<CaptureInfo, String> {
    let active = SESSION
        .lock()
        .map_err(|_| "état de capture corrompu".to_string())?
        .take()
        .ok_or("aucune capture en cours")?;
    active.stop.store(true, Ordering::Relaxed);
    active
        .join
        .join()
        .map_err(|_| "le fil de capture s'est arrêté anormalement".to_string())?
}

pub fn pause() -> Result<(), String> {
    if !is_running() {
        return Err("aucune capture en cours".into());
    }
    PAUSED.store(true, Ordering::Relaxed);
    Ok(())
}

pub fn resume() -> Result<(), String> {
    if !is_running() {
        return Err("aucune capture en cours".into());
    }
    PAUSED.store(false, Ordering::Relaxed);
    Ok(())
}

pub fn is_running() -> bool {
    let Ok(mut slot) = SESSION.lock() else {
        return false;
    };
    if let Some(active) = slot.as_ref() {
        // Le fil peut s'être arrêté sur une erreur de périphérique sans que
        // `stop` ait été appelé : la session ne doit pas rester « en cours ».
        if active.join.is_finished() {
            slot.take();
            return false;
        }
        return true;
    }
    false
}

/// Corps du fil de capture. Le flux cpal reste dans ce fil (il n'est pas
/// garanti `Send`) ; on le maintient en vie jusqu'au drapeau d'arrêt.
fn run(
    path: PathBuf,
    stop: Arc<AtomicBool>,
    emit_level: impl Fn(f32) + Send + 'static,
    ready: mpsc::Sender<Result<(), String>>,
) -> Result<CaptureInfo, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or("aucun micro détecté (aucun périphérique d'entrée par défaut)")?;

    let config = device
        .default_input_config()
        .map_err(|e| format!("micro indisponible : configuration impossible ({e})"))?;

    let in_rate = config.sample_rate();
    let channels = config.channels() as usize;
    let converter = Arc::new(Mutex::new(Converter::new(channels, in_rate)));

    let spec = WavSpec {
        channels: 1,
        sample_rate: TARGET_RATE,
        bits_per_sample: 16,
        sample_format: WavSampleFormat::Int,
    };
    let writer: Arc<Mutex<Option<Writer>>> = Arc::new(Mutex::new(Some(
        WavWriter::create(&path, spec)
            .map_err(|e| format!("impossible de créer le WAV temporaire : {e}"))?,
    )));
    let written = Arc::new(AtomicU64::new(0));
    // Une erreur d'écriture disque dans le callback ne doit pas être avalée :
    // sans cela, l'arrêt rendrait « succès » sur un WAV tronqué.
    let write_error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let cb_stop = stop.clone();
    let cb_conv = converter.clone();
    let cb_writer = writer.clone();
    let cb_written = written.clone();
    let cb_ecriture = write_error.clone();
    let mut last_emit = Instant::now();

    let mut on_audio = move |data: &[f32]| {
        if cb_stop.load(Ordering::Relaxed) || PAUSED.load(Ordering::Relaxed) {
            return;
        }
        let out = match cb_conv.lock() {
            Ok(mut c) => c.push(data),
            Err(_) => return,
        };
        if out.is_empty() {
            return;
        }
        if let Ok(mut guard) = cb_writer.lock() {
            if let Some(w) = guard.as_mut() {
                for s in &out {
                    if let Err(e) = w.write_sample(*s) {
                        if let Ok(mut slot) = cb_ecriture.lock() {
                            *slot = Some(format!("écriture du WAV impossible : {e}"));
                        }
                        cb_stop.store(true, Ordering::Relaxed);
                        return;
                    }
                }
            }
        }
        cb_written.fetch_add(out.len() as u64, Ordering::Relaxed);

        if last_emit.elapsed() >= LEVEL_PERIOD {
            last_emit = Instant::now();
            emit_level(rms(&out));
        }
    };

    let err_flag: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let flag = err_flag.clone();
    let xruns = Arc::new(AtomicU64::new(0));
    let xruns_cb = xruns.clone();
    let err_fn = move |err: cpal::Error| {
        // Un surcoût/retard de tampon (Xrun) est courant sur Windows et ne
        // signifie pas que le micro est perdu : on le compte sans arrêter la
        // capture, et on ne remplit pas le journal (des centaines par minute).
        match err.kind() {
            cpal::ErrorKind::Xrun | cpal::ErrorKind::RealtimeDenied => {
                let n = xruns_cb.fetch_add(1, Ordering::Relaxed);
                if n % 200 == 0 {
                    eprintln!("capture : {err} (occurrence {n})");
                }
            }
            _ => {
                if let Ok(mut slot) = flag.lock() {
                    *slot = Some(err.to_string());
                }
            }
        }
    };

    let stream = match config.sample_format() {
        SampleFormat::F32 => device
            .build_input_stream(
                config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| on_audio(data),
                err_fn,
                None,
            )
            .map_err(|e| format!("ouverture du micro impossible : {e}"))?,
        SampleFormat::I16 => device
            .build_input_stream(
                config.into(),
                move |data: &[i16], _: &cpal::InputCallbackInfo| {
                    let floats: Vec<f32> = data.iter().map(|s| *s as f32 / 32768.0).collect();
                    on_audio(&floats)
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("ouverture du micro impossible : {e}"))?,
        other => return Err(format!("format d'échantillon non pris en charge : {other}")),
    };

    stream
        .play()
        .map_err(|e| format!("démarrage du flux micro impossible : {e}"))?;

    let _ = ready.send(Ok(()));

    // Le flux tourne ; on attend l'arrêt ou une erreur du périphérique.
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if let Ok(slot) = err_flag.lock() {
            if slot.is_some() {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }

    drop(stream);

    let erreur = err_flag
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .or_else(|| write_error.lock().ok().and_then(|s| s.clone()));
    let samples = written.load(Ordering::Relaxed);

    let w = {
        let mut guard = writer.lock().map_err(|_| "WAV verrouillé".to_string())?;
        guard.take().ok_or("WAV déjà finalisé".to_string())?
    };
    w.finalize()
        .map_err(|e| format!("finalisation du WAV impossible : {e}"))?;

    if let Some(e) = erreur {
        return Err(format!("capture interrompue : {e}"));
    }

    Ok(CaptureInfo {
        path: path.to_string_lossy().into_owned(),
        duration_secs: samples as f64 / TARGET_RATE as f64,
        samples,
        sample_rate: TARGET_RATE,
        channels: 1,
    })
}

pub fn rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|s| {
            let v = *s as f64 / 32768.0;
            v * v
        })
        .sum();
    (sum / samples.len() as f64).sqrt() as f32
}

/// Conversion d'un flux natif (N canaux, fréquence quelconque) vers 16 kHz mono.
///
/// Moyenne des canaux, puis interpolation linéaire avec position fractionnaire
/// conservée d'un tampon à l'autre : un tampon de 48 000 trames à 48 kHz produit
/// exactement 16 000 échantillons, sans dérive cumulée.
pub struct Converter {
    channels: usize,
    step: f64,
    buffer: Vec<f32>,
    pos: f64,
    filtres: Vec<Biquad>,
}

/// Filtre passe-bas biquad (RBJ), appliqué avant la décimation.
///
/// Sans lui, un signal 48 kHz réduit à 16 kHz replie tout ce qui dépasse 8 kHz
/// dans la bande utile (un sifflement à 12 kHz devient un bourdonnement à
/// 4 kHz). L'interpolation seule ne filtre pas. Deux étages en cascade pour
/// une pente de 24 dB/octave.
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn passe_bas(freq: f32, sr: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * freq / sr;
        let (sin, cos) = (w0.sin(), w0.cos());
        let alpha = sin / (2.0 * 0.707);
        let b0 = (1.0 - cos) / 2.0;
        let b1 = 1.0 - cos;
        let b2 = (1.0 - cos) / 2.0;
        let a0 = 1.0 + alpha;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

impl Converter {
    pub fn new(channels: usize, in_rate: u32) -> Self {
        Self {
            channels: channels.max(1),
            step: in_rate as f64 / TARGET_RATE as f64,
            buffer: Vec::with_capacity(4096),
            pos: 0.0,
            filtres: if in_rate > TARGET_RATE {
                vec![
                    Biquad::passe_bas(7_200.0, in_rate as f32),
                    Biquad::passe_bas(7_200.0, in_rate as f32),
                ]
            } else {
                Vec::new()
            },
        }
    }

    /// Ajoute des échantillons entrelacés et retourne les échantillons 16 kHz
    /// disponibles.
    pub fn push(&mut self, interleaved: &[f32]) -> Vec<i16> {
        for frame in interleaved.chunks_exact(self.channels) {
            let sum: f32 = frame.iter().sum();
            let mut mono = sum / self.channels as f32;
            for f in &mut self.filtres {
                mono = f.process(mono);
            }
            self.buffer.push(mono);
        }

        let mut out = Vec::new();
        while (self.pos as usize) + 1 < self.buffer.len() {
            let i = self.pos.floor() as usize;
            let frac = self.pos - i as f64;
            let v = self.buffer[i] as f64 * (1.0 - frac) + self.buffer[i + 1] as f64 * frac;
            out.push((v.clamp(-1.0, 1.0) * 32767.0).round() as i16);
            self.pos += self.step;
        }

        let consumed = (self.pos.floor() as usize).min(self.buffer.len());
        if consumed > 0 {
            self.buffer.drain(0..consumed);
            self.pos -= consumed as f64;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn onde(rate: u32, seconds: f64, channels: usize, freq: f32) -> Vec<f32> {
        let frames = (rate as f64 * seconds).round() as usize;
        let mut v = Vec::with_capacity(frames * channels);
        for i in 0..frames {
            let t = i as f32 / rate as f32;
            let s = (2.0 * std::f32::consts::PI * freq * t).sin() * 0.5;
            for _ in 0..channels {
                v.push(s);
            }
        }
        v
    }

    #[test]
    fn dix_secondes_a_48k_stereo_donnent_160000_echantillons() {
        let mut c = Converter::new(2, 48_000);
        let data = onde(48_000, 10.0, 2, 440.0);
        let out = c.push(&data);
        assert_eq!(out.len(), 160_000, "10 s à 16 kHz = 160 000 échantillons");
    }

    #[test]
    fn dix_secondes_a_44100_mono_donnent_160000_echantillons() {
        let mut c = Converter::new(1, 44_100);
        let data = onde(44_100, 10.0, 1, 440.0);
        let out = c.push(&data);
        assert_eq!(out.len(), 160_000);
    }

    #[test]
    fn les_tampons_fractionnaires_ne_derivent_pas() {
        // 1 s de signal envoyée en 7 morceaux inégaux doit donner 16 000
        // échantillons, comme en un seul morceau.
        let data = onde(48_000, 1.0, 1, 440.0);
        let mut c = Converter::new(1, 48_000);
        let mut total = 0;
        let taille = data.len() / 7 + 1;
        for morceau in data.chunks(taille) {
            total += c.push(morceau).len();
        }
        assert_eq!(total, 16_000);
    }

    #[test]
    fn le_wav_produit_a_exactement_320000_octets_de_donnees() {
        let dir = std::env::temp_dir().join(format!("echo-wav-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dix-secondes.wav");

        let mut c = Converter::new(1, 48_000);
        let mut samples = c.push(&onde(48_000, 10.0, 1, 440.0));
        samples.resize(160_000, 0);

        let spec = WavSpec {
            channels: 1,
            sample_rate: TARGET_RATE,
            bits_per_sample: 16,
            sample_format: WavSampleFormat::Int,
        };
        let mut w = WavWriter::create(&path, spec).unwrap();
        for s in &samples {
            w.write_sample(*s).unwrap();
        }
        w.finalize().unwrap();

        let bytes = fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 320_000, "en-tête 44 octets + 320 000 de données");

        // En-tête RIFF/WAVE relu champ par champ.
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1, "mono");
        assert_eq!(
            u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            16_000,
            "16 000 Hz"
        );
        assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16, "16 bits");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]),
            320_000,
            "taille du bloc data"
        );
    }

    #[test]
    fn le_rms_suit_l_amplitude() {
        assert!(rms(&vec![0i16; 1000]) < 0.001);
        let fort: Vec<i16> = vec![16_000; 1000];
        let faible: Vec<i16> = vec![300; 1000];
        assert!(rms(&fort) > 0.4);
        assert!(rms(&faible) < 0.02);
    }

    #[test]
    fn un_ton_aigu_est_attenue_avant_la_decimation() {
        // Sans filtre, 12 kHz à 48 kHz se replie à 4 kHz à 16 kHz et reste
        // presque aussi fort que le 1 kHz. Avec le passe-bas, il doit chuter.
        let mut aigu = Converter::new(1, 48_000);
        let sortie_aigu = aigu.push(&onde(48_000, 1.0, 1, 12_000.0));
        let mut grave = Converter::new(1, 48_000);
        let sortie_grave = grave.push(&onde(48_000, 1.0, 1, 1_000.0));

        let r_aigu = rms(&sortie_aigu);
        let r_grave = rms(&sortie_grave);
        assert!(r_grave > 0.30, "le 1 kHz doit passer (rms {r_grave})");
        assert!(
            r_aigu < r_grave * 0.2,
            "le 12 kHz doit être atténué : {r_aigu} contre {r_grave}"
        );
    }

    #[test]
    fn la_purge_efface_les_wav_et_rien_d_autre() {
        let dir = std::env::temp_dir().join(format!("echo-purge-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("capture-1.wav"), b"x").unwrap();
        fs::write(dir.join("capture-2.wav"), b"x").unwrap();
        fs::write(dir.join("garde.txt"), b"x").unwrap();
        assert_eq!(purge_dir(&dir), 2);
        assert!(dir.join("garde.txt").exists());
    }
}
