//! Le texte d'une image, par le moteur OCR de Windows (ADR-010). Local, sans
//! service distant. Bloquant : hors du thread principal.

use image::RgbaImage;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;

/// Sous cette hauteur, l'image est agrandie deux fois avant lecture : un
/// « Bonjour » de 12 px dans 24 px de haut est lu vide sans cela. Reglage
/// mesure, pas une verite.
const AGRANDIR_SOUS: u32 = 40;

/// Le texte de l'image, une ligne OCR par ligne, vide si rien n'est reconnu.
/// Une lecture vide est refaite sur l'image agrandie deux fois : un texte de 9
/// ou 10 px dans une grande zone n'est lu qu'ainsi (mesure, `docs/TESTS.md`).
// ponytail: une image sans texte est lue deux fois. Un texte de 9 px voisin
// d'un texte plus grand reste perdu : la premiere lecture n'est pas vide.
pub fn lire_texte(image: &RgbaImage) -> Result<String, String> {
    let erreur = |e: windows::core::Error| format!("Lecture du texte impossible : {e}");
    let maximum = OcrEngine::MaxImageDimension().map_err(erreur)?;
    if image.width() > maximum || image.height() > maximum {
        return Err(format!(
            "Image trop grande pour la lecture du texte ({} x {}, {maximum} px au plus par côté).",
            image.width(),
            image.height()
        ));
    }
    // Repli sur l'anglais si le profil n'a aucune langue OCR (spec, ADR-010).
    let moteur = OcrEngine::TryCreateFromUserProfileLanguages()
        .or_else(|_| OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&windows::core::HSTRING::from("en-US"))?))
        .map_err(|_| "Aucune langue de lecture du texte n'est installée dans Windows.".to_string())?;
    let double = |image: &RgbaImage| {
        (image.width() * 2 <= maximum && image.height() * 2 <= maximum)
            .then(|| image::imageops::resize(image, image.width() * 2, image.height() * 2, image::imageops::FilterType::CatmullRom))
    };
    if image.height() < AGRANDIR_SOUS {
        if let Some(agrandie) = double(image) {
            return lire(&moteur, &agrandie);
        }
    }
    let texte = lire(&moteur, image)?;
    match double(image) {
        Some(agrandie) if texte.is_empty() => lire(&moteur, &agrandie),
        _ => Ok(texte),
    }
}

fn lire(moteur: &OcrEngine, image: &RgbaImage) -> Result<String, String> {
    let erreur = |e: windows::core::Error| format!("Lecture du texte impossible : {e}");
    // RGBA vers BGRA, le format que lit le moteur.
    let mut bgra = image.as_raw().clone();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let ecrivain = DataWriter::new().map_err(erreur)?;
    ecrivain.WriteBytes(&bgra).map_err(erreur)?;
    let tampon = ecrivain.DetachBuffer().map_err(erreur)?;
    let bitmap = SoftwareBitmap::Create(BitmapPixelFormat::Bgra8, image.width() as i32, image.height() as i32).map_err(erreur)?;
    bitmap.CopyFromBuffer(&tampon).map_err(erreur)?;
    let resultat = moteur.RecognizeAsync(&bitmap).map_err(erreur)?.get().map_err(erreur)?;
    let mut lignes = Vec::new();
    for ligne in resultat.Lines().map_err(erreur)? {
        lignes.push(ligne.Text().map_err(erreur)?.to_string());
    }
    Ok(lignes.join("\n"))
}

/// Lit la prochaine capture sans texte et le pose. `None` : plus rien a lire.
/// Une lecture impossible pose une chaine vide : elle n'est pas retentee.
/// Le verrou de la base n'est jamais tenu pendant la lecture.
pub fn lire_une(etat: &crate::AppState, dossier: &std::path::Path) -> Option<i64> {
    let (id, fichier) = match etat.with_conn(super::stockage::prochaine_a_lire) {
        Ok(prochaine) => prochaine?,
        Err(motif) => {
            eprintln!("texte des captures : {motif}");
            return None;
        }
    };
    let origine = super::stockage::chemins(dossier, &fichier).origine;
    let texte = super::stockage::lire_png(&origine).and_then(|image| lire_texte(&image)).unwrap_or_else(|motif| {
        eprintln!("texte de la capture {id} non lu : {motif}");
        String::new()
    });
    if let Err(motif) = etat.with_conn(|conn| super::stockage::poser_texte(conn, id, &texte)) {
        // Sans texte pose, la meme capture reviendrait aussitot : le fil s'arrete
        // la, jusqu'au prochain reveil.
        eprintln!("texte de la capture {id} non enregistré : {motif}");
        return None;
    }
    Some(id)
}

static REVEIL: std::sync::OnceLock<std::sync::mpsc::Sender<()>> = std::sync::OnceLock::new();

/// Le fil de lecture : prend toujours la capture la plus recente sans texte, ce
/// qui fait passer une capture neuve devant le rattrapage des anciennes.
// ponytail: un fil, une capture a la fois. A paralleliser si le rattrapage
// d'une grosse galerie se fait sentir.
pub fn demarrer(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let (envoi, reception) = std::sync::mpsc::channel::<()>();
    if REVEIL.set(envoi).is_err() {
        return;
    }
    let app = app.clone();
    let mut cadence = Cadence::default();
    std::thread::spawn(move || loop {
        let lue = super::dossier(&app).ok().and_then(|dossier| lire_une(&app.state::<crate::AppState>(), &dossier));
        match lue {
            Some(_) => {
                if cadence.lue(std::time::Instant::now()) {
                    let _ = app.emit(super::EVENEMENT_CHANGEE, ());
                }
            }
            None => {
                if cadence.vide() {
                    let _ = app.emit(super::EVENEMENT_CHANGEE, ());
                }
                if reception.recv().is_err() {
                    return;
                }
            }
        }
    });
}

