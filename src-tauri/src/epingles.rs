//! Les epingles : une capture posee sur l'ecran, toujours devant (ADR-011, plan
//! `plans/equishot-epingle_2026-10-06.md`).
//!
//! Une fenetre par epingle, `epingle-1`, `epingle-2`... Au contraire de la
//! pastille, elle est focalisable (la molette et le clavier lui arrivent apres
//! un clic) et visible dans les captures : c'est un contenu choisi, pas un outil.
//! Rust tient son rectangle et fait tout ce qui touche la fenetre ou le disque ;
//! la page traduit les gestes et tient l'opacite.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::captures::stockage::{self, Capture};
use crate::{captures, fenetres, hors_thread_principal, pastille};

/// Rectangle physique du bureau : coin et taille.
pub type Rect = (i32, i32, u32, u32);

/// Etiquettes des fenetres, et des identifiants de leur menu.
pub const PREFIXE: &str = "epingle-";
const TITRE: &str = "EquiShot, épingle";
/// Emis vers une epingle : « Taille d'origine » de son menu.
const EVENEMENT_ORIGINE: &str = "epingle://origine";
const VERROU: &str = "Les épingles sont dans un état incohérent.";
const DISPARUE: &str = "Cette épingle n'existe plus.";

struct Epingle {
    id: i64,
    rendu: PathBuf,
    /// Date de la derniere modification : la page force le rechargement de l'image.
    version: String,
    /// Taille de l'image en pixels : 100 %.
    base: (u32, u32),
    /// Taille a la pose, en pourcents de `base`.
    pourcent: f64,
    /// Tenu ici : relire la fenetre juste apres l'avoir changee rendrait l'ancien.
    rect: Rect,
}

#[derive(Default)]
pub struct EtatEpingles {
    epingles: Mutex<HashMap<String, Epingle>>,
    derniere: AtomicU32,
}

/// La zone de travail de l'ecran de la fenetre EquiShot, ou de l'ecran principal.
fn zone_de_la_galerie(app: &tauri::AppHandle) -> Result<Rect, String> {
    let ecran = app
        .get_webview_window(pastille::ETIQUETTE_PRINCIPALE)
        .and_then(|f| f.current_monitor().ok().flatten())
        .or(app.primary_monitor().ok().flatten())
        .ok_or("Aucun écran pour l'épingle.")?;
    let zone = ecran.work_area();
    Ok((zone.position.x, zone.position.y, zone.size.width, zone.size.height))
}

fn avec<T>(app: &tauri::AppHandle, etiquette: &str, f: impl FnOnce(&mut Epingle) -> T) -> Result<T, String> {
    let etat = app.state::<EtatEpingles>();
    let mut epingles = etat.epingles.lock().map_err(|_| VERROU.to_string())?;
    epingles.get_mut(etiquette).map(f).ok_or_else(|| DISPARUE.to_string())
}

