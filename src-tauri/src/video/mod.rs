//! La video locale (ADR-016) : la selection en mode video donne la zone, un
//! compte a rebours de 3 s passe dans la barre d'enregistrement, puis le moteur
//! filme. Arreter range le MP4 dans la galerie et le copie comme fichier.
//!
//! La barre (`enregistrement.html`) est protegee (`content_protected`) : elle
//! n'est jamais dans la video. Pendant une zone, la selection reste ouverte,
//! reduite a son contour et traversee par la souris ; en ecran entier, elle se
//! ferme. Tout ce qui attend (decompte, moteur, disque) tourne hors du thread
//! principal.

pub mod animation;
pub mod bulle;
pub mod direct;
pub mod frise;
pub mod gif;
pub mod lecture;
pub mod moteur;
pub mod ecoute;
pub mod effets;
pub mod peripheriques;
pub mod scenes;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::captures::stockage::{self, Capture, Origine, ZoneCss};
use crate::{captures, db, fenetres, hors_thread_principal, AppState};
pub use bulle::Rect;
use moteur::{Demande, Enregistrement, Reglages};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

pub const ETIQUETTE_BARRE: &str = "video-barre";
/// Emis vers la barre : la phase, le decompte, le temps filme.
pub const EVENEMENT_ETAT: &str = "video://etat";
/// Reglage : les choix du son et du curseur, en JSON (`Reglages`).
const CLE_REGLAGES: &str = "video_reglages";
/// Taille de la barre, en pixels CSS. 472 depuis la 0.22 : le zoom et le projecteur y prennent 2 x 36 px.
const BARRE: (f64, f64) = (472.0, 52.0);
/// La meme fenetre pendant la mise en scene : le panneau de preparation. La rangee
/// Camera (0.18) ajoute 90 px estimes, a verifier a l'oeil a la prise unique.
/// La rangee Scene (0.19) ajoute 70 px estimes, a verifier a l'oeil a la prise unique.
/// La rangee Effets (0.22) ajoute 75 px estimes, a verifier a l'oeil.
const PANNEAU: (f64, f64) = (420.0, 575.0);
/// Emis vers la barre : `Survol`, a chaque changement.
const EVENEMENT_SURVOL: &str = "video://survol";
/// La poignee de la barre (point rouge et duree, a gauche), en pixels CSS.
const POIGNEE_CSS: f64 = 96.0;
/// Une heure filmee au plus ; la barre previent cinq minutes avant.
const PLAFOND_MS: u64 = 3_600_000;
const PREVENIR_MS: u64 = 3_300_000;

fn tenir<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Default)]
pub struct EtatVideo {
    en_cours: Mutex<Option<EnCours>>,
    dernier: AtomicU64,
}

struct EnCours {
    numero: u64,
    /// `None` pendant le decompte.
    enreg: Option<Enregistrement>,
    reste: u32,
    sortie: PathBuf,
    quand: chrono::DateTime<chrono::Local>,
    origine: Origine,
    /// Les sources son demandees qui manquent, a dire dans la barre.
    absentes: Vec<String>,
    /// Gardee pendant la mise en scene, prise par `video_lancer` : sa presence
    /// fait la phase `prepare`.
    demande: Option<Demande>,
    /// Les jauges de la mise en scene ; `None` le temps d'en relancer une.
    ecoute: Option<ecoute::Ecoute>,
    /// Places du panneau et de la barre, pixels physiques du bureau.
    places: ((i32, i32), (i32, i32)),
    /// La fenetre a qui rendre le premier plan au lancement.
    devant: isize,
    /// La zone filmee (pixels physiques du bureau) et l'echelle de son ecran : la
    /// bulle de camera s'y place et s'y borne.
    zone_bureau: Rect,
    echelle: f64,
    /// La bulle de camera ouverte ; `None` sans camera ou zone trop petite.
    bulle: Option<bulle::Bulle>,
    /// Les cameras enumerees par la page de la bulle, pour le panneau.
    cameras: Vec<bulle::Camera>,
    /// Zone trop petite, camera perdue : dit par le panneau, puis par la barre.
    notes_camera: Vec<String>,
    /// Ce que filme cette mise en scene : ce que « Enregistrer comme scène » garde.
    cible: scenes::Cible,
    /// La scene appliquee, un ecran de repli : dit par le panneau.
    notes_scene: Vec<String>,
    /// Les effets de la prise (ADR-022) et les raccourcis du zoom et du projecteur,
    /// poses seulement pendant qu'elle filme.
    direct: Option<direct::Partage>,
    raccourcis: Vec<Shortcut>,
    raccourcis_lisibles: (String, String),
}

/// Ce que la barre affiche.
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EtatBarre {
    /// `prepare`, `decompte`, `filme`, `pause`, ou `aucun` quand rien ne tourne.
    phase: &'static str,
    reste: u32,
    actif_ms: u64,
    avertissement: Option<String>,
    niveaux: moteur::Niveaux,
    /// Mise en scene : les sources qui n'ont pas pu s'ouvrir.
    notes: Vec<String>,
    /// Les cameras branchees, vues par la page de la bulle.
    cameras: Vec<bulle::Camera>,
    /// Mise en scene : ce qui est filme (0.19).
    cible: Option<scenes::Cible>,
    /// Le zoom et le projecteur (0.22), et leurs raccourcis lisibles.
    zoom: bool,
    projecteur: bool,
    raccourcis_effets: (String, String),
}

impl EnCours {
    fn etat(&self) -> EtatBarre {
        if self.demande.is_some() {
            let ecoute = self.ecoute.as_ref();
            let mut notes: Vec<String> = ecoute.map(|e| e.notes().iter().map(|n| capitaliser(n)).collect()).unwrap_or_default();
            notes.extend(self.notes_camera.iter().cloned());
            notes.extend(self.notes_scene.iter().cloned());
            return EtatBarre {
                phase: "prepare",
                niveaux: ecoute.map(ecoute::Ecoute::niveaux).unwrap_or_default(),
                notes,
                cameras: self.cameras.clone(),
                cible: Some(self.cible.clone()),
                raccourcis_effets: self.raccourcis_lisibles.clone(),
                ..Default::default()
            };
        }
        let Some(enreg) = &self.enreg else {
            return EtatBarre { phase: "decompte", reste: self.reste, ..Default::default() };
        };
        let actif_ms = enreg.actif_ms();
        // `try_lock` : le fil des images tient ce verrou le temps d'une scene.
        let (zoom, projecteur) = self.direct.as_ref().and_then(|d| d.try_lock().ok().map(|d| d.allumes())).unwrap_or_default();
        EtatBarre {
            phase: if enreg.en_pause() { "pause" } else { "filme" },
            reste: 0,
            actif_ms,
            avertissement: avertissement_de(&self.absentes, enreg.perdues(), &self.notes_camera, actif_ms),
            niveaux: enreg.niveaux(),
            notes: Vec::new(),
            cameras: self.cameras.clone(),
            cible: None,
            zoom,
            projecteur,
            raccourcis_effets: self.raccourcis_lisibles.clone(),
        }
    }
}

/// Les raccourcis du zoom et du projecteur, poses pendant la prise. En dev, Maj en plus :
/// l'instance de production garde les siens.
fn raccourcis_effets(dev: bool) -> [&'static str; 2] {
    if dev {
        ["Ctrl+Alt+Shift+Z", "Ctrl+Alt+Shift+L"]
    } else {
        ["Ctrl+Alt+Z", "Ctrl+Alt+L"]
    }
}

fn lisible(combinaison: &str) -> String {
    combinaison.replace("Shift", "Maj").replace('+', " + ")
}

fn raccourcis_lisibles(dev: bool) -> (String, String) {
    let [z, l] = raccourcis_effets(dev).map(lisible);
    (z, l)
}

/// Pose les deux raccourcis ; rend ceux qui ont tenu et une note par refus. Hors de
/// tout verrou : le greffon attend le thread principal.
fn poser_raccourcis_effets(app: &tauri::AppHandle) -> (Vec<Shortcut>, Vec<String>) {
    let dev = app.state::<AppState>().dev;
    let (mut poses, mut notes) = (Vec::new(), Vec::new());
    for (combinaison, nom) in raccourcis_effets(dev).into_iter().zip(["zoom", "projecteur"]) {
        match combinaison.parse::<Shortcut>() {
            Ok(r) if app.global_shortcut().register(r).is_ok() => poses.push(r),
            _ => notes.push(format!("{nom} sans raccourci ({} déjà pris)", lisible(combinaison))),
        }
    }
    (poses, notes)
}

fn retirer_raccourcis_effets(app: &tauri::AppHandle, raccourcis: &[Shortcut]) {
    for r in raccourcis {
        let _ = app.global_shortcut().unregister(*r);
    }
}