/// Pendant un rattrapage, la galerie relancerait sa requete a chaque capture
/// lue : elle n'est prevenue qu'une fois par seconde au plus, et une derniere
/// fois quand la file est vide. La premiere lecture d'un lot se dit aussitot.
#[derive(Default)]
struct Cadence {
    derniere: Option<std::time::Instant>,
    en_attente: bool,
}

impl Cadence {
    const ECART: std::time::Duration = std::time::Duration::from_secs(1);

    /// Une capture est lue : vrai s'il faut prevenir maintenant.
    fn lue(&mut self, maintenant: std::time::Instant) -> bool {
        if self.derniere.is_some_and(|d| maintenant.duration_since(d) < Self::ECART) {
            self.en_attente = true;
            return false;
        }
        self.derniere = Some(maintenant);
        self.en_attente = false;
        true
    }

    /// Plus rien a lire : vrai si une lecture n'a pas encore ete dite.
    fn vide(&mut self) -> bool {
        std::mem::take(&mut self.en_attente)
    }
}

/// Une capture neuve attend sa lecture.
pub fn reveiller() {
    if let Some(envoi) = REVEIL.get() {
        let _ = envoi.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn charger(nom: &str) -> image::RgbaImage {
        let chemin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(nom);
        image::open(chemin).expect("image témoin").to_rgba8()
    }

    #[test]
    fn captures_ocr_lit_l_image_temoin() {
        let texte = lire_texte(&charger("ocr-temoin.png")).expect("OCR");
        assert!(texte.contains("facture"), "lu : {texte:?}");
        assert!(texte.contains("2026"), "lu : {texte:?}");
    }

    #[test]
    fn captures_ocr_lit_un_petit_mot() {
        let texte = lire_texte(&charger("ocr-petit.png")).expect("OCR");
        assert!(texte.contains("Bonjour"), "lu : {texte:?}");
    }

    #[test]
    fn captures_ocr_lit_un_petit_mot_dans_une_grande_zone() {
        let texte = lire_texte(&charger("ocr-large-petit.png")).expect("OCR");
        assert!(texte.contains("Bonjour"), "lu : {texte:?}");
    }

    #[test]
    fn captures_ocr_image_vide_rend_une_chaine_vide() {
        let blanc = image::RgbaImage::from_pixel(200, 50, image::Rgba([255, 255, 255, 255]));
        assert_eq!(lire_texte(&blanc).expect("OCR"), "");
    }

    #[test]
    fn captures_ocr_fil_la_plus_recente_d_abord() {
        use super::super::stockage;
        let dossier = std::env::temp_dir().join(format!("equishot-ocr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        let lisible = stockage::chemins(&dossier, "2026-10/lisible.png").origine;
        std::fs::create_dir_all(lisible.parent().expect("dossier du mois")).expect("dossier");
        std::fs::copy(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("ocr-temoin.png"), &lisible).expect("copie");
        let etat = crate::AppState {
            db: std::sync::Mutex::new(crate::db::open_in_memory().expect("base")),
            data_dir: dossier.clone(),
            dev: true,
        };
        let quand = chrono::Local::now();
        let origine = stockage::Origine::default();
        let (lisible_id, absente_id) = etat
            .with_conn(|conn| {
                let a = stockage::inserer(conn, &quand, "2026-10/lisible.png", (400, 60), &origine, "[]", None)?;
                let b = stockage::inserer(conn, &quand, "2026-10/absente.png", (10, 10), &origine, "[]", None)?;
                Ok((a, b))
            })
            .expect("insertions");
        assert_eq!(lire_une(&etat, &dossier), Some(absente_id), "la plus récente d'abord");
        assert_eq!(lire_une(&etat, &dossier), Some(lisible_id));
        assert_eq!(lire_une(&etat, &dossier), None, "plus rien à lire");
        let texte = |id| etat.with_conn(|conn| stockage::texte_de(conn, id)).expect("texte");
        assert_eq!(texte(absente_id).as_deref(), Some(""), "un fichier absent n'est pas retenté");
        assert!(texte(lisible_id).is_some_and(|t| t.contains("facture")), "lu : {:?}", texte(lisible_id));
        let _ = std::fs::remove_dir_all(&dossier);
    }

    #[test]
    fn captures_ocr_cadence_une_annonce_par_seconde_et_une_en_fin_de_lot() {
        let t0 = std::time::Instant::now();
        let ms = std::time::Duration::from_millis;
        let mut c = Cadence::default();
        assert!(c.lue(t0), "la première lecture se dit aussitôt");
        assert!(!c.lue(t0 + ms(100)));
        assert!(!c.lue(t0 + ms(900)));
        assert!(c.lue(t0 + ms(1000)), "une seconde après, de nouveau");
        assert!(!c.vide(), "rien en attente juste après une annonce");
        assert!(!c.lue(t0 + ms(1500)));
        assert!(c.vide(), "la fin du lot dit la dernière lecture");
        assert!(!c.vide(), "une seule fois");
    }

    #[test]
    fn captures_ocr_refuse_une_image_trop_grande() {
        let large = image::RgbaImage::new(10_001, 1);
        assert!(lire_texte(&large).is_err());
    }
}
