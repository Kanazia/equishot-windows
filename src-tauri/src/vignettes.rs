//! Les vignettes flottantes : la pile des dernieres captures, accrochee a la
//! pastille (plan `plans/equishot-vignettes_2026-10-02.md`).
//!
//! La pile vit ici, la page de la fenetre « vignettes » la suit par un evenement.
//! Comme la pastille, la fenetre n'est pas focalisable (un clic sur une carte
//! laisse le premier plan ou il est) et elle est protegee (aucune capture ni
//! aucun partage d'ecran ne la montre). La mesure 4 (`docs/TESTS.md`) a montre
//! qu'un glisser part d'une telle fenetre.

use std::path::PathBuf;
use std::sync::{mpsc, Mutex};
use std::time::Instant;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::captures::stockage::Capture;
use crate::{captures, db, hors_thread_principal, pastille, AppState};

pub const ETIQUETTE: &str = "vignettes";
/// Emis vers la fenetre des vignettes quand la pile change.
const EVENEMENT_PILE: &str = "vignettes://pile";
/// Emis vers toutes les fenetres quand le reglage change.
const EVENEMENT_REGLAGE: &str = "vignettes://reglage";
/// Reglage : `0` coupe les vignettes. Absent, elles sont allumees.
const CLE_REGLAGE: &str = "vignettes";
/// Reglage : `1` cache la pile quand la pastille est rangee. Absent, elle reste.
const CLE_AVEC_PASTILLE: &str = "vignettes_avec_pastille";
/// Cinq cartes au plus ; les plus anciennes restent dans la galerie.
const PLAFOND: usize = 5;

// Geometrie de la page, en pixels CSS (`src/vignettes/main.tsx` la reprend).
const LARGEUR: f64 = 216.0;
const CARTE: f64 = 132.0;
const ECART: f64 = 8.0;
/// Entre la pastille et la carte la plus recente.
const MARGE: f64 = 8.0;
/// Bande de « Tout fermer », a l'autre bout de la pile.
const BANDE: f64 = 32.0;

#[derive(Default)]
pub struct EtatVignettes {
    /// La plus recente en tete.
    pile: Mutex<Vec<Capture>>,
    /// Une seule creation de la fenetre a la fois (`preparer` et `ajouter` peuvent se croiser).
    creation: Mutex<()>,
    /// Un seul placement a la fois : chacun lit la pastille puis place la fenetre,
    /// et le dernier a passer lit forcement sa position finale. Sans lui, un
    /// placement lu tot pouvait s'appliquer en dernier (relecteurs, 2026-10-03).
    placement: Mutex<()>,
}

const VERROU: &str = "La pile des vignettes est dans un état incohérent.";

/// Ce que la page affiche : les cartes de haut en bas, et le cote de la pastille.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pile {
    cartes: Vec<Capture>,
    /// Vrai si la pile est au-dessus de la pastille : la plus recente en bas.
    dessus: bool,
}

/// Met `capture` en tete, sans doublon, et tient le plafond.
fn empiler(pile: &mut Vec<Capture>, capture: Capture) {
    pile.retain(|c| c.id != capture.id);
    pile.insert(0, capture);
    pile.truncate(PLAFOND);
}

/// Rectangle physique : coin et taille.
type Rect = (i32, i32, u32, u32);

/// Place de la fenetre des vignettes dans la zone de travail de l'ecran : sous
/// la pastille si `n` cartes y tiennent, sinon du cote qui en montre le plus,
/// alignee sur son bord gauche. Rend le rectangle, vrai si la pile est au-dessus,
/// et le nombre de cartes montrees (les plus recentes) : la pile ne deborde
/// jamais sur la pastille ni hors de l'ecran, sauf si pas meme une carte ne tient.
fn disposer(pastille: Rect, zone: Rect, echelle: f64, n: usize) -> (Rect, bool, usize) {
    let (px, py, _, ph) = pastille;
    let (zx, zy, zl, zh) = zone;
    let tient = |place: i32| ((((place.max(0) as f64 / echelle) - MARGE - BANDE) / (CARTE + ECART)).floor().max(0.0) as usize).min(n);
    let dessous = tient(zy + zh as i32 - (py + ph as i32));
    let dessus = tient(py - zy);
    let au_dessus = dessous < n && dessus > dessous;
    let montre = if au_dessus { dessus } else { dessous }.max(1);
    let largeur = (LARGEUR * echelle).round() as u32;
    let hauteur = ((MARGE + montre as f64 * (CARTE + ECART) + BANDE) * echelle).round() as u32;
    let x = px.min(zx + zl as i32 - largeur as i32).max(zx);
    let y = if au_dessus { py - hauteur as i32 } else { py + ph as i32 };
    ((x, y, largeur, hauteur), au_dessus, montre)
}