/// Le gestionnaire global des raccourcis, sur le thread principal : vrai si la frappe
/// est celle du zoom ou du projecteur d'une prise en cours. Jamais d'attente (`try_lock`).
pub fn sur_raccourci_effet(app: &tauri::AppHandle, raccourci: &Shortcut) -> bool {
    let etat = app.state::<EtatVideo>();
    let Ok(en_cours) = etat.en_cours.try_lock() else { return false };
    let Some(e) = en_cours.as_ref() else { return false };
    let Some(i) = e.raccourcis.iter().position(|r| r == raccourci) else { return false };
    if let Some(mut d) = e.direct.as_ref().and_then(|d| d.try_lock().ok()) {
        if i == 0 {
            d.basculer_zoom();
        } else {
            d.basculer_projecteur();
        }
    }
    true
}

/// Les boutons de la barre : bascule le zoom (`zoom`) ou le projecteur.
#[tauri::command]
pub async fn video_effet(app: tauri::AppHandle, zoom: bool) {
    let direct = tenir(&app.state::<EtatVideo>().en_cours).as_ref().and_then(|e| e.direct.clone());
    if let Some(d) = direct {
        let mut d = tenir(&d);
        if zoom {
            d.basculer_zoom();
        } else {
            d.basculer_projecteur();
        }
    }
    dire_etat(&app);
}

/// La ligne de la barre : sources absentes, perdues, notes de la camera, puis le plafond.
fn avertissement_de(absentes: &[String], perdues: Vec<String>, notes_camera: &[String], actif_ms: u64) -> Option<String> {
    let mut avertissement = absentes.iter().cloned().chain(perdues).chain(notes_camera.iter().cloned()).collect::<Vec<_>>().join(", ");
    if actif_ms >= PREVENIR_MS {
        if !avertissement.is_empty() {
            avertissement.push_str(". ");
        }
        avertissement.push_str("Arrêt automatique à une heure");
    }
    (!avertissement.is_empty()).then(|| capitaliser(&avertissement))
}

fn capitaliser(texte: &str) -> String {
    let mut lettres = texte.chars();
    lettres.next().map(|p| p.to_uppercase().chain(lettres).collect()).unwrap_or_default()
}

/// Vrai si un enregistrement (ou son decompte) est en cours.
pub fn en_cours(app: &tauri::AppHandle) -> bool {
    tenir(&app.state::<EtatVideo>().en_cours).is_some()
}

/// Le raccourci « Filmer » pendant un enregistrement : il s'arrete.
pub fn arreter_detache(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(motif) = arreter(app.clone()).await {
            captures::avertir(&app, &motif);
        }
    });
}

fn reglages_de(conn: &rusqlite::Connection) -> Reglages {
    db::reglage(conn, CLE_REGLAGES).ok().and_then(|brut| serde_json::from_str(&brut).ok()).unwrap_or_default()
}

fn lire_reglages(app: &tauri::AppHandle) -> Reglages {
    app.state::<AppState>().with_conn(|conn| Ok(reglages_de(conn))).unwrap_or_default()
}

/// La seule ecriture de `video_reglages` : lire, modifier, ecrire sous le verrou de
/// la base, que le panneau, le glisser de la bulle et les scenes se partagent.
fn modifier_dans(etat: &AppState, f: impl FnOnce(&mut Reglages)) -> Result<Reglages, String> {
    etat.with_conn(|conn| {
        let mut r = reglages_de(conn);
        f(&mut r);
        let brut = serde_json::to_string(&r).map_err(|e| e.to_string())?;
        db::regler(conn, CLE_REGLAGES, &brut)?;
        Ok(r)
    })
}

/// Bloquant : hors du thread principal.
fn modifier_reglages(app: &tauri::AppHandle, f: impl FnOnce(&mut Reglages)) -> Result<Reglages, String> {
    modifier_dans(&app.state::<AppState>(), f)
}

/// Le fichier d'une video neuve : `AAAA-MM/AAAAMMJJ-HHMMSS-mmm.mp4` sous le dossier des videos.
fn sortie(app: &tauri::AppHandle, quand: &chrono::DateTime<chrono::Local>) -> Result<PathBuf, String> {
    sortie_sous(&captures::dossier_videos(app)?, quand)
}

/// Rien ne s'ecrit ici : le dossier du mois se cree au lancement (`Enregistrement::demarrer`).
fn sortie_sous(dossier: &std::path::Path, quand: &chrono::DateTime<chrono::Local>) -> Result<PathBuf, String> {
    Ok(dossier.join(stockage::nom_fichier(quand)).with_extension("mp4"))
}

/// Ou poser la barre (pixels physiques du bureau) : sous la zone, au-dessus si
/// le bas de l'ecran manque, dans la zone sinon ; en bas au centre sans zone.
/// Centree sur la zone et gardee dans l'ecran.
fn placer_barre(zone: Option<(i32, i32, u32, u32)>, ecran: ((i32, i32), (u32, u32)), barre: (u32, u32)) -> (i32, i32) {
    let ((ex, ey), (el, eh)) = ecran;
    let (bl, bh) = (barre.0 as i32, barre.1 as i32);
    let ecart = 8 * bh / 52;
    let (x, y) = match zone {
        None => (ex + (el as i32 - bl) / 2, ey + eh as i32 - bh - 6 * ecart),
        Some((zx, zy, zl, zh)) => {
            let x = zx + (zl as i32 - bl) / 2;
            let dessous = zy + zh as i32 + ecart;
            let dessus = zy - ecart - bh;
            let y = if dessous + bh <= ey + eh as i32 {
                dessous
            } else if dessus >= ey {
                dessus
            } else {
                zy + zh as i32 - ecart - bh
            };
            (x, y)
        }
    };
    (x.clamp(ex, ex + el as i32 - bl), y.clamp(ey, ey + eh as i32 - bh))
}

/// Ouvre la barre en panneau de mise en scene, focalisable (Entree, Echap), et lui
/// donne le premier plan tant que la selection l'a encore : apres, Windows ne le
/// donnerait plus. Il est rendu au lancement.
/// ponytail: montree par tao (`show`), pas par `montrer_sans_activer` : `set_focus`
/// ne fait rien sur une fenetre que tao croit cachee (tao 0.35.3, `window.rs:175-186`).
fn ouvrir_barre(app: &tauri::AppHandle, place: (i32, i32)) -> Result<(), String> {
    let fenetre = WebviewWindowBuilder::new(app, ETIQUETTE_BARRE, WebviewUrl::App("enregistrement.html".into()))
        .title("EquiShot, enregistrement")
        .inner_size(PANNEAU.0, PANNEAU.1)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .content_protected(crate::db::fenetres_protegees())
        .visible(false)
        .build()
        .map_err(|e| format!("Barre d'enregistrement impossible à ouvrir : {e}"))?;
    fenetres::sans_menu_alt(&fenetre);
    fenetres::hors_occlusion(&fenetre);
    fenetre
        .set_position(tauri::PhysicalPosition::new(place.0, place.1))
        .map_err(|e| format!("Barre d'enregistrement impossible à placer : {e}"))?;
    fenetre.show().and_then(|_| fenetre.set_focus()).map_err(|e| format!("Barre d'enregistrement impossible à montrer : {e}"))
}

fn fermer_barre(app: &tauri::AppHandle) {
    if let Some(fenetre) = app.get_webview_window(ETIQUETTE_BARRE) {
        fenetres::avant_fermeture(&fenetre);
        let _ = fenetre.close();
    }
}

/// Ferme le contour d'une zone filmee, s'il est encore la, sans toucher au
/// premier plan : il a ete rendu au depart, et l'on travaille ailleurs depuis.
fn fermer_selection(app: &tauri::AppHandle) {
    if let Some(selection) = app.get_webview_window(captures::ETIQUETTE_SELECTION) {
        fenetres::avant_fermeture(&selection);
        let _ = selection.close();
    }
}

fn dire_etat(app: &tauri::AppHandle) {
    let etat = tenir(&app.state::<EtatVideo>().en_cours).as_ref().map(EnCours::etat).unwrap_or(EtatBarre { phase: "aucun", ..Default::default() });
    let _ = app.emit_to(ETIQUETTE_BARRE, EVENEMENT_ETAT, etat);
}

/// Ce qu'une mise en scene filme : le coin et la taille de l'ecran (pixels physiques
/// du bureau), la zone relative a l'ecran (`None` : l'ecran entier), la fiche, et si
/// la selection ouverte en est le contour.
pub(crate) struct Visee {
    pub position: (i32, i32),
    pub taille: (u32, u32),
    pub zone: Option<(u32, u32, u32, u32)>,
    pub origine: Origine,
    pub depuis_selection: bool,
}

