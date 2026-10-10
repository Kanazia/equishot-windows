//! Relire un MP4 par Media Foundation, sans lecteur a l'ecran (ADR-016) : les
//! durees des pistes, le plus grand trou, les bips a 880 Hz et les eclairs du
//! centre de l'image. Sert au parcours de bout en bout et aux essais reels.
//! Repris du `verifier` de la preuve technique.

use std::path::Path;

use image::RgbaImage;

use windows62::core::HSTRING;
use windows62::Win32::Media::MediaFoundation::*;
use windows62::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

type Erreur = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Releve {
    pub largeur: u32,
    pub hauteur: u32,
    /// Fin de la piste image (dernier horodatage plus sa duree), en secondes.
    pub image_s: f64,
    pub son_s: f64,
    pub images: u64,
    /// Plus grand ecart entre deux echantillons d'une meme piste, en ms.
    pub trou_image_ms: f64,
    pub trou_son_ms: f64,
    /// Debut de chaque bip a 880 Hz, en secondes du fichier.
    pub bips_s: Vec<f64>,
    /// Plus grand echantillon du son (mono), en valeur absolue : le volume d'un bip (0.17).
    pub pic_son: f32,
    /// Debut de chaque eclair blanc au centre de l'image, en secondes du fichier.
    pub eclairs_s: Vec<f64>,
    /// Luminance moyenne de la premiere image, de 0 a 255.
    pub luminance_premiere: f64,
}

/// Relit tout le fichier. Bloquant : quelques secondes pour une minute de video.
pub fn relever(mp4: &Path) -> Result<Releve, String> {
    releve(mp4).map_err(|e| format!("Lecture de {} impossible : {e}", mp4.display()))
}

fn releve(mp4: &Path) -> Result<Releve, Erreur> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
    }
    let chemin = std::fs::canonicalize(mp4)?;
    let url = HSTRING::from(chemin.to_string_lossy().trim_start_matches(r"\\?\"));
    let mut r = Releve::default();
    let resultat = (|| -> Result<(), Erreur> {
        (r.image_s, r.trou_image_ms, r.images) = piste(&url, true)?;
        (r.son_s, r.trou_son_ms, _) = piste(&url, false)?;
        (r.largeur, r.hauteur, r.eclairs_s, r.luminance_premiere) = eclairs(&url)?;
        (r.bips_s, r.pic_son) = bips(&url)?;
        Ok(())
    })();
    unsafe {
        let _ = MFShutdown();
    }
    resultat.map(|_| r)
}

fn lecteur(url: &HSTRING, attributs: Option<&IMFAttributes>, flux: u32) -> Result<IMFSourceReader, Erreur> {
    unsafe {
        let r = MFCreateSourceReaderFromURL(url, attributs)?;
        r.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        r.SetStreamSelection(flux, true)?;
        Ok(r)
    }
}

/// Lit l'echantillon suivant : `None` a la fin du flux.
fn suivant(r: &IMFSourceReader, flux: u32) -> Result<Option<(i64, IMFSample)>, Erreur> {
    loop {
        let (mut drapeaux, mut horodatage) = (0u32, 0i64);
        let mut echantillon: Option<IMFSample> = None;
        unsafe { r.ReadSample(flux, 0, None, Some(&mut drapeaux), Some(&mut horodatage), Some(&mut echantillon))? };
        if drapeaux & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            return Ok(None);
        }
        if let Some(e) = echantillon {
            return Ok(Some((horodatage, e)));
        }
    }
}

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const SON: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;

/// Piste compressee : fin, plus grand trou (ms), nombre d'echantillons.
fn piste(url: &HSTRING, video: bool) -> Result<(f64, f64, u64), Erreur> {
    let flux = if video { VIDEO } else { SON };
    let r = lecteur(url, None, flux)?;
    let (mut fin, mut temps) = (0i64, Vec::new());
    while let Some((t, e)) = suivant(&r, flux)? {
        fin = fin.max(t + unsafe { e.GetSampleDuration() }.unwrap_or(0));
        temps.push(t);
    }
    temps.sort();
    let trou = temps.windows(2).map(|w| w[1] - w[0]).max().unwrap_or(0);
    Ok((fin as f64 / 1e7, trou as f64 / 1e4, temps.len() as u64))
}

