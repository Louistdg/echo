//! Capture d'écran d'une zone, en GDI pur (aucune dépendance externe côté OS).
//!
//! Le panneau plein écran laisse l'utilisateur dessiner un rectangle ; la zone
//! est ensuite copiée depuis l'écran et enregistrée en PNG dans le dossier de
//! notes (`captures/`).

use std::path::{Path, PathBuf};

/// Copie une zone de l'écran en pixels RGBA (largeur × hauteur × 4).
#[cfg(windows)]
pub fn copier_zone(x: i32, y: i32, largeur: i32, hauteur: i32) -> Result<Vec<u8>, String> {
    use std::ffi::c_void;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDC(hwnd: *mut c_void) -> *mut c_void;
        fn ReleaseDC(hwnd: *mut c_void, hdc: *mut c_void) -> i32;
    }
    #[link(name = "gdi32")]
    unsafe extern "system" {
        fn CreateCompatibleDC(hdc: *mut c_void) -> *mut c_void;
        fn CreateCompatibleBitmap(hdc: *mut c_void, cx: i32, cy: i32) -> *mut c_void;
        fn SelectObject(hdc: *mut c_void, obj: *mut c_void) -> *mut c_void;
        fn BitBlt(dst: *mut c_void, x: i32, y: i32, cx: i32, cy: i32, src: *mut c_void, sx: i32, sy: i32, rop: u32) -> i32;
        fn GetDIBits(hdc: *mut c_void, bmp: *mut c_void, start: u32, lignes: u32, bits: *mut c_void, info: *mut BitmapInfo, usage: u32) -> i32;
        fn DeleteObject(obj: *mut c_void) -> i32;
        fn DeleteDC(hdc: *mut c_void) -> i32;
    }

    #[repr(C)]
    struct BitmapInfoHeader {
        taille: u32,
        largeur: i32,
        hauteur: i32,
        plans: u16,
        bits: u16,
        compression: u32,
        taille_image: u32,
        xppm: i32,
        yppm: i32,
        couleurs: u32,
        importantes: u32,
    }
    #[repr(C)]
    struct BitmapInfo {
        entete: BitmapInfoHeader,
        couleurs: [u32; 3],
    }

    if largeur <= 0 || hauteur <= 0 {
        return Err("zone de capture vide".into());
    }

    unsafe {
        let ecran = GetDC(std::ptr::null_mut());
        if ecran.is_null() {
            return Err("écran inaccessible".into());
        }
        let memoire = CreateCompatibleDC(ecran);
        let bitmap = CreateCompatibleBitmap(ecran, largeur, hauteur);
        if memoire.is_null() || bitmap.is_null() {
            ReleaseDC(std::ptr::null_mut(), ecran);
            return Err("création du bitmap impossible".into());
        }
        let ancien = SelectObject(memoire, bitmap);
        // SRCCOPY | CAPTUREBLT : inclut aussi les fenêtres à couches
        let ok = BitBlt(memoire, 0, 0, largeur, hauteur, ecran, x, y, 0x00CC_0020 | 0x4000_0000);
        if ok == 0 {
            SelectObject(memoire, ancien);
            DeleteObject(bitmap);
            DeleteDC(memoire);
            ReleaseDC(std::ptr::null_mut(), ecran);
            return Err("copie de l'écran refusée par Windows".into());
        }

        let mut info = BitmapInfo {
            entete: BitmapInfoHeader {
                taille: std::mem::size_of::<BitmapInfoHeader>() as u32,
                largeur,
                // hauteur négative = lignes de haut en bas
                hauteur: -hauteur,
                plans: 1,
                bits: 32,
                compression: 0,
                taille_image: (largeur * hauteur * 4) as u32,
                xppm: 0,
                yppm: 0,
                couleurs: 0,
                importantes: 0,
            },
            couleurs: [0; 3],
        };
        let mut pixels = vec![0u8; (largeur * hauteur * 4) as usize];
        let lignes = GetDIBits(memoire, bitmap, 0, hauteur as u32,
            pixels.as_mut_ptr() as *mut c_void, &mut info, 0);

        SelectObject(memoire, ancien);
        DeleteObject(bitmap);
        DeleteDC(memoire);
        ReleaseDC(std::ptr::null_mut(), ecran);

        if lignes == 0 {
            return Err("lecture des pixels impossible".into());
        }
        // BGRA -> RGBA
        for px in pixels.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }
        Ok(pixels)
    }
}

#[cfg(not(windows))]
pub fn copier_zone(_x: i32, _y: i32, _largeur: i32, _hauteur: i32) -> Result<Vec<u8>, String> {
    Err("la capture d'écran est disponible sur Windows".into())
}

/// Enregistre un PNG dans `dossier/captures/`, renvoie le chemin créé.
pub fn enregistrer_png(dossier_notes: &Path, largeur: u32, hauteur: u32, pixels: &[u8]) -> Result<PathBuf, String> {
    let dossier = dossier_notes.join("captures");
    std::fs::create_dir_all(&dossier).map_err(|e| format!("dossier captures inaccessible : {e}"))?;
    let nom = format!("{}.png", chrono::Local::now().format("%Y-%m-%d-%H%M%S"));
    let chemin = dossier.join(nom);
    let tampon = image::RgbaImage::from_raw(largeur, hauteur, pixels.to_vec())
        .ok_or_else(|| "dimensions d'image invalides".to_string())?;
    tampon.save(&chemin).map_err(|e| format!("écriture du PNG impossible : {e}"))?;
    Ok(chemin)
}

/// Décode un PNG donné en base64 et l'écrit dans `dossier/captures/`.
pub fn enregistrer_png_base64(dossier_notes: &Path, donnees: &str) -> Result<PathBuf, String> {
    use base64::Engine;
    let brut = donnees.split(',').last().unwrap_or(donnees);
    let octets = base64::engine::general_purpose::STANDARD
        .decode(brut)
        .map_err(|e| format!("image base64 illisible : {e}"))?;
    let image = image::load_from_memory(&octets).map_err(|e| format!("PNG invalide : {e}"))?;
    let dossier = dossier_notes.join("captures");
    std::fs::create_dir_all(&dossier).map_err(|e| format!("dossier captures inaccessible : {e}"))?;
    let chemin = dossier.join(format!("{}.png", chrono::Local::now().format("%Y-%m-%d-%H%M%S")));
    image.save(&chemin).map_err(|e| format!("écriture du PNG impossible : {e}"))?;
    Ok(chemin)
}

/// Copie une image PNG (base64) dans le presse-papiers Windows.
pub fn copier_image_base64(donnees: &str) -> Result<(), String> {
    use base64::Engine;
    let brut = donnees.split(',').last().unwrap_or(donnees);
    let octets = base64::engine::general_purpose::STANDARD
        .decode(brut)
        .map_err(|e| format!("image base64 illisible : {e}"))?;
    let image = image::load_from_memory(&octets).map_err(|e| format!("PNG invalide : {e}"))?;
    let rgba = image.to_rgba8();
    let (largeur, hauteur) = rgba.dimensions();
    let mut presse = arboard::Clipboard::new().map_err(|e| format!("presse-papiers inaccessible : {e}"))?;
    presse
        .set_image(arboard::ImageData { width: largeur as usize, height: hauteur as usize, bytes: rgba.into_raw().into() })
        .map_err(|e| format!("copie de l'image impossible : {e}"))?;
    Ok(())
}