/// La cible d'une mise en scene, pour « Enregistrer comme scène ».
fn cible_de(nom: &str, position: (i32, i32), taille: (u32, u32), zone: Option<(u32, u32, u32, u32)>) -> scenes::Cible {
    let ecran = scenes::Ecran { nom: nom.to_string(), position, taille };
    match zone {
        Some(zone) => scenes::Cible::Zone { ecran, zone },
        None => scenes::Cible::Ecran { ecran },
    }
}

/// Le relachement du trace en mode video, un clic sur une fenetre, ou Entree
/// pour l'ecran entier. La mise en scene s'ouvre : rien n'est ecrit avant
/// « Démarrer » (`video_lancer`).
#[tauri::command]
pub async fn video_demarrer(app: tauri::AppHandle, zone: Option<ZoneCss>, facteur: f64, fenetre: Option<usize>) -> Result<(), String> {
    let (position, taille, physique, origine) = hors_thread_principal(app.clone(), move |app| captures::zone_a_filmer(&app, zone, facteur, fenetre)).await?;
    let visee = Visee { position, taille, zone: physique.map(|z| (z.x, z.y, z.largeur, z.hauteur)), origine, depuis_selection: true };
    let devant = captures::devant(&app);
    ouvrir_mise_en_scene(app, visee, devant, Vec::new()).await
}

/// Ouvre la mise en scene sur `visee`, depuis la selection ou depuis une scene a
/// cible fixe (sans selection, donc sans contour). Rien n'est ecrit avant « Démarrer ».
async fn ouvrir_mise_en_scene(app: tauri::AppHandle, visee: Visee, devant: isize, notes_scene: Vec<String>) -> Result<(), String> {
    let Visee { position, taille, zone: voulue, origine, depuis_selection } = visee;
    let zone = match voulue {
        Some((x, y, l, h)) => frise::zone_paire(x as i64, y as i64, l as i64, h as i64, taille).ok_or("La zone est trop petite pour une vidéo (16 x 16 au moins).")?,
        None => frise::zone_paire(0, 0, taille.0 as i64, taille.1 as i64, taille).ok_or("Écran illisible.")?,
    };
    let cible = cible_de(&origine.ecran, position, taille, voulue.map(|_| zone));
    let quand = chrono::Local::now();
    let (sortie_mp4, reglages) = hors_thread_principal(app.clone(), move |app| Ok((sortie(&app, &quand)?, lire_reglages(&app)))).await?;
    let echelle = origine.echelle;
    let en_px = |t: (f64, f64)| ((t.0 * echelle).round() as u32, (t.1 * echelle).round() as u32);
    let sur_le_bureau = voulue.map(|_| (position.0 + zone.0 as i32, position.1 + zone.1 as i32, zone.2, zone.3));
    let places = (placer_barre(sur_le_bureau, (position, taille), en_px(PANNEAU)), placer_barre(sur_le_bureau, (position, taille), en_px(BARRE)));
    let zone_bureau: Rect = sur_le_bureau.map(|z| (z.0, z.1, z.2 as i32, z.3 as i32)).unwrap_or((position.0, position.1, taille.0 as i32, taille.1 as i32));
    let reglages_lus = reglages.clone();
    let etat = app.state::<EtatVideo>();
    let numero = etat.dernier.fetch_add(1, Ordering::Relaxed) + 1;
    {
        let mut en_cours = tenir(&etat.en_cours);
        if en_cours.is_some() {
            return Err("Un enregistrement vidéo est déjà en cours.".to_string());
        }
        *en_cours = Some(EnCours {
            numero,
            enreg: None,
            reste: 3,
            sortie: sortie_mp4.clone(),
            quand,
            origine: origine.clone(),
            absentes: Vec::new(),
            ecoute: Some(ecoute::Ecoute::lancer(&reglages)),
            demande: Some(Demande { ecran: position, zone, sortie: sortie_mp4, reglages, effets: None }),
            places,
            devant,
            zone_bureau,
            echelle,
            bulle: None,
            cameras: Vec::new(),
            notes_camera: Vec::new(),
            cible,
            notes_scene,
            direct: None,
            raccourcis: Vec::new(),
            raccourcis_lisibles: raccourcis_lisibles(app.state::<AppState>().dev),
        });
    }
    // Une zone tracee garde son contour, traverse par la souris ; l'ecran entier ferme la
    // selection. Avant le panneau pour le contour (le `SW_SHOW` de tao l'activerait
    // apres lui), apres pour la fermeture (le premier plan ne doit pas filer ailleurs).
    // ponytail: une scene a zone fixe n'a pas de contour (la selection est sautee, c'est elle qui le dessine).
    let contour = if depuis_selection && voulue.is_some() { app.get_webview_window(captures::ETIQUETTE_SELECTION) } else { None };
    if let Some(selection) = &contour {
        let _ = selection.set_ignore_cursor_events(true);
    }
    if let Err(motif) = ouvrir_barre(&app, places.0) {
        // L'ecoute lachee s'arrete seule (Drop).
        *tenir(&etat.en_cours) = None;
        fermer_barre(&app);
        if let Some(selection) = &contour {
            let _ = selection.set_ignore_cursor_events(false);
        }
        return Err(motif);
    }
    if depuis_selection && contour.is_none() {
        fermer_selection(&app);
    }
    if reglages_lus.camera {
        ouvrir_bulle(&app, zone_bureau, echelle, &reglages_lus);
    }
    let fil = app.clone();
    std::thread::spawn(move || preparer(fil, numero));
    Ok(())
}

/// Les ecrans branches, tels que Tauri (tao) les nomme : `\\.\DISPLAYn` (`szDevice`),
/// le meme nom que `Origine.ecran`. Bloquant : hors du thread principal.
pub fn ecrans_branches(app: &tauri::AppHandle) -> Result<Vec<scenes::EcranVu>, String> {
    let principal = app.primary_monitor().ok().flatten().and_then(|m| m.name().cloned());
    let ecrans = app.available_monitors().map_err(|e| format!("Écrans illisibles : {e}"))?;
    Ok(ecrans
        .iter()
        .map(|m| scenes::EcranVu {
            nom: m.name().cloned().unwrap_or_default(),
            position: (m.position().x, m.position().y),
            taille: (m.size().width, m.size().height),
            echelle: m.scale_factor(),
            principal: principal.is_some() && m.name() == principal.as_ref(),
        })
        .collect())
}

/// Les ecrans branches, pour la sonde et le parcours.
#[tauri::command]
pub async fn video_ecrans(app: tauri::AppHandle) -> Result<Vec<scenes::EcranVu>, String> {
    hors_thread_principal(app, |app| ecrans_branches(&app)).await
}

/// L'ecran et la zone que filme une cible fixe, et la ligne de repli. Bloquant.
fn viser_cible(app: &tauri::AppHandle, cible: &scenes::Cible) -> Result<(scenes::EcranVu, Option<(u32, u32, u32, u32)>, Option<&'static str>), String> {
    let ecrans = ecrans_branches(app)?;
    let (voulu, zone) = match cible {
        scenes::Cible::Ecran { ecran } => (ecran, None),
        scenes::Cible::Zone { ecran, zone } => (ecran, Some(*zone)),
        scenes::Cible::ATracer => return Err("Cette scène se trace : elle passe par la sélection.".to_string()),
    };
    scenes::viser(voulu, zone, &ecrans).ok_or_else(|| "Aucun écran branché.".to_string())
}

/// Une cible fixe : l'ecran retrouve (ou le principal, dit dans le panneau) et sa
/// zone, sans selection. Ecrans et fiche se lisent hors du thread principal.
async fn lancer_cible_fixe(app: tauri::AppHandle, cible: scenes::Cible, devant: isize, nom: String) -> Result<(), String> {
    let (vu, zone, repli, fenetre, application) = hors_thread_principal(app.clone(), move |app| {
        let (vu, zone, repli) = viser_cible(&app, &cible)?;
        let (fenetre, application) = captures::fenetre_au_premier_plan(&app, devant);
        Ok((vu, zone, repli, fenetre, application))
    })
    .await?;
    let origine = Origine { ecran: vu.nom.clone(), echelle: vu.echelle, fenetre, application };
    let mut notes = vec![format!("Scène « {nom} ».")];
    notes.extend(repli.map(str::to_string));
    let visee = Visee { position: vu.position, taille: vu.taille, zone, origine, depuis_selection: false };
    ouvrir_mise_en_scene(app, visee, devant, notes).await
}