/// Vrai si le reglage n'a pas coupe les vignettes.
fn actives(app: &tauri::AppHandle) -> bool {
    app.state::<AppState>().with_conn(|conn| db::reglage(conn, CLE_REGLAGE)).map(|v| v != "0").unwrap_or(true)
}

/// Vrai si la pile se range avec la pastille.
fn avec_pastille(app: &tauri::AppHandle) -> bool {
    app.state::<AppState>().with_conn(|conn| db::reglage(conn, CLE_AVEC_PASTILLE)).map(|v| v == "1").unwrap_or(false)
}

/// La pile se montre si elle a des cartes, sauf si la pastille est rangee et
/// que le reglage veut qu'elle se range avec elle.
fn montree(cartes: usize, pastille_visible: bool, avec_pastille: bool) -> bool {
    cartes > 0 && (pastille_visible || !avec_pastille)
}

/// La fenetre des vignettes, creee cachee au premier besoin. Jamais depuis le
/// thread principal : creer une fenetre y attend ce meme thread.
fn fenetre(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    let etat = app.state::<EtatVignettes>();
    let _seule = etat.creation.lock().map_err(|_| VERROU.to_string())?;
    if let Some(fenetre) = app.get_webview_window(ETIQUETTE) {
        return Ok(fenetre);
    }
    WebviewWindowBuilder::new(app, ETIQUETTE, WebviewUrl::App("vignettes.html".into()))
        .title("EquiShot, vignettes")
        .inner_size(LARGEUR, CARTE)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .content_protected(crate::db::fenetres_protegees())
        .visible(false)
        .build()
        .map_err(|e| format!("Fenêtre des vignettes impossible à ouvrir : {e}"))
}

/// La pile telle que la page la dessine, et le rectangle de sa fenetre.
fn etat_affiche(app: &tauri::AppHandle) -> Result<(Pile, Option<Rect>), String> {
    let cartes = app.state::<EtatVignettes>().pile.lock().map_err(|_| VERROU.to_string())?.clone();
    if cartes.is_empty() {
        return Ok((Pile { cartes, dessus: false }, None));
    }
    let bulle = app.get_webview_window(pastille::ETIQUETTE).ok_or("La pastille est introuvable.")?;
    if !montree(cartes.len(), !pastille::rangee(), avec_pastille(app)) {
        return Ok((Pile { cartes, dessus: false }, None));
    }
    let (position, taille) = (bulle.outer_position().map_err(|e| e.to_string())?, bulle.outer_size().map_err(|e| e.to_string())?);
    let ecran = app
        .monitor_from_point(position.x as f64 + 8.0, position.y as f64 + 8.0)
        .ok()
        .flatten()
        .or(app.primary_monitor().ok().flatten())
        .ok_or("Aucun écran pour les vignettes.")?;
    let zone = ecran.work_area();
    let (rect, dessus, montre) = disposer(
        (position.x, position.y, taille.width, taille.height),
        (zone.position.x, zone.position.y, zone.size.width, zone.size.height),
        ecran.scale_factor(),
        cartes.len(),
    );
    // ponytail: les cartes qui ne tiennent pas sont cachees, pas retirees de la pile.
    let cartes = cartes.into_iter().take(montre);
    let cartes = if dessus { cartes.rev().collect() } else { cartes.collect() };
    Ok((Pile { cartes, dessus }, Some(rect)))
}