/// Pose une epingle de `capture`. `place` : le rectangle physique de la zone,
/// quand elle vient de la selection ; absente, l'epingle se centre sur l'ecran
/// de la fenetre EquiShot. Bloquant, jamais sur le thread principal. Rend
/// l'etiquette de la fenetre.
pub fn epingler(app: &tauri::AppHandle, capture: &Capture, place: Option<Rect>) -> Result<String, String> {
    crate::captures::exiger_image(capture)?;
    // Le rendu : le cadre (ADR-007) et ses marges de fond (ADR-012).
    let base = capture.taille_rendu();
    let (rect, pourcent) = match place {
        // Une zone de la selection n'est jamais recadree : le rendu a sa taille.
        Some(place) => (place, 100.0),
        None => place_centree(base, zone_de_la_galerie(app)?),
    };
    let etat = app.state::<EtatEpingles>();
    let etiquette = format!("{PREFIXE}{}", etat.derniere.fetch_add(1, Ordering::Relaxed) + 1);
    let epingle = Epingle { id: capture.id, rendu: PathBuf::from(&capture.rendu), version: capture.modifie_le.clone(), base, pourcent, rect };
    etat.epingles.lock().map_err(|_| VERROU.to_string())?.insert(etiquette.clone(), epingle);
    let construite = WebviewWindowBuilder::new(app, &etiquette, WebviewUrl::App("epingle.html".into()))
        .title(TITRE)
        .decorations(false)
        .transparent(true)
        // `shadow(true)` arrondit les coins sous Windows 11 : l'image serait rognee.
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        // Posee sans prendre le premier plan ; un clic le lui donne.
        .focused(false)
        .visible(false)
        .build();
    let fenetre = match construite {
        Ok(fenetre) => fenetre,
        Err(e) => {
            etat.epingles.lock().map_err(|_| VERROU.to_string())?.remove(&etiquette);
            return Err(format!("Épingle impossible à ouvrir : {e}"));
        }
    };
    // Alt relache seul gelerait la fenetre, qui prend le clavier.
    fenetres::sans_menu_alt(&fenetre);
    let (suivi, nom) = (app.clone(), etiquette.clone());
    fenetre.on_window_event(move |evenement| {
        let etat = suivi.state::<EtatEpingles>();
        let Ok(mut epingles) = etat.epingles.lock() else { return };
        match evenement {
            // ponytail: un Moved en retard sur une rafale de molette peut decaler l'ancre d'un cran,
            // le suivant la rattrape ; un rectangle date par cran si cela se voit.
            tauri::WindowEvent::Moved(p) => {
                if let Some(e) = epingles.get_mut(&nom) {
                    (e.rect.0, e.rect.1) = (p.x, p.y);
                }
            }
            // Changer d'ecran peut changer l'echelle : Windows retaille la fenetre.
            tauri::WindowEvent::Resized(t) if t.width > 0 && t.height > 0 => {
                if let Some(e) = epingles.get_mut(&nom) {
                    (e.rect.2, e.rect.3) = (t.width, t.height);
                }
            }
            tauri::WindowEvent::Destroyed => {
                epingles.remove(&nom);
            }
            _ => {}
        }
    });
    if let Err(motif) = placer(&fenetre, rect) {
        // Cachee et jamais montree, elle resterait ouverte pour rien.
        let _ = fenetre.destroy();
        return Err(motif);
    }
    // Sans l'activer : la fenetre ou l'on travaille garde le clavier.
    fenetres::montrer_sans_activer(&fenetre, true);
    Ok(etiquette)
}

fn placer(fenetre: &tauri::WebviewWindow, (x, y, l, h): Rect) -> Result<(), String> {
    // La position avant la taille : changer d'ecran peut changer l'echelle.
    fenetre
        .set_position(tauri::PhysicalPosition::new(x, y))
        .and_then(|_| fenetre.set_size(tauri::PhysicalSize::new(l, h)))
        .map_err(|e| format!("Épingle impossible à placer : {e}"))
}

fn fermer(fenetre: &tauri::WebviewWindow) {
    // Une epingle active rend le focus en se fermant : sans ce marquage, l'exe tombe.
    fenetres::avant_fermeture(fenetre);
    let _ = fenetre.close();
}

fn fermer_tout(app: &tauri::AppHandle) {
    for (etiquette, fenetre) in app.webview_windows() {
        if etiquette.starts_with(PREFIXE) {
            fermer(&fenetre);
        }
    }
}

/// Copie l'image de l'epingle : son rendu, ou sa copie de la corbeille.
fn copier(app: &tauri::AppHandle, etiquette: &str) -> Result<(), String> {
    let rendu = avec(app, etiquette, |e| e.rendu.clone())?;
    let chemin = image_de(&rendu, &captures::dossier(app)?);
    captures::copier_image(&stockage::lire_png(&chemin)?)
}