/// Chaque image du fichier, decodee en RGB32 par Media Foundation, rendue en
/// RGBA de haut en bas a sa taille vraie, avec son instant en secondes. `f`
/// rend faux pour s'arreter. Rend la taille de la video.
fn images_rgb32(url: &HSTRING, mut f: impl FnMut(f64, RgbaImage) -> bool) -> Result<(u32, u32), Erreur> {
    let attributs = unsafe {
        let mut a = None;
        MFCreateAttributes(&mut a, 1)?;
        let a = a.ok_or("attributs")?;
        a.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        a
    };
    let r = lecteur(url, Some(&attributs), VIDEO)?;
    let (largeur, hauteur, pas) = unsafe {
        let t = MFCreateMediaType()?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        r.SetCurrentMediaType(VIDEO, None, &t)?;
        let courant = r.GetCurrentMediaType(VIDEO)?;
        // Taille alignee sur 16 par le decodeur : le pas d'une ligne la suit.
        let alignee = courant.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let natif = r.GetNativeMediaType(VIDEO, 0)?.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(alignee);
        // Un pas negatif dit une image de bas en haut.
        let pas = courant.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|p| p as i32).unwrap_or((alignee >> 32) as i32 * 4);
        ((natif >> 32) as u32, (natif & 0xffff_ffff) as u32, pas)
    };
    while let Some((t, e)) = suivant(&r, VIDEO)? {
        let image = unsafe {
            let b = e.ConvertToContiguousBuffer()?;
            let (mut ptr, mut longueur) = (std::ptr::null_mut(), 0u32);
            b.Lock(&mut ptr, None, Some(&mut longueur))?;
            let octets = std::slice::from_raw_parts(ptr as *const u8, longueur as usize);
            let pas_vrai = pas_du_tampon(octets.len(), largeur, hauteur, pas.unsigned_abs() as usize);
            let lignes = octets.len() / pas_vrai;
            let mut image = RgbaImage::new(largeur, hauteur);
            for y in 0..hauteur.min(lignes as u32) {
                let source = if pas < 0 { lignes - 1 - y as usize } else { y as usize } * pas_vrai;
                for x in 0..largeur {
                    let i = source + x as usize * 4;
                    if let Some(p) = octets.get(i..i + 3) {
                        image.put_pixel(x, y, image::Rgba([p[2], p[1], p[0], 255]));
                    }
                }
            }
            b.Unlock()?;
            image
        };
        if !f(t as f64 / 1e7, image) {
            break;
        }
    }
    Ok((largeur, hauteur))
}

/// Le pas vrai d'une ligne du tampon decode. Le decodeur aligne largeur et
/// hauteur sur 16, mais le type annonce parfois le pas de la largeur vraie :
/// 440 px de large donnaient 1 760 annonces pour 1 792 dans le tampon, et une
/// image cisaillee (vu le 7 octobre 2026). On garde le premier pas candidat qui
/// tombe juste sur un nombre de lignes entre la hauteur vraie et l'alignee.
fn pas_du_tampon(longueur: usize, largeur: u32, hauteur: u32, annonce: usize) -> usize {
    let aligne = |n: u32| (n as usize).div_ceil(16) * 16;
    let lignes_possibles = hauteur as usize..=aligne(hauteur);
    [aligne(largeur) * 4, annonce, largeur as usize * 4]
        .into_iter()
        .find(|&p| p > 0 && longueur % p == 0 && lignes_possibles.contains(&(longueur / p)))
        .unwrap_or(annonce.max(1))
}

/// Les images du fichier pour un GIF (ADR-016) : une toutes les `pas_s` secondes,
/// sur `max_s` secondes au plus. Bloquant.
pub fn images(mp4: &Path, pas_s: f64, max_s: f64, mut f: impl FnMut(RgbaImage)) -> Result<(), String> {
    let lu = (|| -> Result<(), Erreur> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        }
        let url = HSTRING::from(std::fs::canonicalize(mp4)?.to_string_lossy().trim_start_matches(r"\\?\"));
        let mut prochain = 0.0;
        let lu = images_rgb32(&url, |t, image| {
            if t + 1e-3 >= prochain {
                f(image);
                prochain += pas_s;
            }
            prochain < max_s - 1e-3
        });
        unsafe {
            let _ = MFShutdown();
        }
        lu.map(|_| ())
    })();
    lu.map_err(|e| format!("Lecture de {} impossible : {e}", mp4.display()))
}