/// Replace la fenetre, la montre ou la cache, et donne la pile a la page.
/// Hors du thread principal (toutes ses commandes y attendent ce thread).
fn actualiser(app: &tauri::AppHandle) -> Result<(), String> {
    let etat = app.state::<EtatVignettes>();
    let _seul = etat.placement.lock().map_err(|_| VERROU.to_string())?;
    let (pile, rect) = etat_affiche(app)?;
    let Some((x, y, l, h)) = rect else {
        if let Some(fenetre) = app.get_webview_window(ETIQUETTE) {
            crate::fenetres::montrer_sans_activer(&fenetre, false);
            let _ = fenetre.emit_to(ETIQUETTE, EVENEMENT_PILE, &pile);
        }
        return Ok(());
    };
    let fenetre = fenetre(app)?;
    // La position avant la taille : changer d'ecran peut changer l'echelle.
    fenetre
        .set_position(tauri::PhysicalPosition::new(x, y))
        .and_then(|_| fenetre.set_size(tauri::PhysicalSize::new(l, h)))
        .map_err(|e| format!("Fenêtre des vignettes impossible à placer : {e}"))?;
    let _ = app.emit_to(ETIQUETTE, EVENEMENT_PILE, &pile);
    // Sans l'activer : la galerie devant perdait le premier plan au profit de la pile.
    crate::fenetres::montrer_sans_activer(&fenetre, true);
    Ok(())
}

fn modifier_pile(app: &tauri::AppHandle, changer: impl FnOnce(&mut Vec<Capture>)) -> Result<(), String> {
    changer(&mut *app.state::<EtatVignettes>().pile.lock().map_err(|_| VERROU.to_string())?);
    actualiser(app)
}

fn dire(motif: Result<(), String>) {
    if let Err(motif) = motif {
        eprintln!("vignettes : {motif}");
    }
}

/// Une capture commence : la fenetre des vignettes se cree des maintenant, cachee,
/// pour que sa page soit chargee quand la capture arrive. Creee a l'enregistrement,
/// la premiere carte mettait 2,6 s a paraitre (parcours du 2026-10-03).
pub fn preparer(app: &tauri::AppHandle) {
    if app.get_webview_window(ETIQUETTE).is_some() {
        return;
    }
    a_part(app, |app| {
        if actives(&app) {
            dire(fenetre(&app).map(|_| ()));
        }
    });
}

/// Une capture vient d'etre enregistree : sa vignette entre dans la pile, si le
/// reglage le veut, et la pastille rangee revient avec elle.
pub fn ajouter(app: &tauri::AppHandle, capture: Capture) {
    a_part(app, move |app| {
        if actives(&app) {
            dire(modifier_pile(&app, |pile| empiler(pile, capture)));
            pastille::montrer(&app);
        }
    });
}

/// Le travail sur la pile part a cote : la commande de capture rend la main
/// sans attendre la fenetre des vignettes.
fn a_part(app: &tauri::AppHandle, travail: impl FnOnce(tauri::AppHandle) + Send + 'static) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || travail(app));
}

/// Une capture a change (annotation refaite) : sa carte suit.
pub fn remplacer(app: &tauri::AppHandle, capture: Capture) {
    a_part(app, move |app| {
        let present = app.state::<EtatVignettes>().pile.lock().map(|p| p.iter().any(|c| c.id == capture.id)).unwrap_or(false);
        if present {
            dire(modifier_pile(&app, |pile| {
                if let Some(carte) = pile.iter_mut().find(|c| c.id == capture.id) {
                    *carte = capture;
                }
            }));
        }
    });
}

/// Une capture a ete supprimee : sa carte part.
pub fn retirer(app: &tauri::AppHandle, id: i64) {
    a_part(app, move |app| dire(modifier_pile(&app, |pile| pile.retain(|c| c.id != id))));
}

/// La pastille a bouge : la pile la suit. Appele depuis le thread principal.
pub fn suivre(app: &tauri::AppHandle) {
    if app.get_webview_window(ETIQUETTE).is_none() {
        return;
    }
    a_part(app, |app| dire(actualiser(&app)));
}