fn ouvrir_dans_la_galerie(app: &tauri::AppHandle, etiquette: &str) -> Result<(), String> {
    let id = avec(app, etiquette, |e| e.id)?;
    // Supprimee depuis : la pastille le dit, l'epingle reste.
    captures::lire(app, id).map_err(|_| "Cette capture n'est plus dans la galerie.".to_string())?;
    pastille::ouvrir_principale(app, &format!("capture-{id}"))
}

/// Un clic du menu d'une epingle (`epingle-<action>@<etiquette>`). Appele par le
/// gestionnaire du menu de notification, sur le thread principal : le travail part a cote.
pub fn sur_menu(app: &tauri::AppHandle, id: &str) {
    let Some((action, etiquette)) = id.strip_prefix(PREFIXE).and_then(|reste| reste.split_once('@')) else { return };
    let (app, action, etiquette) = (app.clone(), action.to_string(), etiquette.to_string());
    tauri::async_runtime::spawn_blocking(move || {
        let fait = match action.as_str() {
            "copier" => copier(&app, &etiquette),
            "galerie" => ouvrir_dans_la_galerie(&app, &etiquette),
            "origine" => app.emit_to(etiquette.as_str(), EVENEMENT_ORIGINE, ()).map_err(|e| e.to_string()),
            "fermer" => app.get_webview_window(&etiquette).map(|f| fermer(&f)).ok_or_else(|| DISPARUE.to_string()),
            "tout" => {
                fermer_tout(&app);
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(motif) = fait {
            captures::avertir(&app, &motif);
        }
    });
}

/// Part de la zone de travail qu'une epingle de la galerie ne depasse pas a sa pose.
const PART_ECRAN: f64 = 0.8;
const POURCENT_MIN: f64 = 20.0;
const POURCENT_MAX: f64 = 500.0;
/// Plus grand cote d'une epingle, en pixels physiques : un ecran entier a 500 %
/// ferait une fenetre de 20 000 px que la WebView n'a pas a peindre.
const COTE_MAX: f64 = 8192.0;

/// Place d'une epingle de la galerie : centree dans `zone`, reduite d'un meme
/// rapport si elle depasse 80 % de sa largeur ou de sa hauteur. Rend le
/// rectangle et la taille en pourcents de l'image.
fn place_centree(image: (u32, u32), zone: Rect) -> (Rect, f64) {
    let (zx, zy, zl, zh) = zone;
    let rapport = (PART_ECRAN * zl as f64 / image.0.max(1) as f64)
        .min(PART_ECRAN * zh as f64 / image.1.max(1) as f64)
        .min(1.0);
    let cote = |c: u32| ((c as f64 * rapport).round() as u32).max(1);
    let (l, h) = (cote(image.0), cote(image.1));
    ((zx + (zl as i32 - l as i32) / 2, zy + (zh as i32 - h as i32) / 2, l, h), rapport * 100.0)
}

/// Le rectangle a `pourcent` de `base`, le point `ancre` (fraction de la fenetre,
/// 0 a 1) restant sous le curseur. Taille bornee a 20-500 % et a `COTE_MAX`,
/// 100 % si la page envoie autre chose qu'un nombre.
fn zoomer(rect: Rect, base: (u32, u32), pourcent: f64, ancre: (f64, f64)) -> (Rect, f64) {
    let plafond = COTE_MAX * 100.0 / base.0.max(base.1).max(1) as f64;
    let pourcent = if pourcent.is_finite() { pourcent.clamp(POURCENT_MIN, POURCENT_MAX).min(plafond) } else { 100.0 };
    let borne = |a: f64| if a.is_finite() { a.clamp(0.0, 1.0) } else { 0.5 };
    let (ax, ay) = (borne(ancre.0), borne(ancre.1));
    let (x, y, l, h) = rect;
    let cote = |b: u32| ((b as f64 * pourcent / 100.0).round() as u32).max(1);
    let (nl, nh) = (cote(base.0), cote(base.1));
    let nx = x + (ax * l as f64 - ax * nl as f64).round() as i32;
    let ny = y + (ay * h as f64 - ay * nh as f64).round() as i32;
    ((nx, ny, nl, nh), pourcent)
}

/// L'image d'une epingle : son rendu, ou sa copie dans la corbeille des captures
/// si la capture a ete supprimee depuis (`stockage::vers_corbeille`).
fn image_de(rendu: &Path, dossier: &Path) -> PathBuf {
    match rendu.file_name() {
        Some(nom) if !rendu.exists() => dossier.join(stockage::CORBEILLE).join(nom),
        _ => rendu.to_path_buf(),
    }
}

// --- Commandes ---

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InfosEpingle {
    id: i64,
    rendu: String,
    version: String,
    pourcent: f64,
}

#[tauri::command]
pub async fn epingle_infos(app: tauri::AppHandle, fenetre: tauri::WebviewWindow) -> Result<InfosEpingle, String> {
    // Une capture supprimee juste apres « Epingler » se lit dans la corbeille.
    // ponytail: une suppression entre cette ligne et le fetch de la page laisse l'epingle vide ; relire les infos au premier echec si ca se voit.
    let dossier = captures::dossier(&app)?;
    avec(&app, fenetre.label(), |e| InfosEpingle {
        id: e.id,
        rendu: image_de(&e.rendu, &dossier).to_string_lossy().into_owned(),
        version: e.version.clone(),
        pourcent: e.pourcent,
    })
}

/// Molette, clic du milieu, « Taille d'origine » : la taille demandee, bornee,
/// autour du point `ancre` (fraction de la fenetre). Rend la taille obtenue.
#[tauri::command]
pub async fn epingle_taille(app: tauri::AppHandle, fenetre: tauri::WebviewWindow, pourcent: f64, ancre_x: f64, ancre_y: f64) -> Result<f64, String> {
    let (rect, pourcent) = avec(&app, fenetre.label(), |e| {
        let (rect, pourcent) = zoomer(e.rect, e.base, pourcent, (ancre_x, ancre_y));
        (e.rect, e.pourcent) = (rect, pourcent);
        (rect, pourcent)
    })?;
    placer(&fenetre, rect)?;
    Ok(pourcent)
}

#[tauri::command]
pub async fn epingle_copier(app: tauri::AppHandle, fenetre: tauri::WebviewWindow) -> Result<(), String> {
    let etiquette = fenetre.label().to_string();
    hors_thread_principal(app, move |app| copier(&app, &etiquette)).await
}

/// Le clic droit : le menu natif, sous le curseur. Ses clics partent au
/// gestionnaire du menu de notification, puis a `sur_menu`.
#[tauri::command]
pub async fn epingle_menu(app: tauri::AppHandle, fenetre: tauri::WebviewWindow) -> Result<(), String> {
    let entree = |action: &str, texte: &str| MenuItem::with_id(&app, format!("{PREFIXE}{action}@{}", fenetre.label()), texte, true, None::<&str>);
    let menu = Menu::with_items(
        &app,
        &[
            &entree("copier", "Copier").map_err(|e| e.to_string())?,
            &entree("galerie", "Ouvrir dans la galerie").map_err(|e| e.to_string())?,
            &entree("origine", "Taille d'origine").map_err(|e| e.to_string())?,
            &PredefinedMenuItem::separator(&app).map_err(|e| e.to_string())?,
            &entree("fermer", "Fermer").map_err(|e| e.to_string())?,
            &entree("tout", "Fermer toutes les épingles").map_err(|e| e.to_string())?,
        ],
    )
    .map_err(|e| format!("Menu de l'épingle impossible : {e}"))?;
    fenetre.popup_menu(&menu).map_err(|e| format!("Menu de l'épingle impossible à ouvrir : {e}"))
}

/// Double-clic, Echap.
#[tauri::command]
pub async fn epingle_fermer(fenetre: tauri::WebviewWindow) {
    fermer(&fenetre);
}

/// « Épingler » de la fiche de la galerie.
#[tauri::command]
pub async fn epingle_ouvrir(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    hors_thread_principal(app, move |app| epingler(&app, &captures::lire(&app, id)?, None).map(|_| ())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epingle_de_galerie_centree_et_reduite_sous_80_pour_cent() {
        let ecran = (0, 0, 1920, 1040);
        // Petite : taille reelle, centree.
        assert_eq!(place_centree((400, 300), ecran), ((760, 370, 400, 300), 100.0));
        // Ecran entier : reduite d'un meme rapport, la hauteur limite (0,8 x 1040 / 1080).
        let ((x, y, l, h), pourcent) = place_centree((1920, 1080), ecran);
        assert_eq!((x, y, l, h), (220, 104, 1479, 832));
        assert!((pourcent - 77.037).abs() < 0.01, "{pourcent}");
        // Ecran a gauche, x negatifs.
        assert_eq!(place_centree((100, 100), (-1920, 0, 1920, 1080)).0, (-1010, 490, 100, 100));
    }

    #[test]
    fn molette_garde_le_point_sous_le_curseur_et_borne_la_taille() {
        let base = (400, 200);
        // 100 -> 150 %, curseur au centre : la fenetre grandit autour de lui.
        assert_eq!(zoomer((100, 100, 400, 200), base, 150.0, (0.5, 0.5)), ((0, 50, 600, 300), 150.0));
        // Curseur au coin haut gauche : ce coin ne bouge pas.
        assert_eq!(zoomer((100, 100, 400, 200), base, 150.0, (0.0, 0.0)).0, (100, 100, 600, 300));
        // Bornes : 20 et 500 %.
        assert_eq!(zoomer((0, 0, 400, 200), base, 5.0, (0.0, 0.0)), ((0, 0, 80, 40), 20.0));
        assert_eq!(zoomer((0, 0, 400, 200), base, 900.0, (0.0, 0.0)), ((0, 0, 2000, 1000), 500.0));
        // Une valeur folle de la page ne casse rien : 100 %.
        assert_eq!(zoomer((0, 0, 80, 40), base, f64::NAN, (0.0, 0.0)), ((0, 0, 400, 200), 100.0));
        // Une ancre hors de la fenetre est ramenee au bord.
        assert_eq!(zoomer((100, 100, 400, 200), base, 150.0, (7.0, -3.0)).0, (-100, 100, 600, 300));
        // Une zone de 4 px a 20 % garde un pixel.
        assert_eq!(zoomer((0, 0, 4, 4), (4, 4), 20.0, (0.0, 0.0)).0, (0, 0, 1, 1));
        // Un grand ecran a 500 % : le plus grand cote s'arrete a 8192 px.
        let ((_, _, l, h), pourcent) = zoomer((0, 0, 4000, 1000), (4000, 1000), 500.0, (0.0, 0.0));
        assert_eq!((l, h), (8192, 2048));
        assert!((pourcent - 204.8).abs() < 0.001, "{pourcent}");
    }

    #[test]
    fn image_d_une_capture_supprimee_lue_dans_la_corbeille() {
        let dossier = std::env::temp_dir().join(format!("equishot-epingle-{}", std::process::id()));
        let rendu = dossier.join("20261006-101500-000.rendu.png");
        std::fs::create_dir_all(&dossier).unwrap();
        std::fs::write(&rendu, b"png").unwrap();
        assert_eq!(image_de(&rendu, &dossier), rendu, "presente : le rendu");
        std::fs::remove_file(&rendu).unwrap();
        assert_eq!(image_de(&rendu, &dossier), dossier.join("corbeille").join("20261006-101500-000.rendu.png"));
        let _ = std::fs::remove_dir_all(&dossier);
    }
}