/// `t:x,y` : un instant en secondes et un point de l'image.
pub fn lire_demande(arg: &str) -> Option<(f64, u32, u32)> {
    let (t, point) = arg.split_once(':')?;
    let (x, y) = point.split_once(',')?;
    Some((t.trim().parse().ok()?, x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Sert chaque demande non encore servie dont l'instant est atteint ; vrai quand tout est servi.
fn servir(t: f64, image: &RgbaImage, demandes: &[(f64, u32, u32)], sorties: &mut [Option<[u8; 3]>]) -> bool {
    for ((instant, x, y), sortie) in demandes.iter().zip(sorties.iter_mut()) {
        if sortie.is_none() && t + 1e-3 >= *instant {
            // Un point hors de l'image est ramene au bord ; une image vide ne sert rien.
            let p = image.get_pixel_checked((*x).min(image.width().saturating_sub(1)), (*y).min(image.height().saturating_sub(1)));
            *sortie = p.map(|p| [p[0], p[1], p[2]]);
        }
    }
    sorties.iter().all(Option::is_some)
}

/// La couleur de l'image en chaque point demande, a l'instant demande : le parcours
/// y lit la bulle de camera (0.18). `None` au-dela du fichier. Bloquant.
pub fn couleurs(mp4: &Path, demandes: &[(f64, u32, u32)]) -> Result<Vec<Option<[u8; 3]>>, String> {
    let mut sorties = vec![None; demandes.len()];
    let lu = (|| -> Result<(), Erreur> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
        }
        let url = HSTRING::from(std::fs::canonicalize(mp4)?.to_string_lossy().trim_start_matches(r"\\?\"));
        let lu = images_rgb32(&url, |t, image| !servir(t, &image, demandes, &mut sorties));
        unsafe {
            let _ = MFShutdown();
        }
        lu.map(|_| ())
    })();
    lu.map_err(|e| format!("Lecture de {} impossible : {e}", mp4.display()))?;
    Ok(sorties)
}

/// La luminance du centre de chaque image. Un eclair commence quand elle passe
/// 200 apres au moins 0,3 s sous 100.
fn eclairs(url: &HSTRING) -> Result<(u32, u32, Vec<f64>, f64), Erreur> {
    let (mut debuts, mut sombre_depuis, mut premiere) = (Vec::new(), None::<f64>, None);
    let (largeur, hauteur) = images_rgb32(url, |s, image| {
        let (l, h) = image.dimensions();
        premiere.get_or_insert_with(|| luminance(&image, 0, 0, l, h));
        let centre = luminance(&image, l / 2 - 10.min(l / 2), h / 2 - 10.min(h / 2), 20.min(l), 20.min(h));
        if centre < 100.0 {
            sombre_depuis.get_or_insert(s);
        } else if centre > 200.0 {
            if sombre_depuis.is_some_and(|d| s - d >= 0.3) {
                debuts.push(s);
            }
            sombre_depuis = None;
        }
        true
    })?;
    Ok((largeur, hauteur, debuts, premiere.unwrap_or(0.0)))
}

/// Luminance moyenne d'un rectangle de l'image.
fn luminance(image: &RgbaImage, x: u32, y: u32, l: u32, h: u32) -> f64 {
    let (mut somme, mut n) = (0.0, 0u64);
    for ligne in y..(y + h).min(image.height()) {
        for colonne in x..(x + l).min(image.width()) {
            let p = image.get_pixel(colonne, ligne);
            somme += 0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64;
            n += 1;
        }
    }
    if n == 0 {
        0.0
    } else {
        somme / n as f64
    }
}

/// Le son decode en flottant : debut de chaque bip a 880 Hz (Goertzel par
/// fenetres de 10 ms, seuil a 10 % du maximum, 300 ms de calme entre deux),
/// et le plus grand echantillon.
fn bips(url: &HSTRING) -> Result<(Vec<f64>, f32), Erreur> {
    let r = lecteur(url, None, SON)?;
    let (canaux, taux) = unsafe {
        let t = MFCreateMediaType()?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        t.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)?;
        r.SetCurrentMediaType(SON, None, &t)?;
        let mt = r.GetCurrentMediaType(SON)?;
        (mt.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)? as usize, mt.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)? as f64)
    };
    let (mut mono, mut debut) = (Vec::<f32>::new(), None);
    while let Some((t, e)) = suivant(&r, SON)? {
        debut.get_or_insert(t);
        unsafe {
            let b = e.ConvertToContiguousBuffer()?;
            let (mut ptr, mut longueur) = (std::ptr::null_mut(), 0u32);
            b.Lock(&mut ptr, None, Some(&mut longueur))?;
            let f = std::slice::from_raw_parts(ptr as *const f32, longueur as usize / 4);
            mono.extend(f.chunks_exact(canaux.max(1)).map(|c| c.iter().sum::<f32>() / canaux.max(1) as f32));
            b.Unlock()?;
        }
    }
    let pic = mono.iter().fold(0f32, |m, x| m.max(x.abs()));
    let d0 = debut.unwrap_or(0) as f64 / 1e7;
    let fenetre = (taux / 100.0) as usize;
    let coeff = 2.0 * (2.0 * std::f64::consts::PI * 880.0 / taux).cos();
    let puissances: Vec<f64> = mono
        .chunks(fenetre.max(1))
        .map(|w| {
            let (mut s1, mut s2) = (0.0f64, 0.0f64);
            for &x in w {
                let s0 = x as f64 + coeff * s1 - s2;
                s2 = s1;
                s1 = s0;
            }
            (s1 * s1 + s2 * s2 - coeff * s1 * s2) / (w.len() as f64 * w.len() as f64)
        })
        .collect();
    let max = puissances.iter().cloned().fold(0.0, f64::max);
    // Un son sans bip : rien a trouver (le seuil relatif trouverait du bruit).
    if max < 1e-6 {
        return Ok((Vec::new(), pic));
    }
    let (mut debuts, mut calme) = (Vec::new(), usize::MAX);
    for (i, &p) in puissances.iter().enumerate() {
        if p > max * 0.1 {
            if calme >= 30 {
                debuts.push(d0 + i as f64 * 0.01);
            }
            calme = 0;
        } else {
            calme = calme.saturating_add(1);
        }
    }
    Ok((debuts, pic))
}