// ---------------------------------------------------------------------------
// Commandes
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn vignettes_lister(app: tauri::AppHandle) -> Result<Pile, String> {
    Ok(etat_affiche(&app)?.0)
}

#[tauri::command]
pub async fn vignette_fermer(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    modifier_pile(&app, |pile| pile.retain(|c| c.id != id))
}

#[tauri::command]
pub async fn vignettes_tout_fermer(app: tauri::AppHandle) -> Result<(), String> {
    modifier_pile(&app, Vec::clear)
}

/// Le reglage « Vignette flottante apres une capture ».
#[tauri::command]
pub async fn vignettes_actives(app: tauri::AppHandle) -> Result<bool, String> {
    hors_thread_principal(app, |app| Ok(actives(&app))).await
}

/// Applique le reglage tout de suite : coupe, la pile se vide.
#[tauri::command]
pub async fn vignettes_activer(app: tauri::AppHandle, actif: bool) -> Result<bool, String> {
    hors_thread_principal(app, move |app| {
        app.state::<AppState>().with_conn(|conn| db::regler(conn, CLE_REGLAGE, if actif { "1" } else { "0" }))?;
        if !actif {
            modifier_pile(&app, Vec::clear)?;
        }
        let _ = app.emit(EVENEMENT_REGLAGE, actif);
        Ok(actif)
    })
    .await
}

/// Le reglage « Ranger les vignettes avec la pastille ».
#[tauri::command]
pub async fn vignettes_avec_pastille(app: tauri::AppHandle) -> Result<bool, String> {
    hors_thread_principal(app, |app| Ok(avec_pastille(&app))).await
}