/// `close()` est asynchrone : l'etiquette reste prise jusqu'a la destruction, et
/// rouvrir avant echouerait (`build()`). Bloquant, 2 s au plus.
fn attendre_fermees(app: &tauri::AppHandle, etiquettes: &[&str]) -> bool {
    let fin = Instant::now() + Duration::from_secs(2);
    while etiquettes.iter().any(|e| app.get_webview_window(e).is_some()) {
        if Instant::now() >= fin {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

/// Ouvre la mise en scene sur une cible fixe. Une mise en scene deja ouverte est
/// lachee d'abord, sans rendre le premier plan, et son `devant` est repris.
async fn reviser(app: tauri::AppHandle, cible: scenes::Cible, nom: String, devant: isize) -> Result<(), String> {
    let ouverte = {
        let etat = app.state::<EtatVideo>();
        let mut en_cours = tenir(&etat.en_cours);
        if en_cours.as_ref().is_some_and(|e| e.demande.is_some()) { en_cours.take() } else { None }
    };
    let devant = match ouverte {
        Some(en) => {
            let devant = en.devant;
            lacher(app.clone(), en.ecoute, None).await?;
            hors_thread_principal(app.clone(), |app| {
                attendre_fermees(&app, &[ETIQUETTE_BARRE, bulle::ETIQUETTE]).then_some(()).ok_or_else(|| "Le panneau précédent ne s'est pas fermé.".to_string())
            })
            .await?;
            devant
        }
        None => devant,
    };
    lancer_cible_fixe(app, cible, devant, nom).await
}

/// Sonde seulement (exe de debug, instance dev) : une cible fixe sans scene en base,
/// comme une scene choisie au menu ; une mise en scene ouverte est rouverte sur elle.
// ponytail: une couture de test, comme `lecture_ratee_pour_le_parcours`.
#[tauri::command]
pub async fn video_sonde_viser(app: tauri::AppHandle, cible: scenes::Cible) -> Result<(), String> {
    let permis = cfg!(debug_assertions) && app.state::<AppState>().dev;
    if !permis {
        return Err("Réservé à la sonde.".to_string());
    }
    reviser(app, cible, "sonde".to_string(), crate::fenetres::premier_plan()).await
}

/// Emis a toutes les pages quand les scenes changent : panneau et reglages relisent.
pub const EVENEMENT_SCENES: &str = "video://scenes";

fn maintenant() -> String {
    chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// Les scenes lisibles, `(id, nom)`, pour les menus « Filmer ». Bloquant.
pub fn noms_des_scenes(app: &tauri::AppHandle) -> Result<Vec<(i64, String)>, String> {
    app.state::<AppState>().with_conn(scenes::lister).map(|l| l.scenes.into_iter().map(|s| (s.id, s.nom)).collect())
}

/// Le menu de l'icone suit la base, les pages relisent. Bloquant (le menu attend le
/// thread principal) : appele hors de lui, base relachee.
fn scenes_changees(app: &tauri::AppHandle) {
    if let Err(motif) = crate::pastille::rafraichir_filmer(app) {
        eprintln!("{motif}");
    }
    let _ = app.emit(EVENEMENT_SCENES, ());
}

/// Son et bulle d'une scene, place comprise : par la seule ecriture, jamais par
/// `video_regler` (qui garde la place en base).
fn appliquer_mise(app: &tauri::AppHandle, mise: &scenes::Mise) -> Result<Reglages, String> {
    modifier_reglages(app, |r| *r = mise.appliquer(r))
}

/// Une scene appliquee en mise en scene : la bulle ouverte va a la place de la scene.
/// `set_position` seulement (aucun drapeau de tao).
fn replacer_bulle(app: &tauri::AppHandle, r: &Reglages) {
    let place = {
        let etat = app.state::<EtatVideo>();
        let mut en_cours = tenir(&etat.en_cours);
        let Some(e) = en_cours.as_mut() else { return };
        let (zone, echelle) = (e.zone_bureau, e.echelle);
        let Some(b) = e.bulle.as_mut() else { return };
        b.pos = bulle::place_initiale(zone, b.diametre, r.camera_place, echelle);
        b.pos
    };
    if let Some(fenetre) = app.get_webview_window(bulle::ETIQUETTE) {
        let _ = fenetre.set_position(tauri::PhysicalPosition::new(place.0, place.1));
    }
}

/// La cible de la mise en scene ouverte, `None` hors de la phase `prepare`.
fn cible_ouverte(app: &tauri::AppHandle) -> Option<scenes::Cible> {
    tenir(&app.state::<EtatVideo>().en_cours).as_ref().filter(|e| e.demande.is_some()).map(|e| e.cible.clone())
}

#[tauri::command]
pub async fn video_scenes(app: tauri::AppHandle) -> Result<scenes::Liste, String> {
    hors_thread_principal(app, |app| app.state::<AppState>().with_conn(scenes::lister)).await
}

/// « Enregistrer comme scène » du panneau : la cible de la mise en scene ouverte (ou
/// « à tracer »), le son et la bulle tels qu'ecrits. Le panneau attend ses ecritures avant.
#[tauri::command]
pub async fn video_scene_enregistrer(app: tauri::AppHandle, nom: String, a_tracer: bool, remplacer: bool) -> Result<scenes::Ecrite, String> {
    let actuelle = cible_ouverte(&app).ok_or("Aucune mise en scène ouverte.")?;
    let cible = if a_tracer { scenes::Cible::ATracer } else { actuelle };
    hors_thread_principal(app, move |app| {
        let donnees = scenes::Donnees { cible, mise: scenes::Mise::de(&lire_reglages(&app)) };
        let ecrite = app.state::<AppState>().with_conn(|conn| scenes::enregistrer(conn, &nom, &donnees, remplacer, &maintenant()))?;
        if ecrite != scenes::Ecrite::NomPris {
            scenes_changees(&app);
        }
        Ok(ecrite)
    })
    .await
}

#[tauri::command]
pub async fn video_scene_renommer(app: tauri::AppHandle, id: i64, nom: String) -> Result<(), String> {
    hors_thread_principal(app, move |app| {
        app.state::<AppState>().with_conn(|conn| scenes::renommer(conn, id, &nom, &maintenant()))?;
        scenes_changees(&app);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn video_scene_supprimer(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    hors_thread_principal(app, move |app| {
        app.state::<AppState>().with_conn(|conn| scenes::supprimer(conn, id))?;
        scenes_changees(&app);
        Ok(())
    })
    .await
}

/// La liste « Scène » du panneau : son et bulle s'appliquent tout de suite ; une
/// cible fixe autre que celle de la mise en scene la rouvre sur elle (le panneau se
/// ferme et revient). « À tracer » garde la zone deja tracee.
#[tauri::command]
pub async fn video_scene_appliquer(app: tauri::AppHandle, id: i64) -> Result<Reglages, String> {
    let scene = hors_thread_principal(app.clone(), move |app| app.state::<AppState>().with_conn(|conn| scenes::lire(conn, id))).await?;
    let actuelle = cible_ouverte(&app).ok_or("Aucune mise en scène ouverte.")?;
    let mise = scene.mise.clone();
    let r = hors_thread_principal(app.clone(), move |app| appliquer_mise(&app, &mise)).await?;
    // Comparee une fois visee : un ecran deplace ou un repli ne rouvre pas a chaque fois.
    let (rouvrir, repli) = match &scene.cible {
        scenes::Cible::ATracer => (false, None),
        fixe => {
            let fixe = fixe.clone();
            let (vu, zone, repli) = hors_thread_principal(app.clone(), move |app| viser_cible(&app, &fixe)).await?;
            (cible_de(&vu.nom, vu.position, vu.taille, zone) != actuelle, repli)
        }
    };
    if rouvrir {
        // Le panneau qui a demande est deja ferme : l'erreur se dit ici.
        if let Err(motif) = reviser(app.clone(), scene.cible, scene.nom, 0).await {
            captures::avertir(&app, &motif);
            return Err(motif);
        }
        return Ok(r);
    }
    let (nom, suivis) = (scene.nom, r.clone());
    hors_thread_principal(app.clone(), move |app| {
        reecouter(&app, &suivis);
        appliquer_bulle(&app, &suivis);
        replacer_bulle(&app, &suivis);
        if let Some(e) = tenir(&app.state::<EtatVideo>().en_cours).as_mut() {
            e.notes_scene = std::iter::once(format!("Scène « {nom} »."))
                .chain(repli.map(str::to_string))
                .collect();
        }
        Ok(())
    })
    .await?;
    dire_etat(&app);
    Ok(r)
}

/// Une scene choisie dans un menu « Filmer » : ses reglages s'ecrivent, puis la mise
/// en scene s'ouvre sur sa cible fixe, ou la selection si elle est « à tracer ».
pub async fn lancer_scene(app: tauri::AppHandle, id: i64, devant: isize) -> Result<(), String> {
    if en_cours(&app) {
        return Err("Un enregistrement vidéo est déjà en cours.".to_string());
    }
    let scene = hors_thread_principal(app.clone(), move |app| app.state::<AppState>().with_conn(|conn| scenes::lire(conn, id))).await?;
    let mise = scene.mise.clone();
    hors_thread_principal(app.clone(), move |app| appliquer_mise(&app, &mise).map(|_| ())).await?;
    match scene.cible {
        scenes::Cible::ATracer => captures::ouvrir_selection_devant(app, captures::Mode::Video, devant).await,
        fixe => {
            // Une selection ouverte (image, texte) se ferme : son premier plan d'avant est le bon.
            let devant = match captures::devant(&app) {
                0 => devant,
                d => d,
            };
            if app.get_webview_window(captures::ETIQUETTE_SELECTION).is_some() {
                captures::fermer_selection(&app);
            }
            reviser(app, fixe, scene.nom, devant).await
        }
    }
}

pub fn lancer_scene_detache(app: &tauri::AppHandle, id: i64, devant: isize) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(motif) = lancer_scene(app.clone(), id, devant).await {
            captures::avertir(&app, &motif);
        }
    });
}

/// Vrai pendant la mise en scene de l'enregistrement `numero`.
fn en_preparation(app: &tauri::AppHandle, numero: u64) -> bool {
    tenir(&app.state::<EtatVideo>().en_cours).as_ref().is_some_and(|e| e.numero == numero && e.demande.is_some())
}

/// Les jauges vers le panneau, 15 fois par seconde, tant que dure la mise en scene.
fn preparer(app: tauri::AppHandle, numero: u64) {
    while en_preparation(&app, numero) {
        dire_etat(&app);
        std::thread::sleep(Duration::from_millis(66));
    }
}

/// « Démarrer » ou Entree : l'ecoute s'arrete, le panneau redevient la barre
/// compacte et non focalisable, le premier plan est rendu, le decompte part.
#[tauri::command]
pub async fn video_lancer(app: tauri::AppHandle) -> Result<(), String> {
    let etat = app.state::<EtatVideo>();
    let (numero, ecoute, demande, place_barre, devant) = {
        let mut en_cours = tenir(&etat.en_cours);
        let e = en_cours.as_mut().ok_or("Rien à lancer.")?;
        let demande = e.demande.take().ok_or("Rien à lancer.")?;
        (e.numero, e.ecoute.take(), demande, e.places.1, e.devant)
    };
    let mut demande = demande;
    // Les choix du panneau sont deja ecrits (`video_regler`) : on les relit.
    demande.reglages = hors_thread_principal(app.clone(), move |app| {
        if let Some(ecoute) = ecoute {
            ecoute.arreter();
        }
        Ok(lire_reglages(&app))
    })
    .await?;
    if let Some(barre) = app.get_webview_window(ETIQUETTE_BARRE) {
        fenetres::non_activable(&barre);
        let _ = barre.set_size(tauri::LogicalSize::new(BARRE.0, BARRE.1));
        let _ = barre.set_position(tauri::PhysicalPosition::new(place_barre.0, place_barre.1));
    }
    fenetres::rendre_premier_plan(devant);
    let fil = app.clone();
    std::thread::spawn(move || decompter_puis_filmer(fil, numero, demande));
    Ok(())
}

/// La barre pendant l'enregistrement, vue du curseur.
#[derive(Clone, Copy, Debug, PartialEq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Survol {
    #[default]
    Normale,
    /// A 10 % et traversante : on voit et on clique ce qui est dessous.
    Effacee,
    /// La poignee survolee : la barre entiere, jusqu'a ce que le curseur sorte.
    Rendue,
}

fn dans(p: (i32, i32), r: Rect) -> bool {
    p.0 >= r.0 && p.0 < r.0 + r.2 && p.1 >= r.1 && p.1 < r.1 + r.3
}

/// La barre pendant l'enregistrement : effacee et traversante sous le curseur,
/// sauf sur la poignee, qui la rend jusqu'a ce que le curseur sorte.
fn survol(avant: Survol, curseur: (i32, i32), barre: Rect, poignee: Rect) -> Survol {
    match (dans(curseur, barre), dans(curseur, poignee), avant) {
        (false, _, _) => Survol::Normale,
        (true, true, _) => Survol::Rendue,
        (true, false, Survol::Rendue) => Survol::Rendue,
        (true, false, _) => Survol::Effacee,
    }
}

/// Un tour de survol : au changement seulement, la barre devient traversante
/// ou non, et la page l'apprend.
// ponytail: curseur lu 20 fois par seconde, un crochet souris global si le cout se voit.
fn suivre_survol(app: &tauri::AppHandle, avant: Survol) -> Survol {
    let Some(barre) = app.get_webview_window(ETIQUETTE_BARRE) else { return avant };
    let (Some(curseur), Ok(pos), Ok(taille), Ok(echelle)) = (fenetres::curseur(), barre.outer_position(), barre.outer_size(), barre.scale_factor()) else {
        return avant;
    };
    let rect = (pos.x, pos.y, taille.width as i32, taille.height as i32);
    let poignee = (pos.x, pos.y, (POIGNEE_CSS * echelle).round() as i32, taille.height as i32);
    let nouveau = survol(avant, curseur, rect, poignee);
    if nouveau != avant {
        fenetres::traversante(&barre, nouveau == Survol::Effacee);
        let _ = app.emit_to(ETIQUETTE_BARRE, EVENEMENT_SURVOL, nouveau);
    }
    nouveau
}

/// Le fil d'un enregistrement : 3, 2, 1, puis le moteur, puis l'etat de la
/// barre et son survol toutes les 50 ms, et le plafond d'une heure.
fn decompter_puis_filmer(app: tauri::AppHandle, numero: u64, demande: Demande) {
    for reste in (1..=3).rev() {
        if let Some(e) = tenir(&app.state::<EtatVideo>().en_cours).as_mut().filter(|e| e.numero == numero) {
            e.reste = reste;
        } else {
            return;
        }
        dire_etat(&app);
        std::thread::sleep(Duration::from_secs(1));
    }
    let lu = tenir(&app.state::<EtatVideo>().en_cours).as_ref().filter(|e| e.numero == numero).map(|e| ((e.zone_bureau.0, e.zone_bureau.1), e.echelle));
    let Some((coin, echelle)) = lu else { return };
    let mut demande = demande;
    let r = &demande.reglages;
    let options = animation::Options { onde: r.onde, trainee: r.trainee, echelle: echelle as f32 };
    let (direct, note_souris) = direct::Direct::lancer(options, coin, r.curseur_agrandi());
    let pour_moteur = direct.clone();
    demande.effets = Some(std::sync::Arc::new(move |quand| tenir(&pour_moteur).scene(quand)));
    let lance = Enregistrement::demarrer(demande);
    let (raccourcis, notes_raccourcis) = if lance.is_ok() { poser_raccourcis_effets(&app) } else { (Vec::new(), Vec::new()) };
    {
        let etat = app.state::<EtatVideo>();
        let mut en_cours = tenir(&etat.en_cours);
        match (en_cours.as_mut().filter(|e| e.numero == numero), lance) {
            (Some(e), Ok((enreg, absentes))) => {
                e.enreg = Some(enreg);
                e.absentes = absentes;
                e.absentes.extend(note_souris);
                e.absentes.extend(notes_raccourcis);
                e.direct = Some(direct);
                e.raccourcis = raccourcis;
            }
            // Abandonne pendant l'ouverture du moteur : rien n'est garde.
            (None, Ok((enreg, _))) => {
                drop(en_cours);
                retirer_raccourcis_effets(&app, &raccourcis);
                enreg.abandonner();
                return;
            }
            (Some(_), Err(motif)) => {
                *en_cours = None;
                drop(en_cours);
                fermer_barre(&app);
                bulle::fermer(&app);
                fermer_selection(&app);
                captures::avertir(&app, &motif);
                return;
            }
            (None, Err(_)) => return,
        }
    }
    let mut etat_survol = Survol::Normale;
    loop {
        dire_etat(&app);
        std::thread::sleep(Duration::from_millis(50));
        etat_survol = suivre_survol(&app, etat_survol);
        let (actif, erreur) = match tenir(&app.state::<EtatVideo>().en_cours).as_ref().filter(|e| e.numero == numero) {
            Some(e) => e.enreg.as_ref().map_or((0, None), |enreg| (enreg.actif_ms(), enreg.erreur())),
            None => return,
        };
        // Une erreur du moteur, ou le plafond : on arrete et on garde ce qui est filme.
        if erreur.is_some() || actif >= PLAFOND_MS {
            arreter_detache(&app);
            return;
        }
    }
}

/// Arrete : pendant le decompte, rien n'est garde ; sinon le MP4 est fini,
/// range et copie.
async fn arreter(app: tauri::AppHandle) -> Result<(), String> {
    let Some(en) = tenir(&app.state::<EtatVideo>().en_cours).take() else { return Ok(()) };
    let Some(enreg) = en.enreg else {
        // Decompte ou mise en scene : rien n'est garde.
        return lacher(app, en.ecoute, en.demande.is_some().then_some(en.devant)).await;
    };
    fermer_barre(&app);
    bulle::fermer(&app);
    retirer_raccourcis_effets(&app, &en.raccourcis);
    let (sortie, quand, origine, direct) = (en.sortie, en.quand, en.origine, en.direct);
    let range = hors_thread_principal(app.clone(), move |app| {
        let horloge = enreg.horloge();
        let souris = direct.map(|d| tenir(&d).journal());
        let bilan = enreg.arreter()?;
        if let Some((depart, evenements)) = souris {
            ecrire_journal(&sortie, &bilan, depart, &evenements, horloge);
        }
        ranger(&app, bilan, &sortie, &quand, &origine)
    })
    .await;
    fermer_selection(&app);
    let (capture, copie) = range?;
    crate::vignettes::ajouter(&app, capture);
    captures::annoncer(&app);
    match copie {
        Ok(()) => captures::informer(&app, "Vidéo enregistrée et copiée comme fichier"),
        Err(motif) => captures::avertir(&app, &format!("Vidéo enregistrée, mais pas copiée : {motif}")),
    }
    Ok(())
}

/// Le journal de la souris a cote du MP4 (ADR-022). Rien ne le lit encore : un echec
/// ne coute pas la video.
// ponytail: echec d'ecriture tu, a dire quand le montage (voie B) le lira.
fn ecrire_journal(
    sortie: &std::path::Path,
    bilan: &moteur::Bilan,
    depart: std::time::Instant,
    evenements: &[animation::Evenement],
    horloge: impl Fn(std::time::Instant) -> Option<f64>,
) {
    let duree = bilan.duree_ms as f64 / 1000.0;
    let actif = |t: f64| horloge(depart + Duration::from_secs_f64(t)).filter(|&a| a <= duree);
    let journal = animation::journal(evenements, (bilan.largeur, bilan.hauteur), actif);
    let _ = std::fs::write(sortie.with_extension("souris.json"), journal.to_string());
}

/// Vignette, ligne de galerie, puis copie du fichier. La ligne qui ne s'ecrit
/// pas emporte la video : sans elle, rien ne la montrerait.
fn ranger(
    app: &tauri::AppHandle,
    bilan: moteur::Bilan,
    sortie: &std::path::Path,
    quand: &chrono::DateTime<chrono::Local>,
    origine: &Origine,
) -> Result<(Capture, Result<(), String>), String> {
    let dossier = captures::dossier_videos(app)?;
    let fichier = sortie.to_string_lossy().into_owned();
    let premiere = bilan.premiere.unwrap_or_else(|| image::RgbaImage::from_pixel(bilan.largeur, bilan.hauteur, image::Rgba([0, 0, 0, 255])));
    let etat = app.state::<AppState>();
    let ligne = stockage::ecrire_rendu(&dossier, &fichier, &premiere)
        .and_then(|_| etat.with_conn(|conn| stockage::inserer_video(conn, quand, &fichier, (bilan.largeur, bilan.hauteur), origine, bilan.duree_ms)));
    let id = match ligne {
        Ok(id) => id,
        Err(motif) => {
            stockage::effacer_fichiers(&dossier, &fichier);
            return Err(motif);
        }
    };
    let capture = etat.with_conn(|conn| stockage::lire(conn, &dossier, id))?;
    Ok((capture, captures::copier_fichier(sortie)))
}

#[tauri::command]
pub async fn video_arreter(app: tauri::AppHandle) -> Result<(), String> {
    arreter(app).await
}

/// Abandonner : rien n'est garde, le fichier partiel est efface.
#[tauri::command]
pub async fn video_abandonner(app: tauri::AppHandle) -> Result<(), String> {
    let Some(en) = tenir(&app.state::<EtatVideo>().en_cours).take() else { return Ok(()) };
    let Some(enreg) = en.enreg else {
        return lacher(app, en.ecoute, en.demande.is_some().then_some(en.devant)).await;
    };
    fermer_barre(&app);
    bulle::fermer(&app);
    fermer_selection(&app);
    retirer_raccourcis_effets(&app, &en.raccourcis);
    hors_thread_principal(app, move |_| {
        enreg.abandonner();
        Ok(())
    })
    .await
}

/// Ferme tout avant que rien ne soit filme : en mise en scene, le premier plan
/// est rendu (`devant`) et l'ecoute arretee, fil joint. Aucun fichier n'existe encore.
async fn lacher(app: tauri::AppHandle, ecoute: Option<ecoute::Ecoute>, devant: Option<isize>) -> Result<(), String> {
    if let Some(devant) = devant {
        fenetres::rendre_premier_plan(devant);
    }
    fermer_barre(&app);
    bulle::fermer(&app);
    fermer_selection(&app);
    if let Some(ecoute) = ecoute {
        hors_thread_principal(app, move |_| {
            ecoute.arreter();
            Ok(())
        })
        .await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn video_pauser(app: tauri::AppHandle, pause: bool) {
    if let Some(enreg) = tenir(&app.state::<EtatVideo>().en_cours).as_ref().and_then(|e| e.enreg.as_ref()) {
        if pause {
            enreg.pauser();
        } else {
            enreg.reprendre();
        }
    }
    dire_etat(&app);
}

#[tauri::command]
pub async fn video_etat(app: tauri::AppHandle) -> EtatBarre {
    tenir(&app.state::<EtatVideo>().en_cours).as_ref().map(EnCours::etat).unwrap_or(EtatBarre { phase: "aucun", ..Default::default() })
}

#[tauri::command]
pub async fn video_reglages(app: tauri::AppHandle) -> Result<Reglages, String> {
    hors_thread_principal(app, |app| Ok(lire_reglages(&app))).await
}

#[tauri::command]
pub async fn video_regler(app: tauri::AppHandle, reglages: Reglages) -> Result<Reglages, String> {
    hors_thread_principal(app, move |app| {
        let reglages = modifier_reglages(&app, |r| *r = fusionner_place(reglages, r))?;
        reecouter(&app, &reglages);
        appliquer_bulle(&app, &reglages);
        Ok(reglages)
    })
    .await
}

/// Les reglages du panneau, avec la place en base : seul le glisser ecrit la place,
/// et le panneau renverrait celle, perimee, qu'il a lue a l'ouverture. Appelee sous
/// le verrou de `modifier_reglages`.
fn fusionner_place(nouveaux: Reglages, en_base: &Reglages) -> Reglages {
    Reglages { camera_place: en_base.camera_place, ..nouveaux }
}

/// Le glisser de la bulle : `dx`, `dy` en pixels CSS cumules depuis l'appui. Rust
/// borne a la zone, place, et n'ecrit la proportion qu'au relachement.
#[tauri::command]
pub async fn video_bulle_deplacer(app: tauri::AppHandle, dx: f64, dy: f64, fini: bool) -> Result<(), String> {
    let (place, proportion, prepare, devant) = {
        let etat = app.state::<EtatVideo>();
        let mut en_cours = tenir(&etat.en_cours);
        let e = en_cours.as_mut().ok_or("Aucune bulle de caméra.")?;
        let (zone, echelle, prepare, devant) = (e.zone_bureau, e.echelle, e.demande.is_some(), e.devant);
        let b = e.bulle.as_mut().ok_or("Aucune bulle de caméra.")?;
        let depart = *b.depart.get_or_insert(b.pos);
        let voulu = (depart.0 + (dx * echelle).round() as i32, depart.1 + (dy * echelle).round() as i32);
        b.pos = bulle::borner(voulu, b.diametre, zone);
        if fini {
            b.depart = None;
        }
        (b.pos, fini.then(|| bulle::proportion(b.pos, b.diametre, zone)), prepare, devant)
    };
    if let Some(fenetre) = app.get_webview_window(bulle::ETIQUETTE) {
        let _ = fenetre.set_position(tauri::PhysicalPosition::new(place.0, place.1));
        // Le clic sur sa WebView donne le premier plan a la bulle quand il etait a une fenetre
        // d'EquiShot (le panneau) : Entree et Echap n'y arrivaient plus (etape 30 j). Rendu au
        // panneau en mise en scene, sinon a la fenetre filmee.
        if fini && fenetre.hwnd().is_ok_and(|h| h.0 as isize == fenetres::premier_plan()) {
            let panneau = app.get_webview_window(ETIQUETTE_BARRE).and_then(|b| b.hwnd().ok()).map_or(0, |h| h.0 as isize);
            fenetres::rendre_premier_plan(if prepare { panneau } else { devant });
        }
    }
    if let Some(proportion) = proportion {
        hors_thread_principal(app, move |app| modifier_reglages(&app, |r| r.camera_place = Some(proportion)).map(|_| ())).await?;
    }
    Ok(())
}

/// Une note du panneau (zone trop petite, camera perdue) ne s'ecrit qu'une fois.
fn noter(notes: &mut Vec<String>, note: &str) {
    if !notes.iter().any(|n| n == note) {
        notes.push(note.to_string());
    }
}

/// Ouvre la bulle et la garde dans `EnCours` ; une zone trop petite ou un echec
/// devient une note du panneau, jamais une erreur de la mise en scene.
fn ouvrir_bulle(app: &tauri::AppHandle, zone: Rect, echelle: f64, r: &Reglages) {
    let ouverte = bulle::ouvrir(app, zone, echelle, r);
    let etat = app.state::<EtatVideo>();
    let mut en_cours = tenir(&etat.en_cours);
    let Some(e) = en_cours.as_mut() else {
        // Abandonne pendant l'ouverture : la fenetre ne doit pas rester.
        drop(en_cours);
        bulle::fermer(app);
        return;
    };
    match ouverte {
        Ok(Some(b)) => e.bulle = Some(b),
        Ok(None) => noter(&mut e.notes_camera, bulle::ZONE_TROP_PETITE),
        Err(motif) => noter(&mut e.notes_camera, &motif),
    }
}

/// Apres chaque `video_regler` : la bulle suit les choix (ouverte, fermee,
/// redimensionnee a la meme proportion, page prevenue de la camera et du miroir).
/// `set_size` et `set_position` ne touchent aucun drapeau de tao : surs.
fn appliquer_bulle(app: &tauri::AppHandle, r: &Reglages) {
    let etat = app.state::<EtatVideo>();
    let (zone, echelle, actuelle) = {
        let en_cours = tenir(&etat.en_cours);
        let Some(e) = en_cours.as_ref() else { return };
        (e.zone_bureau, e.echelle, e.bulle.as_ref().map(|b| (b.taille, b.diametre, b.pos)))
    };
    match (r.camera, actuelle) {
        (false, None) => {}
        (false, Some(_)) => {
            bulle::fermer(app);
            if let Some(e) = tenir(&etat.en_cours).as_mut() {
                e.bulle = None;
            }
        }
        (true, None) => ouvrir_bulle(app, zone, echelle, r),
        (true, Some((taille, diametre, pos))) => {
            let mut retenue = taille;
            if let Some((neuve, d)) = bulle::taille_possible(r.camera_taille, zone, echelle).filter(|(t, _)| *t != taille) {
                let place = bulle::place_initiale(zone, d, Some(bulle::proportion(pos, diametre, zone)), echelle);
                if let Some(fenetre) = app.get_webview_window(bulle::ETIQUETTE) {
                    let css = neuve.diametre_css();
                    let _ = fenetre.set_size(tauri::LogicalSize::new(css, css));
                    let _ = fenetre.set_position(tauri::PhysicalPosition::new(place.0, place.1));
                }
                if let Some(b) = tenir(&etat.en_cours).as_mut().and_then(|e| e.bulle.as_mut()) {
                    b.taille = neuve;
                    b.diametre = d;
                    b.pos = place;
                }
                retenue = neuve;
            }
            let _ = app.emit_to(bulle::ETIQUETTE, bulle::EVENEMENT_BULLE, bulle::config(r, retenue));
        }
    }
}

/// La page de la bulle demande sa configuration : elle n'allume la camera qu'apres,
/// et seulement quand le gestionnaire d'autorisation est pose (2 s au plus).
#[tauri::command]
pub async fn video_bulle_config(app: tauri::AppHandle) -> Result<bulle::ConfigBulle, String> {
    let (autorisee, taille) = tenir(&app.state::<EtatVideo>().en_cours)
        .as_ref()
        .and_then(|e| e.bulle.as_ref())
        .map(|b| (b.autorisee.clone(), b.taille))
        .ok_or("Aucune bulle de caméra.")?;
    hors_thread_principal(app, move |app| {
        if !bulle::attendre_le_drapeau(&autorisee, Duration::from_secs(2)) {
            return Err("Autorisation de la caméra impossible à poser.".to_string());
        }
        Ok(bulle::config(&lire_reglages(&app), taille))
    })
    .await
}

/// La page a enumere les cameras apres son premier `getUserMedia` : le panneau les liste.
#[tauri::command]
pub async fn video_bulle_cameras(app: tauri::AppHandle, cameras: Vec<bulle::Camera>) {
    if let Some(e) = tenir(&app.state::<EtatVideo>().en_cours).as_mut() {
        e.cameras = cameras;
    }
    dire_etat(&app);
}

/// Camera perdue (piste `ended`) ou jamais ouverte : la bulle se ferme, le panneau
/// ou la barre le dit, l'enregistrement continue.
#[tauri::command]
pub async fn video_bulle_perdue(app: tauri::AppHandle, motif: String) {
    bulle::fermer(&app);
    if let Some(e) = tenir(&app.state::<EtatVideo>().en_cours).as_mut() {
        e.bulle = None;
        noter(&mut e.notes_camera, &motif);
    }
    dire_etat(&app);
}

/// En mise en scene, les jauges suivent les choix : un volume change la mesure,
/// une autre source relance l'ecoute. Bloquant (fil joint) : hors du thread principal.
fn reecouter(app: &tauri::AppHandle, r: &Reglages) {
    let etat = app.state::<EtatVideo>();
    let ancienne = {
        let mut en_cours = tenir(&etat.en_cours);
        let Some(e) = en_cours.as_mut().filter(|e| e.demande.is_some()) else { return };
        match &e.ecoute {
            Some(ecoute) if ecoute.meme_choix(r) => {
                ecoute.regler_volumes(r.volume_micro, r.volume_son);
                return;
            }
            _ => (e.numero, e.ecoute.take()),
        }
    };
    if let Some(ecoute) = ancienne.1 {
        ecoute.arreter();
    }
    let neuve = ecoute::Ecoute::lancer(r);
    let mut en_cours = tenir(&etat.en_cours);
    // Lancee ou abandonnee entre-temps : la neuve est lachee (Drop l'arrete).
    if let Some(e) = en_cours.as_mut().filter(|e| e.numero == ancienne.0 && e.demande.is_some()) {
        e.ecoute = Some(neuve);
    }
}

/// Les micros et sorties branches, pour le panneau de preparation.
#[tauri::command]
pub async fn video_peripheriques(app: tauri::AppHandle) -> Result<peripheriques::Peripheriques, String> {
    hors_thread_principal(app, |_| peripheriques::lister_tout()).await
}

/// Le dossier des videos, pour les reglages.
#[tauri::command]
pub async fn video_dossier(app: tauri::AppHandle) -> Result<String, String> {
    hors_thread_principal(app, |app| Ok(captures::dossier_videos(&app)?.to_string_lossy().into_owned())).await
}

/// « Enregistrer en GIF » (0.16) : le GIF s'ecrit a cote du MP4 et part au
/// presse-papiers comme fichier. Rend son chemin.
#[tauri::command]
pub async fn video_gif(app: tauri::AppHandle, id: i64) -> Result<String, String> {
    hors_thread_principal(app, move |app| {
        let capture = captures::lire(&app, id)?;
        if capture.duree_ms.is_none() {
            return Err("Seule une vidéo se change en GIF.".to_string());
        }
        let mp4 = PathBuf::from(&capture.origine);
        let gif = mp4.with_extension("gif");
        gif::depuis_video(&mp4, &gif)?;
        captures::copier_fichier(&gif)?;
        Ok(gif.to_string_lossy().into_owned())
    })
    .await
}

#[tauri::command]
pub async fn video_ouvrir_dossier(app: tauri::AppHandle) -> Result<String, String> {
    hors_thread_principal(app, |app| {
        use tauri_plugin_opener::OpenerExt;
        let dossier = captures::dossier_videos(&app)?;
        app.opener()
            .open_path(dossier.to_string_lossy().to_string(), None::<&str>)
            .map_err(|e| format!("Ouverture du dossier impossible : {e}"))?;
        Ok(dossier.to_string_lossy().into_owned())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    const ECRAN: ((i32, i32), (u32, u32)) = ((0, 0), (1920, 1080));

    #[test]
    fn regler_garde_la_place_en_base() {
        let en_base = Reglages { camera_place: Some((0.4, 0.9)), ..Reglages::default() };
        let du_panneau = Reglages { camera: true, camera_place: Some((0.0, 0.0)), ..Reglages::default() };
        let fusion = fusionner_place(du_panneau, &en_base);
        assert_eq!((fusion.camera, fusion.camera_place), (true, Some((0.4, 0.9))));
        assert_eq!(fusionner_place(Reglages::default(), &Reglages::default()).camera_place, None);
    }

    /// Reserve de la relecture finale de la 0.18 : deux ecritures croisees (glisser,
    /// panneau, scene) ne se perdent plus. A lit, puis attend ; B ecrit pendant ce temps.
    #[test]
    fn deux_ecritures_croisees_ne_se_perdent_pas() {
        use std::sync::{mpsc, Arc};
        let etat = Arc::new(AppState { db: Mutex::new(db::open_in_memory().expect("base")), data_dir: std::env::temp_dir(), dev: true });
        let (parti, attente) = mpsc::channel();
        let a = {
            let etat = etat.clone();
            std::thread::spawn(move || {
                modifier_dans(&etat, |r| {
                    let _ = parti.send(());
                    std::thread::sleep(Duration::from_millis(150));
                    r.camera_place = Some((0.25, 0.75));
                })
            })
        };
        attente.recv().expect("A a lu");
        modifier_dans(&etat, |r| r.volume_son = 0.5).expect("B écrit");
        a.join().expect("A fini").expect("A écrit");
        let fin = etat.with_conn(|conn| Ok(reglages_de(conn))).expect("relus");
        assert_eq!((fin.camera_place, fin.volume_son), (Some((0.25, 0.75)), 0.5));
    }

    #[test]
    fn une_scene_ecrit_sa_place_par_modifier_reglages() {
        let code = include_str!("mod.rs").split("#[cfg(test)]").next().unwrap_or_default();
        // Pas la chaîne entière du mutant M5 : `mutant.mjs` exige qu'elle ne paraisse qu'une fois dans le fichier.
        assert!(code.contains("*r = mise.appliquer(r)"), "la scène écrit tout, place comprise, sous le verrou");
        assert!(!code.contains("fusionner_place(mise"), "jamais par la fusion du panneau");
    }

    #[test]
    fn une_seule_ecriture_des_reglages_video() {
        let code = include_str!("mod.rs").split("#[cfg(test)]").next().unwrap_or_default();
        assert_eq!(code.matches("db::regler(").count(), 1, "seule modifier_dans écrit video_reglages");
        assert!(!code.contains("fn ecrire_reglages"), "l'écriture sans lecture a disparu");
    }

    #[test]
    fn une_note_ne_s_ecrit_qu_une_fois() {
        let mut notes = vec![];
        noter(&mut notes, "Caméra débranchée");
        noter(&mut notes, "Caméra débranchée");
        noter(&mut notes, bulle::ZONE_TROP_PETITE);
        assert_eq!(notes, vec!["Caméra débranchée".to_string(), bulle::ZONE_TROP_PETITE.to_string()]);
    }

    fn en_cours_de_test(cible: scenes::Cible, notes_scene: Vec<String>) -> EnCours {
        EnCours {
            numero: 1,
            enreg: None,
            reste: 3,
            sortie: PathBuf::new(),
            quand: chrono::Local::now(),
            origine: Origine::default(),
            absentes: Vec::new(),
            demande: Some(Demande { ecran: (0, 0), zone: (0, 0, 16, 16), sortie: PathBuf::new(), reglages: Reglages::default(), effets: None }),
            ecoute: None,
            places: ((0, 0), (0, 0)),
            devant: 0,
            zone_bureau: (0, 0, 16, 16),
            echelle: 1.0,
            bulle: None,
            cameras: Vec::new(),
            notes_camera: Vec::new(),
            cible,
            notes_scene,
            direct: None,
            raccourcis: Vec::new(),
            raccourcis_lisibles: Default::default(),
        }
    }

    #[test]
    fn la_mise_en_scene_dit_sa_cible_et_sa_scene() {
        let cible = cible_de(r"\\.\DISPLAY1", (0, 0), (1920, 1080), None);
        assert_eq!(cible, scenes::Cible::Ecran { ecran: scenes::Ecran { nom: r"\\.\DISPLAY1".into(), position: (0, 0), taille: (1920, 1080) } });
        let etat = en_cours_de_test(cible.clone(), vec!["Scène « Démo ».".into()]).etat();
        assert_eq!((etat.phase, etat.cible, etat.notes), ("prepare", Some(cible), vec!["Scène « Démo ».".to_string()]));
        let zone = cible_de(r"\\.\DISPLAY1", (0, 0), (1920, 1080), Some((10, 20, 640, 480)));
        assert!(matches!(zone, scenes::Cible::Zone { zone: (10, 20, 640, 480), .. }));
    }

    /// tao 0.35.3 : un setter de drapeau (`apply_diff`) fait `SW_HIDE` sur une fenetre qu'il
    /// croit cachee et `SW_SHOW`, qui l'active, sur une visible. La barre passe par `fenetres`.
    #[test]
    fn aucun_setter_de_drapeau_de_tao_sur_la_barre() {
        let code = include_str!("mod.rs").split("#[cfg(test)]").next().unwrap_or_default();
        for setter in ["set_focusable(", "set_ignore_cursor_events(", "set_always_on_top(", "set_resizable(", "set_decorations("] {
            assert!(!code.contains(&format!("barre.{setter}")), "barre.{setter}");
        }
    }

    #[test]
    fn la_mise_en_scene_ne_cree_pas_le_dossier_du_mois() {
        let base = std::env::temp_dir().join(format!("equishot-video-sortie-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let chemin = sortie_sous(&base, &chrono::Local::now()).unwrap();
        assert!(chemin.starts_with(&base) && chemin.extension().is_some_and(|e| e == "mp4"));
        let cree = base.exists();
        let _ = std::fs::remove_dir_all(&base);
        assert!(!cree, "rien ne s'ecrit avant « Démarrer »");
    }

    #[test]
    fn barre_sous_la_zone_centree() {
        assert_eq!(placer_barre(Some((100, 100, 800, 600)), ECRAN, (400, 52)), (300, 708));
    }

    #[test]
    fn barre_au_dessus_si_le_bas_manque() {
        assert_eq!(placer_barre(Some((100, 500, 800, 560)), ECRAN, (400, 52)), (300, 440));
    }

    #[test]
    fn barre_dans_la_zone_si_rien_autour() {
        assert_eq!(placer_barre(Some((0, 0, 1920, 1080)), ECRAN, (400, 52)), (760, 1020));
    }

    #[test]
    fn barre_gardee_dans_l_ecran() {
        // Zone collee au bord gauche d'un ecran place a gauche du principal.
        let ecran = ((-1920, 0), (1920, 1080));
        assert_eq!(placer_barre(Some((-1920, 100, 100, 100)), ecran, (400, 52)), (-1920, 208));
    }

    #[test]
    fn barre_en_bas_au_centre_sans_zone() {
        assert_eq!(placer_barre(None, ECRAN, (400, 52)), (760, 980));
    }

    const B: Rect = (100, 100, 400, 52);
    const P: Rect = (100, 100, 96, 52);

    #[test]
    fn survol_hors_poignee_efface() {
        assert_eq!(survol(Survol::Normale, (300, 120), B, P), Survol::Effacee);
    }

    #[test]
    fn poignee_rend_et_la_barre_reste_rendue_jusqu_a_la_sortie() {
        let s = survol(Survol::Effacee, (120, 120), B, P);
        assert_eq!(s, Survol::Rendue);
        // de la poignee vers « Arreter » : toujours rendue
        assert_eq!(survol(s, (480, 120), B, P), Survol::Rendue);
        // sortie : normale ; retour hors poignee : effacee
        let dehors = survol(Survol::Rendue, (50, 50), B, P);
        assert_eq!(dehors, Survol::Normale);
        assert_eq!(survol(dehors, (300, 120), B, P), Survol::Effacee);
    }

    #[test]
    fn bords_exclus() {
        assert_eq!(survol(Survol::Normale, (500, 120), B, P), Survol::Normale);
    }

    #[test]
    fn panneau_sous_la_zone() {
        assert_eq!(placer_barre(Some((100, 100, 800, 400)), ECRAN, (420, 300)), (290, 546));
    }

    #[test]
    fn avertissement_en_capitale() {
        assert_eq!(capitaliser("sans micro (aucun)"), "Sans micro (aucun)");
        assert_eq!(capitaliser(""), "");
    }

    #[test]
    fn la_camera_debranchee_rejoint_l_avertissement_de_la_barre() {
        let a = avertissement_de(&["sans micro (aucun)".to_string()], vec!["Micro débranché".to_string()], &["Caméra débranchée".to_string()], 0);
        assert_eq!(a.as_deref(), Some("Sans micro (aucun), Micro débranché, Caméra débranchée"));
        assert_eq!(avertissement_de(&[], vec![], &[], 0), None);
        assert_eq!(avertissement_de(&[], vec![], &["Caméra débranchée".to_string()], PREVENIR_MS).as_deref(), Some("Caméra débranchée. Arrêt automatique à une heure"));
    }
}