#[cfg(test)]
mod tests {
    use super::{lire_demande, pas_du_tampon, servir};
    use image::RgbaImage;

    #[test]
    fn une_demande_se_lit() {
        assert_eq!(lire_demande("1.5:104,216"), Some((1.5, 104, 216)));
        assert_eq!(lire_demande("2:0,0"), Some((2.0, 0, 0)));
        assert_eq!(lire_demande("x:1,2"), None);
        assert_eq!(lire_demande("1:1"), None);
    }

    #[test]
    fn servir_rend_la_premiere_image_qui_atteint_l_instant() {
        let mut image = RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
        image.put_pixel(3, 1, image::Rgba([200, 100, 50, 255]));
        let demandes = [(1.0, 3, 1), (2.0, 0, 0), (0.5, 9, 9)];
        let mut sorties = vec![None; 3];
        assert!(!servir(0.4, &image, &demandes, &mut sorties));
        assert_eq!(sorties, vec![None, None, None]);
        assert!(!servir(1.2, &image, &demandes, &mut sorties));
        // Un point hors de l'image est ramene au bord.
        assert_eq!(sorties, vec![Some([200, 100, 50]), None, Some([10, 20, 30])]);
        assert!(servir(2.0, &image, &demandes, &mut sorties), "toutes servies");
        assert_eq!(sorties[1], Some([10, 20, 30]));
    }

    #[test]
    fn pas_du_tampon_largeur_non_alignee() {
        // Mesure du 7 octobre 2026 : 440 x 320, 1 760 annonces, 573 440 octets.
        assert_eq!(pas_du_tampon(573_440, 440, 320, 1760), 1792);
    }

    #[test]
    fn pas_du_tampon_hauteur_alignee() {
        // 640 x 360 : le tampon porte 368 lignes de 2 560 octets.
        assert_eq!(pas_du_tampon(2560 * 368, 640, 360, 2560), 2560);
        assert_eq!(pas_du_tampon(2560 * 360, 640, 360, 2560), 2560);
    }

    #[test]
    fn pas_du_tampon_inconnu_garde_l_annonce() {
        assert_eq!(pas_du_tampon(1000, 440, 320, 1760), 1760);
    }
}