/// Applique le reglage tout de suite : la pile se montre ou se cache selon la pastille.
#[tauri::command]
pub async fn vignettes_ranger_avec_pastille(app: tauri::AppHandle, actif: bool) -> Result<bool, String> {
    hors_thread_principal(app, move |app| {
        app.state::<AppState>().with_conn(|conn| db::regler(conn, CLE_AVEC_PASTILLE, if actif { "1" } else { "0" }))?;
        actualiser(&app)?;
        Ok(actif)
    })
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Glisse {
    /// Vrai si `DoDragDrop` a rendu « depose ». `drag` ne lit pas l'effet
    /// obtenu : cela ne dit pas qu'une application a accepte le fichier.
    depose: bool,
    /// Duree du glisser, du depart au depot ou a l'abandon.
    duree_ms: u64,
}

/// Glisse le rendu de la capture `id`, ou le MP4 d'une video, hors de la fenetre qui appelle. Le chemin
/// se lit en base : la page ne donne jamais qu'un identifiant.
///
/// `DoDragDrop` est une boucle modale qui doit tourner sur le fil de la fenetre,
/// le thread principal : c'est la seule attente qu'EquiShot y fait, et ce thread
/// continue d'y traiter les messages (mesure 4). La commande, elle, attend la
/// fin du glisser hors de ce thread.
#[tauri::command]
pub async fn vignette_glisser(app: tauri::AppHandle, fenetre: tauri::WebviewWindow, id: i64) -> Result<Glisse, String> {
    // La vignette (l'image du glisser) se lit ici : sur le thread principal, une
    // lecture d'un disque en veille figerait la pastille.
    let (rendu, vignette) = hors_thread_principal(app.clone(), move |app| {
        let capture = captures::lire(&app, id)?;
        let vignette = std::fs::read(&capture.vignette).map_err(|e| format!("Vignette illisible : {e}"))?;
        // Une video se glisse en MP4 (ADR-016), une image en rendu annote.
        let fichier = if capture.duree_ms.is_some() { capture.origine } else { capture.rendu };
        Ok((PathBuf::from(fichier), vignette))
    })
    .await?;
    let (envoi, reception) = mpsc::channel();
    app.run_on_main_thread(move || {
        let debut = Instant::now();
        let rappel = envoi.clone();
        let depart = drag::start_drag(
            &fenetre,
            drag::DragItem::Files(vec![rendu]),
            drag::Image::Raw(vignette),
            move |resultat, _| {
                let depose = matches!(resultat, drag::DragResult::Dropped);
                let _ = rappel.send(Ok(Glisse { depose, duree_ms: debut.elapsed().as_millis() as u64 }));
            },
            drag::Options::default(),
        );
        if let Err(motif) = depart {
            let _ = envoi.send(Err(format!("Glisser impossible : {motif}")));
        }
    })
    .map_err(|e| format!("Glisser impossible : {e}"))?;
    hors_thread_principal(app, move |_| reception.recv().map_err(|_| "Le glisser s'est interrompu.".to_string())?).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(id: i64) -> Capture {
        Capture { id, ..Default::default() }
    }

    #[test]
    fn pile_cachee_avec_la_pastille_rangee_seulement_si_le_reglage_le_veut() {
        assert!(!montree(0, true, false), "vide, jamais montree");
        assert!(montree(2, true, true), "pastille visible : la pile aussi");
        assert!(montree(2, false, false), "reglage decoche : la pile reste sans la pastille");
        assert!(!montree(2, false, true), "reglage coche, pastille rangee : la pile se cache");
    }

    #[test]
    fn pile_plus_recente_en_tete_cinq_au_plus_sans_doublon() {
        let mut pile = Vec::new();
        for id in 1..=6 {
            empiler(&mut pile, capture(id));
        }
        assert_eq!(pile.iter().map(|c| c.id).collect::<Vec<_>>(), vec![6, 5, 4, 3, 2], "la plus ancienne sort");
        empiler(&mut pile, capture(4));
        assert_eq!(pile.iter().map(|c| c.id).collect::<Vec<_>>(), vec![4, 6, 5, 3, 2], "reprise, elle remonte sans doublon");
    }

    #[test]
    fn pile_sous_la_pastille_ou_au_dessus_et_dans_l_ecran() {
        let ecran = (0, 0, 1920, 1080);
        let hauteur = |n: f64| (MARGE + n * (CARTE + ECART) + BANDE) as u32;
        // Pastille en haut : la pile dessous, collee a son bord bas.
        let ((x, y, l, h), dessus, montre) = disposer((100, 24, 404, 56), ecran, 1.0, 2);
        assert!(!dessus);
        assert_eq!((x, y, l, h, montre), (100, 80, 216, hauteur(2.0), 2));
        // Pastille en bas : la pile au-dessus, son bas contre la pastille.
        let ((_, y, _, h), dessus, montre) = disposer((100, 1000, 404, 56), ecran, 1.0, 5);
        assert!(dessus);
        assert_eq!((y + h as i32, montre), (1000, 5));
        // Pastille au bord droit : la pile reste dans l'ecran.
        let ((x, _, l, _), _, _) = disposer((1800, 24, 404, 56), ecran, 1.0, 1);
        assert_eq!(x + l as i32, 1920);
        // Echelle 1,5 : tout est en pixels physiques.
        let ((_, _, l, _), _, _) = disposer((0, 0, 606, 84), ecran, 1.5, 1);
        assert_eq!(l, 324);
    }

    #[test]
    fn pile_qui_ne_tient_pas_ne_couvre_ni_la_pastille_ni_le_bord() {
        let ecran = (0, 0, 1920, 1080);
        // Pastille au milieu : cinq cartes ne tiennent d'aucun cote, la pile en
        // montre ce qui tient, du cote le plus grand, sans toucher la pastille.
        let ((_, y, _, h), dessus, montre) = disposer((100, 400, 404, 56), ecran, 1.0, 5);
        assert!(!dessus);
        assert_eq!((y, montre), (456, 4));
        assert!(y + h as i32 <= 1080);
        // Portable 1080p a 150 % : pas plus de cartes que l'ecran n'en tient.
        let ((_, y, _, h), dessus, montre) = disposer((0, 36, 606, 84), ecran, 1.5, 5);
        assert!(!dessus && montre < 5);
        assert!(y >= 120 && y + h as i32 <= 1080, "{y} + {h}");
        // Zone de travail : la barre des taches en bas n'est pas couverte.
        let ((_, y, _, h), _, _) = disposer((0, 24, 404, 56), (0, 0, 1920, 1032), 1.0, 5);
        assert!(y + h as i32 <= 1032);
    }
}
