//! Captures : capturer, annoter, retrouver, sur l'ecran vivant (ADR-001).
//!
//! Le raccourci, ou « Zone », releve la fenetre au premier plan, puis ouvre
//! par-dessus l'ecran sous le curseur une fenetre transparente et protegee
//! (`content_protected`) : l'ecran continue de vivre a travers elle, et elle
//! n'apparait dans aucune image. L'ecran n'est pris qu'au relachement du trace,
//! et la zone prise s'affiche figee dans son cadre pour etre annotee. Les formes
//! restent des donnees, l'original n'est jamais modifie.
//!
//! Ni capture, ni encodage, ni fichier sur le thread principal : ils passent par
//! `hors_thread_principal`. Y tournent seulement le tri du raccourci
//! (`sur_raccourci`) et, au demarrage, l'ouverture de la base, la lecture de deux
//! reglages et l'enregistrement du raccourci aupres de Windows.

pub mod defilante;
pub mod deplacer;
pub mod ocr;
pub mod raccourci;
pub mod souris;
pub mod stockage;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, Instant};

use base64::Engine;
use image::RgbaImage;
use tauri::{Emitter, Manager};

use crate::{db, fenetres, hors_thread_principal, AppState};
use raccourci::{Declencheur, EtatRaccourci, Raccourci};
use stockage::{Capture, Origine, ZoneCss};

/// Libelle de la fenetre de selection.
pub const ETIQUETTE_SELECTION: &str = "capture-selection";
/// Emis vers toutes les fenetres quand la galerie a change.
pub const EVENEMENT_CHANGEE: &str = "captures://changee";
/// Emis vers la selection ouverte quand l'autre raccourci change son mode.
pub const EVENEMENT_MODE: &str = "captures://mode";
/// Emis vers la pastille quand un echec n'a aucune page pour se dire.
pub const EVENEMENT_ERREUR: &str = "captures://erreur";
/// Reglage : le declencheur de la capture, en JSON type.
pub const CLE_DECLENCHEUR: &str = "captures_declencheur";
/// Reglage : le declencheur de la case secondaire (ADR-003).
pub const CLE_DECLENCHEUR_SECONDAIRE: &str = "captures_declencheur_secondaire";
/// Reglage : le declencheur de la case « Derniere zone » (ADR-009).
pub const CLE_DECLENCHEUR_DERNIERE: &str = "captures_declencheur_derniere";
/// Reglage : le declencheur de la case « Texte d'une zone » (ADR-010).
pub const CLE_DECLENCHEUR_TEXTE: &str = "captures_declencheur_texte";
/// Reglage : le declencheur de la case « Filmer » (ADR-016).
pub const CLE_DECLENCHEUR_VIDEO: &str = "captures_declencheur_video";
/// Reglage : le declencheur de la case « Ecran entier » (0.21).
pub const CLE_DECLENCHEUR_ECRAN: &str = "captures_declencheur_ecran";
/// La cle de reglage de chaque case, dans l'ordre de `raccourci::CASES`.
const CLES_DECLENCHEURS: [&str; raccourci::NB_CASES] =
    [CLE_DECLENCHEUR, CLE_DECLENCHEUR_SECONDAIRE, CLE_DECLENCHEUR_DERNIERE, CLE_DECLENCHEUR_TEXTE, CLE_DECLENCHEUR_VIDEO, CLE_DECLENCHEUR_ECRAN];
/// Un message neutre pour la pastille (« Texte copié »), l'erreur gardant le rouge.
pub const EVENEMENT_INFO: &str = "captures://info";
/// Reglage : la derniere zone figee, en JSON (`stockage::DerniereZone`).
pub const CLE_DERNIERE_ZONE: &str = "captures_derniere_zone";
/// Le compte a rebours de l'ecran entier differe : les secondes restantes, ou
/// `null` quand il s'arrete (pris ou annule).
pub const EVENEMENT_DIFFERE: &str = "captures://differe";
/// La capture defilante avance : les vues recollees et la hauteur atteinte.
pub const EVENEMENT_DEFILE: &str = "captures://defile";
/// Reglage : le dossier des captures, chemin absolu (ADR-020).
pub const CLE_DOSSIER: &str = "dossier_captures";
/// Au demarrage (`setup`), une fois : le dossier des captures est le reglage
/// s'il existe, sinon le dossier par defaut, ecrit en base en production.
pub fn choisir_dossier(app: &tauri::AppHandle) {
    let etat = app.state::<AppState>();
    let defaut = if etat.dev {
        etat.data_dir.join("captures")
    } else {
        app.path().picture_dir().map(|d| d.join("EquiShot")).unwrap_or_else(|_| etat.data_dir.join("captures"))
    };
    let choisi = etat
        .with_conn(|conn| {
            let regle = db::reglage(conn, CLE_DOSSIER)?;
            let choisi = if regle.is_empty() { defaut.clone() } else { PathBuf::from(&regle) };
            if regle.is_empty() && !etat.dev {
                db::regler(conn, CLE_DOSSIER, &choisi.to_string_lossy())?;
            }
            Ok(choisi)
        })
        .unwrap_or_else(|motif| {
            eprintln!("dossier des captures non lu : {motif}");
            defaut
        });
    if let Ok(mut c) = app.state::<EtatCaptures>().choisi.lock() {
        *c = choisi;
    }
}

/// La selection ouverte : d'ou elle part, l'ecran qu'elle couvre, la zone prise.
struct Selection {
    origine: Origine,
    /// Fenetre au premier plan avant la selection : elle le retrouve apres.
    devant: isize,
    /// Coin de l'ecran sur le bureau, en pixels physiques.
    position: (i32, i32),
    /// Taille de l'ecran, en pixels physiques, telle que Tauri la donne.
    taille: (u32, u32),
    debut: Instant,
    /// Delai entre le declenchement et la selection montree.
    delai_ms: Option<u64>,
    /// La zone prise au relachement du trace.
    prise: Option<Arc<RgbaImage>>,
    /// Numero du trace de `prise`, donne par la page.
    trace: u32,
    /// La capture deja ecrite pour `prise` (ADR-004), que Terminer annote,
    /// que Supprimer et un nouveau trace jettent.
    enregistree: Option<i64>,
    /// Les fenetres de l'ecran, de la plus haute a la plus basse (ADR-008).
    fenetres: Vec<FenetreVue>,
    /// La fiche de `prise` quand c'est une fenetre cliquee : son titre et son
    /// application, pas ceux du premier plan.
    origine_prise: Option<Origine>,
    /// La zone de `prise`, relative a l'ecran : gardee comme derniere zone
    /// quand elle est figee (ADR-009).
    zone_prise: Option<stockage::ZonePhysique>,
    /// Image, texte (ADR-010 : la zone figee est lue et son texte copie) ou
    /// video (ADR-016 : la zone est filmee).
    mode: Mode,
}

/// Ce que la selection fait de la zone tracee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Image,
    Texte,
    Video,
}

impl Selection {
    fn origine_de_la_prise(&self) -> Origine {
        self.origine_prise.clone().unwrap_or_else(|| self.origine.clone())
    }
}

/// Une fenetre de l'ecran de la selection, rognee a cet ecran.
#[derive(Clone, serde::Serialize)]
pub struct FenetreVue {
    #[serde(flatten)]
    zone: stockage::ZonePhysique,
    titre: String,
    /// Poignee : l'application se lit seulement pour la fenetre cliquee.
    #[serde(skip)]
    id: u32,
}

/// Les fenetres visibles sur l'ecran `position`, `taille`, dans l'ordre de
/// Windows. xcap ecarte deja les invisibles, les masquees par DWM, le bureau et
/// celles d'EquiShot (`is_valid_window`), et donne leur cadre DWM. Bloquant.
fn fenetres_de_l_ecran(position: (i32, i32), taille: (u32, u32)) -> Vec<FenetreVue> {
    let Ok(toutes) = xcap::Window::all() else { return Vec::new() };
    toutes
        .into_iter()
        .filter(|f| !f.is_minimized().unwrap_or(true))
        .filter_map(|f| {
            let cadre = (f.x().ok()?, f.y().ok()?, f.width().ok()?, f.height().ok()?);
            let zone = stockage::rogner_a_l_ecran(cadre, position, taille)?;
            Some(FenetreVue { zone, titre: f.title().unwrap_or_default(), id: f.id().ok()? })
        })
        .collect()
}

/// L'application d'une fenetre relevee, vide si elle n'est plus la.
fn application_de(id: u32) -> String {
    xcap::Window::all()
        .ok()
        .and_then(|toutes| toutes.into_iter().find(|f| f.id().ok() == Some(id)))
        .and_then(|f| f.app_name().ok())
        .unwrap_or_default()
}

#[derive(Default)]
pub struct EtatCaptures {
    raccourci: Mutex<Raccourci>,
    selection: Mutex<Option<Selection>>,
    /// Une seule selection a la fois : deux frappes rapprochees n'ouvrent pas
    /// deux fenetres. Porte l'instant de la frappe qui l'a ouverte.
    en_cours: Mutex<Option<Instant>>,
    /// Le dossier des captures, choisi au demarrage, change par le seul
    /// deplacement (ADR-020). Vide avant `choisir_dossier`.
    choisi: Mutex<PathBuf>,
    /// Tenu en lecture par qui ecrit dans le dossier des captures (`ecriture`),
    /// en ecriture par le deplacement : aucune ecriture n'est en vol pendant qu'il
    /// copie, et aucune ne part vers l'ancien dossier apres lui. Toujours pris
    /// avant `choisi`.
    ecritures: RwLock<()>,
    /// Dernier dossier ouvert au protocole `asset`.
    dossier_ouvert: Mutex<Option<PathBuf>>,
    /// Dernier dossier des videos ouvert au protocole `asset` (ADR-016).
    videos_ouvert: Mutex<Option<PathBuf>>,
    differe: Mutex<Differe>,
    /// Le drapeau d'arret de la capture defilante en cours (ADR-015).
    defilement: Mutex<Option<Arc<AtomicBool>>>,
}

/// L'ecran entier differe (ADR-009). Chaque differe a son numero : une attente
/// annulee, puis relancee dans la meme seconde, ne prend pas l'ecran du nouveau.
#[derive(Default)]
struct Differe {
    en_cours: Option<u64>,
    dernier: u64,
}

impl Differe {
    /// Le numero du nouveau differe, ou `None` si un autre court deja.
    fn commencer(&mut self) -> Option<u64> {
        if self.en_cours.is_some() {
            return None;
        }
        self.dernier += 1;
        self.en_cours = Some(self.dernier);
        self.en_cours
    }

    fn encore(&self, numero: u64) -> bool {
        self.en_cours == Some(numero)
    }

    /// Vrai s'il y avait un differe a arreter.
    fn annuler(&mut self) -> bool {
        self.en_cours.take().is_some()
    }

    /// L'attente `numero` est au bout : vrai si elle doit prendre l'ecran.
    fn finir(&mut self, numero: u64) -> bool {
        self.encore(numero) && self.en_cours.take().is_some()
    }
}

/// Les delais du menu, seuls permis : une page ne lance pas une attente d'une heure.
fn delai_permis(secondes: u32) -> Result<u32, String> {
    match secondes {
        3 | 5 | 10 => Ok(secondes),
        _ => Err(format!("Délai de {secondes} s refusé : 3, 5 ou 10 s seulement.")),
    }
}

const VERROU: &str = "L'état des captures est incohérent.";

/// Dossier des captures, choisi au demarrage (`choisir_dossier`). Cree, et
/// ouvert a la WebView. Ne touche pas la base : sans risque sous `with_conn`.
pub(crate) fn dossier(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let captures = app.state::<EtatCaptures>();
    let dossier = captures.choisi.lock().map_err(|_| VERROU.to_string())?.clone();
    if dossier.as_os_str().is_empty() {
        return Err("Le dossier des captures n'est pas encore choisi.".to_string());
    }
    let mut ouvert = captures.dossier_ouvert.lock().map_err(|_| VERROU.to_string())?;
    if ouvert.as_ref() != Some(&dossier) {
        std::fs::create_dir_all(&dossier).map_err(|e| format!("Création de {} impossible : {e}", dossier.display()))?;
        app.asset_protocol_scope()
            .allow_directory(&dossier, true)
            .map_err(|e| format!("Portée des captures refusée pour {} : {e}", dossier.display()))?;
        *ouvert = Some(dossier.clone());
    }
    Ok(dossier)
}

/// A tenir tant qu'on ecrit dans le dossier des captures, de `dossier()` au
/// dernier fichier : un deplacement attend que l'ecriture finisse.
pub(crate) fn ecriture(app: &tauri::AppHandle) -> Result<RwLockReadGuard<'_, ()>, String> {
    app.state::<EtatCaptures>().inner().ecritures.read().map_err(|_| VERROU.to_string())
}

/// L'exclusivite sur le dossier des captures, sans attendre : refusee pendant
/// un deplacement ou une ecriture. La mise a jour la tient jusqu'a sa sortie.
pub(crate) fn sans_ecriture(app: &tauri::AppHandle) -> Result<RwLockWriteGuard<'_, ()>, String> {
    app.state::<EtatCaptures>().inner().ecritures.try_write().map_err(|e| match e {
        std::sync::TryLockError::WouldBlock => "Le dossier des captures est en cours de déplacement ou d'écriture : réessayez dans un instant.".to_string(),
        std::sync::TryLockError::Poisoned(_) => VERROU.to_string(),
    })
}

/// Dossier des videos (ADR-016) : le dossier Videos de Windows, sous-dossier
/// EquiShot, en production ; `videos` sous le dossier de l'instance en
/// developpement. Cree, et ouvert a la WebView pour la lecture dans la galerie.
pub(crate) fn dossier_videos(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let etat = app.state::<AppState>();
    let dossier = if etat.dev {
        etat.data_dir.join("videos")
    } else {
        app.path().video_dir().map_err(|e| format!("Dossier Vidéos de Windows introuvable : {e}"))?.join("EquiShot")
    };
    let captures = app.state::<EtatCaptures>();
    let mut ouvert = captures.videos_ouvert.lock().map_err(|_| VERROU.to_string())?;
    if ouvert.as_ref() != Some(&dossier) {
        std::fs::create_dir_all(&dossier).map_err(|e| format!("Création de {} impossible : {e}", dossier.display()))?;
        app.asset_protocol_scope()
            .allow_directory(&dossier, true)
            .map_err(|e| format!("Portée des vidéos refusée pour {} : {e}", dossier.display()))?;
        *ouvert = Some(dossier.clone());
    }
    Ok(dossier)
}

/// Les gestes d'image (annoter, recadrer, epingler) refusent une video.
pub(crate) fn exiger_image(capture: &Capture) -> Result<(), String> {
    match capture.duree_ms {
        Some(_) => Err("Une vidéo ne s'annote pas et ne s'épingle pas.".to_string()),
        None => Ok(()),
    }
}

/// Met un fichier au presse-papiers comme fichier (`CF_HDROP`) : un Ctrl+V
/// dans une messagerie ou l'Explorateur le depose. Vide d'abord : arboard 3.6
/// ajoute le fichier sans vider, et l'Explorateur ne voyait que l'ancien contenu.
pub(crate) fn copier_fichier(chemin: &std::path::Path) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut presse_papiers| {
            presse_papiers.clear()?;
            presse_papiers.set().file_list(&[chemin])
        })
        .map_err(|e| format!("Copie du fichier dans le presse-papiers impossible : {e}"))
}

/// Plusieurs captures a la fois (selection de la galerie) : leurs fichiers
/// (`CF_HDROP`) et leurs chemins en texte. Un dossier ou une messagerie prend les
/// fichiers, un champ de texte ou un terminal les chemins.
pub(crate) fn copier_fichiers(chemins: &[PathBuf]) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut presse_papiers| {
            // Le texte vide le presse-papiers avant d'ecrire ; la liste de fichiers s'y ajoute sans vider.
            presse_papiers.set_text(chemins_en_texte(chemins))?;
            presse_papiers.set().file_list(chemins)
        })
        .map_err(|e| format!("Copie des captures dans le presse-papiers impossible : {e}"))
}

/// Les chemins comme « Copier en tant que chemin » de l'Explorateur : entre
/// guillemets, un par ligne.
fn chemins_en_texte(chemins: &[PathBuf]) -> String {
    chemins.iter().map(|c| format!("\"{}\"", c.display())).collect::<Vec<_>>().join("\r\n")
}

/// La capture `id`, lue en base, chemins absolus. Bloquant : hors du thread principal.
pub fn lire(app: &tauri::AppHandle, id: i64) -> Result<Capture, String> {
    let dossier = dossier(app)?;
    app.state::<AppState>().with_conn(|conn| stockage::lire(conn, &dossier, id))
}

/// Au demarrage : le raccourci du reglage est pris si Windows veut, et le
/// dossier des captures s'ouvre a la galerie. Aucun echec n'arrete EquiShot.
pub fn initialiser(app: &tauri::AppHandle) {
    let dev = app.state::<AppState>().dev;
    let lire = |cle| app.state::<AppState>().with_conn(|conn| db::reglage(conn, cle)).unwrap_or_default();
    let bruts = CLES_DECLENCHEURS.map(lire);
    // Le fil du crochet d'abord : une case a la souris s'y pose aussitot.
    souris::demarrer(app);
    if let Ok(mut raccourci) = app.state::<EtatCaptures>().raccourci.lock() {
        raccourci.demarrer(app, bruts.each_ref().map(String::as_str), dev);
        let etat = raccourci.etat();
        for motif in [etat.principal.motif, etat.secondaire.motif, etat.derniere.motif, etat.texte.motif, etat.video.motif, etat.ecran.motif].into_iter().flatten() {
            eprintln!("raccourci indisponible au démarrage : {motif}");
        }
    }
    // Le dossier se cree sur le disque : pas dans le `setup`, sur le thread principal.
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(motif) = dossier(&app) {
            eprintln!("dossier des captures non ouvert : {motif}");
        }
        // Le texte des captures, neuves puis anciennes (ADR-010).
        ocr::demarrer(&app);
    });
}

/// La fenetre EquiShot est detruite : si l'enregistreur des reglages ecoutait,
/// la page n'a rien demonte et la combinaison resterait suspendue. Hors du
/// thread principal, comme `captures_suspendre_raccourci` : `register` attend
/// ce thread, et l'evenement `Destroyed` y tourne.
pub fn reprendre_raccourci(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let etat = app.state::<EtatCaptures>();
        let Ok(mut raccourci) = etat.raccourci.lock() else { return };
        raccourci.suspendre(&app, false);
        let _ = app.emit(raccourci::EVENEMENT_RACCOURCI, &raccourci.etat());
    });
}

/// Dit un echec que personne ne regarde : la frappe du raccourci part d'une
/// autre application, aucune page n'est la pour afficher l'erreur. La pastille
/// le montre.
pub(crate) fn avertir(app: &tauri::AppHandle, texte: &str) {
    eprintln!("capture : {texte}");
    let _ = app.emit_to(crate::pastille::ETIQUETTE, EVENEMENT_ERREUR, texte.to_string());
}

/// Comme `avertir`, pour une bonne nouvelle : la pastille la dit au ton neutre.
pub(crate) fn informer(app: &tauri::AppHandle, texte: &str) {
    let _ = app.emit_to(crate::pastille::ETIQUETTE, EVENEMENT_INFO, texte.to_string());
}

/// Appele par le gestionnaire unique du greffon de raccourci, sur l'appui.
pub fn sur_raccourci(app: &tauri::AppHandle, raccourci: &tauri_plugin_global_shortcut::Shortcut) {
    // `try_lock`, jamais `lock` : ce gestionnaire tourne sur le thread principal,
    // et le reglage du raccourci tient ce verrou pendant qu'il attend le thread
    // principal (`register`, `unregister`). Attendre ici les bloquerait tous les
    // deux. Une frappe pendant un reglage en cours est ignoree.
    let case = app.state::<EtatCaptures>().raccourci.try_lock().ok().and_then(|r| r.case_de(raccourci));
    if let Some(case) = case {
        declencher(app, case);
    }
}

/// L'action d'une case de raccourci, clavier ou souris : la derniere zone se
/// reprend, le texte ouvre la selection en mode texte (ADR-010), l'ecran entier
/// se prend sans selection (0.21), les deux
/// autres ouvrent la selection.
pub fn declencher(app: &tauri::AppHandle, case: usize) {
    match case {
        raccourci::DERNIERE => lancer_derniere_zone(app),
        raccourci::TEXTE => lancer_selection_texte(app),
        // Pendant une video, `ouvrir_selection` l'arrete au lieu d'ouvrir.
        raccourci::VIDEO => lancer_selection_video(app, 0),
        raccourci::ECRAN => lancer_ecran_entier(app),
        _ => lancer_selection(app),
    }
}

/// Ouvre la selection depuis un geste sans page pour attendre la reponse :
/// le raccourci, le menu de la zone de notification.
pub fn lancer_selection(app: &tauri::AppHandle) {
    ouvrir_selection_detachee(app, Mode::Image, 0);
}

/// La selection en mode texte : la zone figee est lue, son texte copie.
pub fn lancer_selection_texte(app: &tauri::AppHandle) {
    ouvrir_selection_detachee(app, Mode::Texte, 0);
}

/// La selection en mode video (ADR-016) : la zone tracee est filmee. `devant` : le
/// premier plan releve avant un menu (0 : celui de maintenant).
pub fn lancer_selection_video(app: &tauri::AppHandle, devant: isize) {
    ouvrir_selection_detachee(app, Mode::Video, devant);
}

fn ouvrir_selection_detachee(app: &tauri::AppHandle, mode: Mode, devant: isize) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(motif) = ouvrir_selection_devant(app.clone(), mode, devant).await {
            avertir(&app, &motif);
        }
    });
}

/// L'ecran entier depuis le menu de la zone de notification.
pub fn lancer_ecran_entier(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(motif) = ecran_entier(app.clone()).await {
            avertir(&app, &motif);
        }
    });
}

/// Titre et application de la fenetre au premier plan, avant que la fenetre
/// de selection ne le prenne. Vides si Windows ne les donne pas.
pub(crate) fn fenetre_au_premier_plan(app: &tauri::AppHandle, devant: isize) -> (String, String) {
    // `xcap` ecarte les fenetres du processus courant (`is_valid_window`, lu
    // dans sa source) : une fenetre d'EquiShot au premier plan se reconnait a
    // sa poignee.
    let la_notre = app.webview_windows().into_values().find(|f| devant != 0 && f.hwnd().is_ok_and(|poignee| poignee.0 as isize == devant));
    if let Some(fenetre) = la_notre {
        return (fenetre.title().unwrap_or_default(), "EquiShot".to_string());
    }
    let Ok(fenetres) = xcap::Window::all() else {
        return Default::default();
    };
    // Par sa poignee, pas par `is_focused` : releve avant un menu, `devant` n'a plus le
    // premier plan (muda l'a donne a la pastille). `id()` est la poignee chez xcap.
    fenetres
        .into_iter()
        .find(|f| devant != 0 && f.id().is_ok_and(|id| id == devant as u32))
        .map(|f| (f.title().unwrap_or_default(), f.app_name().unwrap_or_default()))
        .unwrap_or_default()
}

/// L'ecran sous le curseur, tel que Tauri le donne : coin, taille physique,
/// echelle, nom. Et la fenetre au premier plan a cet instant, ou `devant` s'il a ete
/// releve avant un menu (muda donne le premier plan a la pastille).
fn reperer(app: &tauri::AppHandle, devant: isize) -> Result<((i32, i32), (u32, u32), Origine, isize), String> {
    let devant = if devant == 0 { fenetres::premier_plan() } else { devant };
    let (fenetre, application) = fenetre_au_premier_plan(app, devant);
    let curseur = app.cursor_position().map_err(|e| format!("Position du curseur illisible : {e}"))?;
    let ecran = app
        .monitor_from_point(curseur.x, curseur.y)
        .map_err(|e| format!("Écrans illisibles : {e}"))?
        .or(app.primary_monitor().ok().flatten())
        .ok_or("Aucun écran sous le curseur.")?;
    let origine = Origine {
        ecran: ecran.name().cloned().unwrap_or_default(),
        echelle: ecran.scale_factor(),
        fenetre,
        application,
    };
    Ok(((ecran.position().x, ecran.position().y), (ecran.size().width, ecran.size().height), origine, devant))
}

/// Prend l'ecran dont le coin est `position`. Bloquant : hors du thread
/// principal. La pastille et la selection, protegees, n'y sont pas.
fn prendre_ecran(position: (i32, i32)) -> Result<RgbaImage, String> {
    let ecran = xcap::Monitor::from_point(position.0 + 1, position.1 + 1).map_err(|e| format!("Écran introuvable : {e}"))?;
    ecran.capture_image().map_err(|e| format!("Capture de l'écran impossible : {e}"))
}

fn liberer(app: &tauri::AppHandle) {
    let etat = app.state::<EtatCaptures>();
    if let Ok(mut selection) = etat.selection.lock() {
        *selection = None;
    }
    if let Ok(mut en_cours) = etat.en_cours.lock() {
        *en_cours = None;
    };
}

/// Passe ce delai sans s'etre montree, une selection est tenue pour perdue.
const SELECTION_PERDUE: Duration = Duration::from_secs(5);

/// Ouvre la fenetre de selection sur l'ecran sous le curseur. Sans effet si une
/// selection est deja ouverte.
pub async fn ouvrir_selection(app: tauri::AppHandle, mode: Mode) -> Result<(), String> {
    ouvrir_selection_devant(app, mode, 0).await
}

/// `ouvrir_selection` avec le premier plan releve avant un menu (0 : celui de maintenant).
pub async fn ouvrir_selection_devant(app: tauri::AppHandle, mode: Mode, devant: isize) -> Result<(), String> {
    // Pendant une video, la selection ouverte n'est qu'un contour : le
    // raccourci « Filmer » arrete, les autres attendent la fin (ADR-016).
    if crate::video::en_cours(&app) {
        if mode == Mode::Video {
            crate::video::arreter_detache(&app);
            return Ok(());
        }
        return Err("Un enregistrement vidéo est en cours : arrêtez-le d'abord.".to_string());
    }
    // L'instant de la frappe en cours, ou `None` si la voie est libre et prise ici.
    let deja = {
        let etat = app.state::<EtatCaptures>();
        let mut en_cours = etat.en_cours.lock().map_err(|_| VERROU.to_string())?;
        let deja = *en_cours;
        if deja.is_none() {
            *en_cours = Some(Instant::now());
        }
        deja
    };
    if let Some(depuis) = deja {
        // Une selection dont la page n'a jamais rendu la main resterait
        // invisible et tiendrait le raccourci : passe le delai, elle est
        // detruite, la frappe suivante rouvre.
        let perdue = depuis.elapsed() >= SELECTION_PERDUE;
        match app.get_webview_window(ETIQUETTE_SELECTION) {
            Some(fenetre) if fenetre.is_visible().unwrap_or(false) => {
                // Le raccourci frappe de nouveau arrete aussi une capture defilante.
                arreter_defilement(&app);
                changer_mode(&app, mode);
                let _ = fenetre.set_focus();
            }
            Some(fenetre) if perdue => {
                let _ = fenetre.destroy();
            }
            _ => {}
        }
        return Ok(());
    }
    let ouverture = ouvrir(app.clone(), mode, devant).await;
    if ouverture.is_err() {
        // Une fenetre creee puis mal placee ne reste pas, invisible, derriere l'erreur.
        match app.get_webview_window(ETIQUETTE_SELECTION) {
            Some(fenetre) => {
                let _ = fenetre.destroy();
            }
            None => liberer(&app),
        }
    }
    ouverture
}

/// L'autre raccourci frappe pendant une selection ouverte : elle prend son mode
/// tant que rien n'est fige. Une zone figee, peut-etre annotee, n'est pas lue
/// a la place : la selection garde son mode et prend seulement le premier plan.
fn changer_mode(app: &tauri::AppHandle, mode: Mode) {
    let etat = app.state::<EtatCaptures>();
    let Ok(mut selection) = etat.selection.lock() else { return };
    if let Some(s) = selection.as_mut().filter(|s| s.prise.is_none() && s.mode != mode) {
        s.mode = mode;
        let _ = app.emit_to(ETIQUETTE_SELECTION, EVENEMENT_MODE, mode);
    }
}

async fn ouvrir(app: tauri::AppHandle, mode: Mode, devant: isize) -> Result<(), String> {
    let debut = Instant::now();
    crate::vignettes::preparer(&app);
    // Le premier plan est releve avant que la selection ne le prenne.
    let (position, taille, origine, devant) = hors_thread_principal(app.clone(), move |app| reperer(&app, devant)).await?;
    *app.state::<EtatCaptures>().selection.lock().map_err(|_| VERROU.to_string())? =
        Some(Selection {
            origine,
            devant,
            position,
            taille,
            debut,
            delai_ms: None,
            prise: None,
            trace: 0,
            enregistree: None,
            fenetres: Vec::new(),
            origine_prise: None,
            zone_prise: None,
            mode,
        });

    // Invisible tant que la page n'est pas prete : `captures_selection_prete` la montre.
    let fenetre = tauri::WebviewWindowBuilder::new(&app, ETIQUETTE_SELECTION, tauri::WebviewUrl::App("selection.html".into()))
        .title("EquiShot, capture")
        .decorations(false)
        .transparent(true)
        .content_protected(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .visible(false)
        .build()
        .map_err(|e| format!("Fenêtre de sélection impossible à ouvrir : {e}"))?;
    fenetres::sans_menu_alt(&fenetre);
    fenetres::hors_occlusion(&fenetre);
    let poignee = app.clone();
    fenetre.on_window_event(move |evenement| {
        if let tauri::WindowEvent::Destroyed = evenement {
            liberer(&poignee);
        }
    });
    // En pixels physiques, la position avant la taille : changer d'ecran peut
    // changer l'echelle, et Windows retaille alors la fenetre.
    fenetre
        .set_position(tauri::PhysicalPosition::new(position.0, position.1))
        .and_then(|_| fenetre.set_size(tauri::PhysicalSize::new(taille.0, taille.1)))
        .map_err(|e| format!("Fenêtre de sélection impossible à placer : {e}"))
}

/// La fenetre qui avait le premier plan avant la selection, 0 sans selection.
/// La video la garde pour le lui rendre au lancement (ADR-017).
pub(crate) fn devant(app: &tauri::AppHandle) -> isize {
    app.state::<EtatCaptures>().selection.lock().ok().and_then(|s| s.as_ref().map(|s| s.devant)).unwrap_or(0)
}

/// Ferme la selection et rend le premier plan a la fenetre qui l'avait.
pub(crate) fn fermer_selection(app: &tauri::AppHandle) {
    let devant = devant(app);
    // Avant la fermeture : EquiShot a encore le premier plan, Windows le laisse
    // donc le donner.
    fenetres::rendre_premier_plan(devant);
    match app.get_webview_window(ETIQUETTE_SELECTION) {
        Some(fenetre) => {
            fenetres::avant_fermeture(&fenetre);
            let _ = fenetre.close();
        }
        None => liberer(app),
    }
}

/// BMP 24 bits, lignes de bas en haut. La zone prise part a la page sans
/// compression : encoder en PNG prendrait plus que la capture.
fn bmp(image: &RgbaImage) -> Vec<u8> {
    let (largeur, hauteur) = image.dimensions();
    let ligne = (largeur as usize * 3 + 3) & !3;
    let pixels = ligne * hauteur as usize;
    let mut sortie = Vec::with_capacity(54 + pixels);
    sortie.extend_from_slice(b"BM");
    sortie.extend_from_slice(&((54 + pixels) as u32).to_le_bytes());
    sortie.extend_from_slice(&0u32.to_le_bytes());
    sortie.extend_from_slice(&54u32.to_le_bytes());
    sortie.extend_from_slice(&40u32.to_le_bytes());
    sortie.extend_from_slice(&(largeur as i32).to_le_bytes());
    sortie.extend_from_slice(&(hauteur as i32).to_le_bytes());
    sortie.extend_from_slice(&1u16.to_le_bytes());
    sortie.extend_from_slice(&24u16.to_le_bytes());
    sortie.extend_from_slice(&0u32.to_le_bytes());
    sortie.extend_from_slice(&(pixels as u32).to_le_bytes());
    sortie.extend_from_slice(&[0u8; 16]);
    let brut = image.as_raw();
    let octets = largeur as usize * 4;
    for y in (0..hauteur as usize).rev() {
        for pixel in brut[y * octets..(y + 1) * octets].chunks_exact(4) {
            sortie.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
        sortie.resize(sortie.len() + ligne - largeur as usize * 3, 0);
    }
    sortie
}

pub(crate) fn copier_image(image: &RgbaImage) -> Result<(), String> {
    let (largeur, hauteur) = image.dimensions();
    arboard::Clipboard::new()
        .and_then(|mut presse_papiers| {
            presse_papiers.set_image(arboard::ImageData {
                width: largeur as usize,
                height: hauteur as usize,
                bytes: std::borrow::Cow::Borrowed(image.as_raw()),
            })
        })
        .map_err(|e| format!("Copie dans le presse-papiers impossible : {e}"))
}

/// Le rendu aplati par la page, en PNG encode en base64. Il a exactement la taille attendue (l'original, ou l'original oriente, ADR-018), sinon la page et Rust n'ont pas recadre pareil.
fn decoder_rendu(rendu: &str, taille: (u32, u32)) -> Result<RgbaImage, String> {
    let octets = base64::engine::general_purpose::STANDARD
        .decode(rendu.trim())
        .map_err(|e| format!("Image annotée illisible : {e}"))?;
    let image = image::load_from_memory_with_format(&octets, image::ImageFormat::Png)
        .map_err(|e| format!("Image annotée illisible : {e}"))?
        .to_rgba8();
    if image.dimensions() != taille {
        return Err(format!(
            "L'image annotée mesure {} x {}, l'image attendue {} x {} : capture refusée.",
            image.width(),
            image.height(),
            taille.0,
            taille.1
        ));
    }
    Ok(image)
}

/// Ecrit une capture neuve : original, rendu, vignette, ligne, puis
/// presse-papiers. La copie vient en dernier et son echec ne perd pas la
/// capture : il est rendu a cote d'elle, pour etre dit. Une ligne qui ne
/// s'ecrit pas emporte ses fichiers : sans elle, rien ne les montrerait.
/// `vignette` : faux pour la zone gardee au relachement, dont la carte vient a
/// la fermeture de la selection (ADR-004). `texte` : le texte deja lu (mode
/// « Texte », ADR-010), ecrit avec la ligne et copie a la place de l'image ;
/// `None`, l'image est copiee et le fil de lecture lira son texte.
fn finir(
    app: &tauri::AppHandle,
    origine: &RgbaImage,
    meta: &Origine,
    formes: &serde_json::Value,
    rendu: Option<&str>,
    vignette: bool,
    texte: Option<&str>,
) -> Result<(Capture, Option<String>), String> {
    let formes = stockage::valider_formes(formes)?;
    let rendu = rendu.map(|r| decoder_rendu(r, origine.dimensions())).transpose()?;
    let rendu = rendu.as_ref().unwrap_or(origine);
    let _ecriture = ecriture(app)?;
    let dossier = dossier(app)?;
    let quand = chrono::Local::now();
    let fichier = stockage::nom_fichier(&quand);
    // Si l'original ne s'ecrit pas, rien n'est a nettoyer : surtout pas le
    // fichier d'une autre capture qui porterait ce nom.
    stockage::ecrire_origine(&dossier, &fichier, origine)?;
    let etat = app.state::<AppState>();
    let ligne = stockage::ecrire_rendu(&dossier, &fichier, rendu)
        .and_then(|_| etat.with_conn(|conn| stockage::inserer(conn, &quand, &fichier, origine.dimensions(), meta, &formes, texte)));
    let id = match ligne {
        Ok(id) => id,
        Err(motif) => {
            stockage::effacer_fichiers(&dossier, &fichier);
            return Err(motif);
        }
    };
    // La ligne est validee : a partir d'ici, plus rien n'efface ses fichiers.
    if texte.is_none() {
        ocr::reveiller();
    }
    let capture = etat.with_conn(|conn| stockage::lire(conn, &dossier, id))?;
    // La carte avant la copie : copier une grande image prend plus d'une seconde
    // (`arboard` l'encode aussi en PNG), la carte n'a pas a l'attendre.
    if vignette {
        crate::vignettes::ajouter(app, capture.clone());
    }
    let copie = match texte {
        // Rien de reconnu : le presse-papiers garde ce qu'il avait.
        Some(texte) if texte.trim().is_empty() => Ok(()),
        Some(texte) => copier_texte(texte),
        None => copier_image(rendu),
    };
    Ok((capture, copie.err()))
}

fn copier_texte(texte: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut presse_papiers| presse_papiers.set_text(texte))
        .map_err(|e| format!("Copie dans le presse-papiers impossible : {e}"))
}

/// L'image de depart d'une reedition : le rendu aplati par la page, controle contre la
/// taille orientee, sinon l'original oriente (ADR-018). Les deux chemins donnent la meme image.
fn image_a_rendre(rendu: Option<&str>, origine: &std::path::Path, taille_origine: (u32, u32), orientation: u8) -> Result<RgbaImage, String> {
    match rendu {
        Some(rendu) => decoder_rendu(rendu, stockage::taille_orientee(taille_origine, orientation)),
        None => Ok(stockage::orienter(&stockage::lire_png(origine)?, orientation)),
    }
}

/// Remplace les formes, l'orientation et le recadrage d'une capture et refait son rendu,
/// rendu aussi a l'appelant. `rendu` absent : plus aucune forme, le rendu repart de
/// l'original oriente. Le rendu de la page a la taille orientee : Rust le rogne ensuite
/// (ADR-007, dans l'espace oriente par l'ADR-018), puis le pose sur son fond (ADR-012).
fn modifier(
    app: &tauri::AppHandle,
    id: i64,
    formes: &serde_json::Value,
    orientation: &serde_json::Value,
    recadrage: &serde_json::Value,
    fond: &serde_json::Value,
    rendu: Option<&str>,
) -> Result<(Capture, RgbaImage), String> {
    let formes = stockage::valider_formes(formes)?;
    let orientation = stockage::valider_orientation(orientation)?;
    let _ecriture = ecriture(app)?;
    let dossier = dossier(app)?;
    let etat = app.state::<AppState>();
    let avant = etat.with_conn(|conn| stockage::lire(conn, &dossier, id))?;
    exiger_image(&avant)?;
    let fichier = PathBuf::from(&avant.origine)
        .strip_prefix(&dossier)
        .map(|relatif| relatif.to_string_lossy().into_owned())
        .map_err(|_| format!("La capture {id} n'est pas dans le dossier des captures."))?;
    // Le cadre vit dans l'espace oriente (ADR-018) : un pivot echange les cotes de l'image.
    let recadrage = stockage::valider_recadrage(recadrage, stockage::taille_orientee((avant.largeur, avant.hauteur), orientation))?;
    let fond = stockage::valider_fond(fond)?;
    let image = image_a_rendre(rendu, std::path::Path::new(&avant.origine), (avant.largeur, avant.hauteur), orientation)?;
    let image = stockage::habiller(&stockage::recadrer(&image, recadrage), fond.as_ref());
    stockage::ecrire_rendu(&dossier, &fichier, &image)?;
    let capture = etat.with_conn(|conn| {
        stockage::remplacer_formes(conn, id, &formes, orientation, recadrage, fond.as_ref(), &chrono::Local::now())?;
        stockage::lire(conn, &dossier, id)
    })?;
    Ok((capture, image))
}

/// Met une capture a la corbeille de son dossier et retire sa ligne. Une video,
/// au chemin absolu, va a celle du dossier des videos : sur le meme disque, un
/// simple renommage suffit.
fn supprimer(app: &tauri::AppHandle, id: i64) -> Result<(), String> {
    let etat = app.state::<AppState>();
    // Les fichiers bougent hors du verrou de la base.
    let fichier = etat.with_conn(|conn| stockage::fichier_de(conn, id))?;
    let _ecriture = ecriture(app)?;
    let dossier = if std::path::Path::new(&fichier).is_absolute() { dossier_videos(app)? } else { dossier(app)? };
    stockage::vers_corbeille(&dossier, &fichier)?;
    etat.with_conn(|conn| stockage::retirer_ligne(conn, id))
}

/// La capture deja ecrite de la selection ouverte, s'il y en a une.
fn enregistree(app: &tauri::AppHandle) -> Result<Option<i64>, String> {
    let etat = app.state::<EtatCaptures>();
    let selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
    Ok(selection.as_ref().and_then(|s| s.enregistree))
}

/// La capture est enregistree mais n'est pas dans le presse-papiers : cela se
/// dit, sans la perdre.
fn dire_copie_manquee(app: &tauri::AppHandle, avertissement: Option<String>) {
    if let Some(motif) = avertissement {
        avertir(app, &format!("Capture enregistrée, mais pas copiée : {motif}"));
    }
}

pub(crate) fn annoncer(app: &tauri::AppHandle) {
    let _ = app.emit(EVENEMENT_CHANGEE, ());
}

/// L'ecran entier sous le curseur, enregistre sans passer par la selection.
async fn ecran_entier(app: tauri::AppHandle) -> Result<Capture, String> {
    crate::vignettes::preparer(&app);
    let (capture, avertissement) = hors_thread_principal(app.clone(), move |app| {
        let (position, _, origine, _) = reperer(&app, 0)?;
        let image = prendre_ecran(position)?;
        finir(&app, &image, &origine, &serde_json::json!([]), None, true, None)
    })
    .await?;
    dire_copie_manquee(&app, avertissement);
    annoncer(&app);
    Ok(capture)
}

/// Garde la zone figee pour la reprendre (ADR-009). Un echec ne retire rien a
/// la capture deja ecrite : il part a la console.
fn garder_derniere_zone(app: &tauri::AppHandle, derniere: &stockage::DerniereZone) {
    let ecrite = serde_json::to_string(derniere)
        .map_err(|e| e.to_string())
        .and_then(|json| app.state::<AppState>().with_conn(|conn| db::regler(conn, CLE_DERNIERE_ZONE, &json)));
    match ecrite {
        Ok(()) => crate::pastille::derniere_zone_prete(app),
        Err(motif) => eprintln!("dernière zone non gardée : {motif}"),
    }
}

/// La derniere zone gardee, si elle se relit. Bloquant : hors du thread principal.
pub fn derniere_zone_gardee(app: &tauri::AppHandle) -> Option<stockage::DerniereZone> {
    let brut = app.state::<AppState>().with_conn(|conn| db::reglage(conn, CLE_DERNIERE_ZONE)).ok()?;
    serde_json::from_str(&brut).ok()
}

/// Reprend la derniere zone sur l'ecran vivant, sans ouvrir la selection, et
/// l'enregistre comme un ecran entier (ADR-009).
async fn derniere_zone(app: tauri::AppHandle) -> Result<Capture, String> {
    crate::vignettes::preparer(&app);
    let (capture, avertissement) = hors_thread_principal(app.clone(), move |app| {
        let gardee = derniere_zone_gardee(&app).ok_or("Aucune zone n'a encore été prise.")?;
        let ecrans = xcap::Monitor::all().map_err(|e| format!("Écrans illisibles : {e}"))?;
        let reperes: Vec<_> = ecrans
            .iter()
            .map(|e| ((e.x().unwrap_or(i32::MIN), e.y().unwrap_or(i32::MIN)), (e.width().unwrap_or(0), e.height().unwrap_or(0))))
            .collect();
        let rang = stockage::ecran_de_la_zone(&reperes, &gardee)
            .ok_or("L'écran de la dernière zone a changé (débranché, déplacé ou d'une autre taille) : rien n'est pris.")?;
        let image = ecrans[rang]
            .capture_region(gardee.x, gardee.y, gardee.largeur, gardee.hauteur)
            .map_err(|e| format!("Capture de la dernière zone impossible : {e}"))?;
        let (fenetre, application) = fenetre_au_premier_plan(&app, fenetres::premier_plan());
        let origine = Origine { ecran: gardee.ecran, echelle: gardee.echelle, fenetre, application };
        finir(&app, &image, &origine, &serde_json::json!([]), None, true, None)
    })
    .await?;
    dire_copie_manquee(&app, avertissement);
    annoncer(&app);
    Ok(capture)
}

/// La derniere zone depuis le menu ou sa case de raccourci.
pub fn lancer_derniere_zone(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(motif) = derniere_zone(app.clone()).await {
            avertir(&app, &motif);
        }
    });
}

/// Lance l'ecran entier differe : refuse si un autre court.
fn differer(app: &tauri::AppHandle, secondes: u32) -> Result<(), String> {
    let secondes = delai_permis(secondes)?;
    let numero = app
        .state::<EtatCaptures>()
        .differe
        .lock()
        .map_err(|_| VERROU.to_string())?
        .commencer()
        .ok_or("Une capture différée est déjà en cours.")?;
    tauri::async_runtime::spawn(attendre(app.clone(), numero, secondes));
    Ok(())
}

/// Le compte a rebours : chaque seconde est dite a la pastille et a l'icone de
/// notification, puis l'ecran sous le curseur est pris, s'il n'a pas ete annule.
async fn attendre(app: tauri::AppHandle, numero: u64, secondes: u32) {
    let encore = |app: &tauri::AppHandle| app.state::<EtatCaptures>().differe.lock().is_ok_and(|d| d.encore(numero));
    for reste in (1..=secondes).rev() {
        if !encore(&app) {
            return;
        }
        suivre_differe(&app, Some(reste));
        let _ = tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_secs(1))).await;
    }
    let a_prendre = app.state::<EtatCaptures>().differe.lock().is_ok_and(|mut d| d.finir(numero));
    if !a_prendre {
        return;
    }
    suivre_differe(&app, None);
    if let Err(motif) = ecran_entier(app.clone()).await {
        avertir(&app, &motif);
    }
}

fn suivre_differe(app: &tauri::AppHandle, reste: Option<u32>) {
    let _ = app.emit(EVENEMENT_DIFFERE, reste);
    crate::pastille::suivre_differe(app, reste);
}

/// Arrete le differe en cours, s'il y en a un.
pub fn annuler_differe(app: &tauri::AppHandle) {
    let arrete = app.state::<EtatCaptures>().differe.lock().is_ok_and(|mut d| d.annuler());
    if arrete {
        suivre_differe(app, None);
    }
}

/// Le differe depuis le menu de la zone de notification.
pub fn lancer_differe(app: &tauri::AppHandle, secondes: u32) {
    if let Err(motif) = differer(app, secondes) {
        avertir(app, &motif);
    }
}

#[tauri::command]
pub async fn captures_differer(app: tauri::AppHandle, secondes: u32) -> Result<(), String> {
    differer(&app, secondes)
}

#[tauri::command]
pub async fn captures_differe_annuler(app: tauri::AppHandle) -> Result<(), String> {
    annuler_differe(&app);
    Ok(())
}

// ---------------------------------------------------------------------------
// Commandes : raccourci
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn captures_etat_raccourci(etat: tauri::State<'_, EtatCaptures>) -> Result<EtatRaccourci, String> {
    Ok(etat.raccourci.lock().map_err(|_| VERROU.to_string())?.etat())
}

/// L'enregistreur des reglages ecoute (`true`) ou a renonce (`false`).
#[tauri::command]
pub async fn captures_suspendre_raccourci(
    app: tauri::AppHandle,
    etat: tauri::State<'_, EtatCaptures>,
    suspendu: bool,
) -> Result<EtatRaccourci, String> {
    let mut raccourci = etat.raccourci.lock().map_err(|_| VERROU.to_string())?;
    raccourci.suspendre(&app, suspendu);
    Ok(raccourci.etat())
}

/// Regle une case (`principal` ou `secondaire`) : `type` vaut `clavier`,
/// `souris` ou `aucun`.
#[tauri::command]
pub async fn captures_regler_raccourci(
    app: tauri::AppHandle,
    etat: tauri::State<'_, EtatCaptures>,
    case: String,
    r#type: String,
    combinaison: String,
) -> Result<EtatRaccourci, String> {
    let i = raccourci::NOMS.iter().position(|nom| *nom == case).ok_or(format!("Case de raccourci inconnue : {case}"))?;
    let cle = CLES_DECLENCHEURS[i];
    let saisie = match r#type.as_str() {
        "clavier" => Declencheur::Clavier { combinaison },
        "souris" => Declencheur::Souris { combinaison },
        "aucun" => Declencheur::Aucun,
        autre => return Err(format!("Type de raccourci inconnu : {autre}")),
    };
    let (avant, reglage, apres) = {
        let mut raccourci = etat.raccourci.lock().map_err(|_| VERROU.to_string())?;
        let avant = raccourci.declencheur(i);
        let reglage = raccourci.regler(&app, i, saisie);
        (avant, reglage, raccourci.etat())
    };
    let json = match reglage {
        Ok(json) => json,
        Err(motif) => {
            // L'enregistreur avait suspendu les cases : elles sont remises, les pages suivent.
            let _ = app.emit(raccourci::EVENEMENT_RACCOURCI, &apres);
            return Err(motif);
        }
    };
    if let Err(motif) = app.state::<AppState>().with_conn(|conn| db::regler(conn, cle, &json)) {
        // Windows tient deja le nouveau declencheur : sans sa valeur en base, il
        // disparaitrait a la relance. L'ancien est remis.
        let _ = etat.raccourci.lock().map_err(|_| VERROU.to_string())?.regler(&app, i, avant);
        return Err(format!("Le raccourci n'a pas pu être enregistré, l'ancien est remis : {motif}"));
    }
    let _ = app.emit(raccourci::EVENEMENT_RACCOURCI, &apres);
    Ok(apres)
}

/// Sonde du crochet souris, instance dev seulement : `etat`, `retirer` (comme
/// Windows apres un delai depasse, sans le dire au fil), `oublier` (remet la
/// duree maximale a zero), `tenir` (occupe le thread principal 3 s).
#[tauri::command]
pub async fn captures_sonde_souris(app: tauri::AppHandle, action: String) -> Result<souris::Sonde, String> {
    if !app.state::<AppState>().dev {
        return Err("Sonde réservée à l'instance de développement.".to_string());
    }
    match action.as_str() {
        "etat" => {}
        "retirer" => souris::retirer_sans_dire(),
        "oublier" => souris::oublier_duree(),
        "tenir" => app.run_on_main_thread(|| std::thread::sleep(Duration::from_secs(3))).map_err(|e| e.to_string())?,
        autre => return Err(format!("Action de sonde inconnue : {autre}")),
    }
    Ok(souris::sonde())
}

// ---------------------------------------------------------------------------
// Commandes : selection
// ---------------------------------------------------------------------------

/// Bouton « Zone » de la pastille.
/// « Vidéo » de la pastille (ADR-016).
#[tauri::command]
pub async fn captures_ouvrir_selection_video(app: tauri::AppHandle) -> Result<(), String> {
    ouvrir_selection(app, Mode::Video).await
}

#[tauri::command]
pub async fn captures_ouvrir_selection(app: tauri::AppHandle) -> Result<(), String> {
    ouvrir_selection(app, Mode::Image).await
}

/// Bouton « Texte » de la pastille (ADR-010).
#[tauri::command]
pub async fn captures_ouvrir_selection_texte(app: tauri::AppHandle) -> Result<(), String> {
    ouvrir_selection(app, Mode::Texte).await
}

/// Mode « Texte » (ADR-010) : la zone figee est lue, la selection se ferme
/// d'abord pour rendre l'ecran, puis la capture s'ecrit avec son texte, et le
/// texte, pas l'image, part au presse-papiers.
#[tauri::command]
pub async fn captures_selection_texte(app: tauri::AppHandle, trace: u32) -> Result<(), String> {
    let (image, meta, derniere) = {
        let etat = app.state::<EtatCaptures>();
        let selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
        let s = selection.as_ref().ok_or("La sélection est fermée.")?;
        let image = s.prise.clone().filter(|_| trace == s.trace).ok_or("Un tracé plus récent a été pris.")?;
        let derniere = s.zone_prise.map(|z| stockage::DerniereZone {
            x: z.x,
            y: z.y,
            largeur: z.largeur,
            hauteur: z.hauteur,
            position: s.position,
            taille: s.taille,
            ecran: s.origine.ecran.clone(),
            echelle: s.origine.echelle,
        });
        (image, s.origine_de_la_prise(), derniere)
    };
    fermer_selection(&app);
    let lu = hors_thread_principal(app.clone(), move |app| {
        let lu = if lecture_ratee_pour_le_parcours(&app) { Err("échec simulé par le parcours.".to_string()) } else { ocr::lire_texte(&image) };
        let (texte, echec) = texte_ou_vide(lu);
        let (_, avertissement) = finir(&app, &image, &meta, &serde_json::json!([]), None, true, Some(&texte))?;
        if let Some(derniere) = derniere {
            garder_derniere_zone(&app, &derniere);
        }
        Ok((texte.lines().filter(|l| !l.trim().is_empty()).count(), echec, avertissement))
    })
    .await;
    // La page est deja fermee : un echec se dit sur la pastille.
    let (lignes, echec, avertissement) = lu.inspect_err(|motif| avertir(&app, motif))?;
    match (echec, avertissement, lignes) {
        (Some(motif), _, _) => avertir(&app, &format!("Zone enregistrée, mais texte non lu : {motif}")),
        (None, Some(motif), _) => avertir(&app, &format!("Texte lu, mais pas copié : {motif}")),
        (None, None, 0) => informer(&app, "Aucun texte reconnu"),
        (None, None, 1) => informer(&app, "Texte copié (1 ligne)"),
        (None, None, n) => informer(&app, &format!("Texte copié ({n} lignes)")),
    }
    annoncer(&app);
    Ok(())
}

/// Parcours e2e seulement : dans l'exe de debug de l'instance dev, un fichier
/// `essai-ocr-echec` dans le dossier de l'instance fait echouer la lecture du
/// mode « Texte ». Aucune langue OCR ne se retire sans toucher a Windows.
// ponytail: une couture de test, absente de l'exe de release.
fn lecture_ratee_pour_le_parcours(app: &tauri::AppHandle) -> bool {
    let etat = app.state::<AppState>();
    cfg!(debug_assertions) && etat.dev && etat.data_dir.join("essai-ocr-echec").exists()
}

/// Le texte lu, ou un texte vide et le motif quand la lecture echoue : la zone
/// tracee s'ecrit quand meme, l'image permet de verifier (ADR-010).
fn texte_ou_vide(lu: Result<String, String>) -> (String, Option<String>) {
    match lu {
        Ok(texte) => (texte, None),
        Err(motif) => (String::new(), Some(motif)),
    }
}

/// « Copier le texte » de la galerie (ADR-010).
#[tauri::command]
pub async fn captures_copier_texte(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    hors_thread_principal(app, move |app| {
        let texte = app.state::<AppState>().with_conn(|conn| stockage::texte_de(conn, id))?;
        match texte {
            None => Err("Le texte de cette capture est en cours de lecture.".to_string()),
            Some(texte) if texte.trim().is_empty() => Err("Aucun texte reconnu dans cette capture.".to_string()),
            Some(texte) => copier_texte(&texte),
        }
    })
    .await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InfosSelection {
    /// Taille de l'ecran couvert, en pixels physiques.
    largeur: u32,
    hauteur: u32,
    echelle: f64,
    /// Delai entre le declenchement et la selection montree, une fois montree.
    delai_ms: Option<u64>,
    /// Image, texte (ADR-010) ou video (ADR-016).
    mode: Mode,
}

#[tauri::command]
pub async fn captures_selection_infos(etat: tauri::State<'_, EtatCaptures>) -> Result<InfosSelection, String> {
    let selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
    let s = selection.as_ref().ok_or("Aucune capture en cours.")?;
    Ok(InfosSelection { largeur: s.taille.0, hauteur: s.taille.1, echelle: s.origine.echelle, delai_ms: s.delai_ms, mode: s.mode })
}

/// La page est prete : la fenetre se montre et prend le clavier. La croix
/// vient seule, sans mouvement de souris (mesure 1 d, `docs/TESTS.md`).
#[tauri::command]
pub async fn captures_selection_prete(app: tauri::AppHandle, etat: tauri::State<'_, EtatCaptures>) -> Result<(), String> {
    let fenetre = app.get_webview_window(ETIQUETTE_SELECTION).ok_or("La fenêtre de sélection est fermée.")?;
    fenetre.show().and_then(|_| fenetre.set_focus()).map_err(|e| format!("Fenêtre de sélection impossible à montrer : {e}"))?;
    if let Some(s) = etat.selection.lock().map_err(|_| VERROU.to_string())?.as_mut() {
        s.delai_ms = Some(s.debut.elapsed().as_millis() as u64);
    }
    Ok(())
}

/// L'ecran et le curseur de la selection ouverte, en pixels physiques du bureau.
fn ecran_et_curseur(app: &tauri::AppHandle) -> Result<((i32, i32), (u32, u32), (i32, i32)), String> {
    let (position, taille) = {
        let etat = app.state::<EtatCaptures>();
        let selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
        let s = selection.as_ref().ok_or("Aucune capture en cours.")?;
        (s.position, s.taille)
    };
    let curseur = app.cursor_position().map_err(|e| format!("Position du curseur illisible : {e}"))?;
    Ok((position, taille, (curseur.x.round() as i32, curseur.y.round() as i32)))
}

/// Les fenetres de l'ecran de la selection, gardees pour `captures_selection_prendre`.
#[tauri::command]
pub async fn captures_selection_fenetres(app: tauri::AppHandle) -> Result<Vec<FenetreVue>, String> {
    hors_thread_principal(app, |app| {
        let (position, taille) = {
            let etat = app.state::<EtatCaptures>();
            let selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
            let s = selection.as_ref().ok_or("Aucune capture en cours.")?;
            (s.position, s.taille)
        };
        let vues = fenetres_de_l_ecran(position, taille);
        let etat = app.state::<EtatCaptures>();
        let mut selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
        selection.as_mut().ok_or("La sélection est fermée.")?.fenetres = vues.clone();
        Ok(vues)
    })
    .await
}

/// La loupe : les pixels de l'ecran vivant autour du curseur (ADR-008), lus a
/// chaque demande et jamais gardes. La selection, protegee, n'y est pas.
#[tauri::command]
pub async fn captures_selection_loupe(app: tauri::AppHandle) -> Result<tauri::ipc::Response, String> {
    hors_thread_principal(app, |app| {
        let (position, taille, curseur) = ecran_et_curseur(&app)?;
        let (zone, decalage) = stockage::cadre_loupe(curseur, position, taille).ok_or("Le curseur a quitté l'écran de la sélection.")?;
        let ecran = xcap::Monitor::from_point(position.0 + 1, position.1 + 1).map_err(|e| format!("Écran introuvable : {e}"))?;
        let lue = ecran
            .capture_region(zone.x, zone.y, zone.largeur, zone.hauteur)
            .map_err(|e| format!("Lecture de la loupe impossible : {e}"))?;
        Ok(tauri::ipc::Response::new(stockage::assembler_loupe(&lue, decalage, (curseur.0 - position.0, curseur.1 - position.1))))
    })
    .await
}

/// La pipette (ADR-013) : la couleur lue par la loupe, `#rrggbb`, copiee en texte.
#[tauri::command]
pub async fn captures_copier_couleur(app: tauri::AppHandle, couleur: String) -> Result<(), String> {
    if !stockage::est_couleur(&couleur) {
        return Err("Une couleur s'écrit #rrggbb.".to_string());
    }
    hors_thread_principal(app, move |_| copier_texte(&couleur)).await
}

/// Une fleche : le curseur bouge de `dx`, `dy` pixels physiques, borne a l'ecran.
#[tauri::command]
pub async fn captures_selection_curseur(app: tauri::AppHandle, dx: i32, dy: i32) -> Result<(), String> {
    hors_thread_principal(app, move |app| {
        let (position, taille, curseur) = ecran_et_curseur(&app)?;
        let (x, y) = stockage::borner_a_l_ecran((curseur.0 + dx, curseur.1 + dy), position, taille);
        fenetres::poser_curseur(x, y);
        Ok(())
    })
    .await
}

/// Ce que la selection ouverte donne a filmer (ADR-016) : le coin et la taille
/// de son ecran, la zone (physique, relative a l'ecran) ou `None` pour l'ecran
/// entier, et la fiche (la fenetre cliquee, sinon le premier plan d'avant).
/// Bloquant : l'application d'une fenetre cliquee se lit aupres de Windows.
pub(crate) fn zone_a_filmer(
    app: &tauri::AppHandle,
    zone: Option<ZoneCss>,
    facteur: f64,
    fenetre: Option<usize>,
) -> Result<((i32, i32), (u32, u32), Option<stockage::ZonePhysique>, Origine), String> {
    let etat = app.state::<EtatCaptures>();
    let (position, taille, cliquee, origine) = {
        let selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
        let s = selection.as_ref().ok_or("Aucune sélection ouverte.")?;
        let cliquee = match fenetre {
            Some(rang) => Some(s.fenetres.get(rang).cloned().ok_or("Cette fenêtre n'est plus dans la liste.")?),
            None => None,
        };
        (s.position, s.taille, cliquee, s.origine.clone())
    };
    let physique = match (&cliquee, zone) {
        (Some(f), _) => Some(f.zone),
        (None, Some(zone)) => Some(stockage::zone_physique(zone, facteur, position, taille).ok_or("La zone tracée est vide.")?),
        (None, None) => None,
    };
    let origine = match cliquee {
        Some(f) => Origine { application: application_de(f.id), fenetre: f.titre, ..origine },
        None => origine,
    };
    Ok((position, taille, physique, origine))
}

/// Le trace est relache : l'ecran est pris maintenant, la zone recadree, et
/// rendue a la page en BMP pour y etre figee et annotee. Un nouveau trace
/// reprend une image neuve. `fenetre` : le rang d'une fenetre cliquee dans
/// `captures_selection_fenetres` ; son cadre remplace la zone (ADR-008).
#[tauri::command]
pub async fn captures_selection_prendre(
    app: tauri::AppHandle,
    zone: ZoneCss,
    facteur: f64,
    trace: u32,
    fenetre: Option<usize>,
) -> Result<tauri::ipc::Response, String> {
    hors_thread_principal(app, move |app| {
        let (position, taille, cliquee, origine, texte) = {
            let etat = app.state::<EtatCaptures>();
            let selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
            let s = selection.as_ref().ok_or("Aucune capture en cours.")?;
            let cliquee = match fenetre {
                Some(rang) => Some(s.fenetres.get(rang).cloned().ok_or("Cette fenêtre n'est plus dans la liste.")?),
                None => None,
            };
            (s.position, s.taille, cliquee, s.origine.clone(), s.mode == Mode::Texte)
        };
        let zone = match &cliquee {
            Some(f) => f.zone,
            None => stockage::zone_physique(zone, facteur, position, taille).ok_or("La zone tracée est vide.")?,
        };
        let origine_prise = cliquee.map(|f| Origine { fenetre: f.titre, application: application_de(f.id), ..origine });
        let ecran = prendre_ecran(position)?;
        if ecran.dimensions() != taille {
            return Err(format!(
                "L'écran pris mesure {} x {}, Tauri le dit {} x {} : capture refusée.",
                ecran.width(),
                ecran.height(),
                taille.0,
                taille.1
            ));
        }
        let image = image::imageops::crop_imm(&ecran, zone.x, zone.y, zone.largeur, zone.hauteur).to_image();
        // Mode « Texte » : la page ne fige rien, la zone ne part pas vers elle.
        // Une reponse vide le lui dit, quel que soit le mode qu'elle affiche.
        let octets = if texte { Vec::new() } else { bmp(&image) };
        let etat = app.state::<EtatCaptures>();
        let mut selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
        let s = selection.as_mut().ok_or("La sélection est fermée.")?;
        // Deux traces rapproches se prennent en parallele : le plus recent garde
        // sa zone, quel que soit l'ordre dans lequel les prises finissent.
        if trace < s.trace {
            return Err("Un tracé plus récent a été pris.".to_string());
        }
        s.trace = trace;
        s.prise = Some(Arc::new(image));
        s.origine_prise = origine_prise;
        s.zone_prise = Some(zone);
        Ok(tauri::ipc::Response::new(octets))
    })
    .await
}

/// Le trace `trace` est fige : sa zone brute est ecrite d'office (ADR-004), et
/// la capture d'un trace precedent part a la corbeille. Rend faux si ce trace
/// est deja remplace.
#[tauri::command]
pub async fn captures_selection_garder(app: tauri::AppHandle, trace: u32) -> Result<bool, String> {
    let ecrite = hors_thread_principal(app.clone(), move |app| {
        let (image, meta, ancienne, derniere) = {
            let etat = app.state::<EtatCaptures>();
            let mut selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
            let s = selection.as_mut().ok_or("La sélection est fermée.")?;
            let Some(image) = s.prise.clone().filter(|_| trace == s.trace) else {
                return Ok(None);
            };
            let derniere = s.zone_prise.map(|z| stockage::DerniereZone {
                x: z.x,
                y: z.y,
                largeur: z.largeur,
                hauteur: z.hauteur,
                position: s.position,
                taille: s.taille,
                ecran: s.origine.ecran.clone(),
                echelle: s.origine.echelle,
            });
            (image, s.origine_de_la_prise(), s.enregistree.take(), derniere)
        };
        if let Some(id) = ancienne {
            supprimer(&app, id)?;
        }
        let (capture, avertissement) = finir(&app, &image, &meta, &serde_json::json!([]), None, false, None)?;
        if let Some(derniere) = derniere {
            garder_derniere_zone(&app, &derniere);
        }
        let etat = app.state::<EtatCaptures>();
        let mut selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
        match selection.as_mut() {
            Some(s) => s.enregistree = Some(capture.id),
            // ponytail: fermee pendant l'ecriture (perdue), la capture reste, avec sa carte.
            None => crate::vignettes::ajouter(&app, capture),
        }
        Ok(Some(avertissement))
    })
    .await?;
    let Some(avertissement) = ecrite else { return Ok(false) };
    dire_copie_manquee(&app, avertissement);
    annoncer(&app);
    Ok(true)
}

/// Échap, ou « Fermer » : la zone deja ecrite reste, brute, et entre dans la
/// pile ; les formes non validees sont abandonnees. Sans zone ecrite, rien
/// n'est ecrit.
#[tauri::command]
pub async fn captures_selection_annuler(app: tauri::AppHandle) -> Result<(), String> {
    let id = enregistree(&app)?;
    fermer_selection(&app);
    if let Some(id) = id {
        let capture = hors_thread_principal(app.clone(), move |app| lire(&app, id)).await?;
        crate::vignettes::ajouter(&app, capture);
    }
    Ok(())
}

/// « Supprimer », ou Suppr : la zone deja ecrite part a la corbeille des
/// captures, puis la selection se ferme.
#[tauri::command]
pub async fn captures_selection_jeter(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(id) = enregistree(&app)? {
        hors_thread_principal(app.clone(), move |app| supprimer(&app, id)).await?;
        annoncer(&app);
    }
    fermer_selection(&app);
    Ok(())
}

/// L'attente apres chaque pas de la capture defilante, celle de ShareX.
const DEFILE_ATTENTE: Duration = Duration::from_millis(300);
// ponytail: plafonds fixes, des reglages si le besoin vient.
const DEFILE_HAUTEUR_MAX: u32 = 20_000;
const DEFILE_DUREE_MAX: Duration = Duration::from_secs(120);

/// Leve le drapeau de la capture defilante en cours. Vrai s'il y en avait une.
fn arreter_defilement(app: &tauri::AppHandle) -> bool {
    let etat = app.state::<EtatCaptures>();
    let Ok(defilement) = etat.defilement.lock() else { return false };
    defilement.as_ref().map(|arret| arret.store(true, Ordering::SeqCst)).is_some()
}

/// En manuel (ADR-019), l'utilisateur fait defiler : relire plus souvent, et
/// lui laisser plus de temps.
const DEFILE_ATTENTE_MANUEL: Duration = Duration::from_millis(150);
const DEFILE_DUREE_MAX_MANUEL: Duration = Duration::from_secs(300);

/// Comment la capture defilante avance (ADR-019) : la molette dans un sens, ou
/// l'utilisateur lui-meme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Facon {
    Auto(defilante::Sens),
    Manuel,
}

impl Facon {
    fn lire(brut: &str) -> Result<Facon, String> {
        use defilante::Sens;
        Ok(match brut {
            "bas" => Facon::Auto(Sens::Bas),
            "haut" => Facon::Auto(Sens::Haut),
            "droite" => Facon::Auto(Sens::Droite),
            "manuel" => Facon::Manuel,
            _ => return Err("Façon de défiler inconnue.".to_string()),
        })
    }
}

/// Ce que le bandeau de la selection montre : `perdue` depuis une vue
/// impossible a recoller, jusqu'a la prochaine qui recolle.
#[derive(Clone, serde::Serialize)]
struct Avancee {
    vues: u32,
    longueur: u32,
    perdue: bool,
}

/// L'automatique passe par un seul recollage, le manuel par la detection du sens.
enum Suivi {
    Un(defilante::Recollage),
    Detection(defilante::Detection),
}

impl Suivi {
    fn ajouter(&mut self, vue: &RgbaImage) -> defilante::Pas {
        match self {
            Suivi::Un(r) => r.ajouter(vue),
            Suivi::Detection(d) => d.ajouter(vue),
        }
    }

    fn longueur(&self) -> u32 {
        match self {
            Suivi::Un(r) => r.longueur(),
            Suivi::Detection(d) => d.longueur(),
        }
    }

    fn image(self) -> RgbaImage {
        match self {
            Suivi::Un(r) => r.image(),
            Suivi::Detection(d) => d.image(),
        }
    }
}

/// La boucle de la capture defilante (ADR-015, ADR-019). En automatique, un ou
/// deux crans de molette au centre de la zone, l'attente, la zone relue et
/// recollee ; le curseur revient ou il etait. En manuel, rien n'est injecte et
/// le curseur ne bouge pas : la zone est relue pendant que l'utilisateur fait
/// defiler. Bloquant. Rend l'image recollee, `None` si rien n'a defile.
fn defiler(app: &tauri::AppHandle, arret: &AtomicBool, facon: Facon) -> Result<Option<RgbaImage>, String> {
    let (position, zone) = {
        let etat = app.state::<EtatCaptures>();
        let selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
        let s = selection.as_ref().ok_or("Aucune capture en cours.")?;
        (s.position, s.zone_prise.ok_or("Aucune zone n'est figée.")?)
    };
    let ecran = xcap::Monitor::from_point(position.0 + 1, position.1 + 1).map_err(|e| format!("Écran introuvable : {e}"))?;
    let lire = || {
        ecran
            .capture_region(zone.x, zone.y, zone.largeur, zone.hauteur)
            .map_err(|e| format!("Lecture de la zone impossible : {e}"))
    };
    // En manuel, le curseur est a l'utilisateur : ni pose, ni remis.
    let avant = match facon {
        Facon::Auto(_) => app.cursor_position().ok(),
        Facon::Manuel => None,
    };
    if avant.is_some() {
        fenetres::poser_curseur(zone.bureau.0 + zone.largeur as i32 / 2, zone.bureau.1 + zone.hauteur as i32 / 2);
    }
    let recollee = (|| {
        let premiere = lire()?;
        let (mut suivi, attente, duree) = match facon {
            Facon::Auto(sens) => (Suivi::Un(defilante::Recollage::nouveau(&premiere, sens)), DEFILE_ATTENTE, DEFILE_DUREE_MAX),
            Facon::Manuel => (Suivi::Detection(defilante::Detection::nouvelle(&premiere)), DEFILE_ATTENTE_MANUEL, DEFILE_DUREE_MAX_MANUEL),
        };
        let axe = if facon == Facon::Auto(defilante::Sens::Droite) { zone.largeur } else { zone.hauteur };
        let crans = if axe >= 600 { 2 } else { 1 };
        let (mut vues, mut immobiles, mut echecs, mut perdue) = (1u32, 0, 0, false);
        let debut = Instant::now();
        // Automatique : deux pas sans mouvement font la fin, meme si un pas a ete
        // lent, et trois vues perdues l'arret. Manuel : seuls Entree, Echap, le
        // raccourci et les plafonds arretent.
        let automatique = facon != Facon::Manuel;
        while !arret.load(Ordering::SeqCst) && (!automatique || (immobiles < 2 && echecs < 3)) && suivi.longueur() < DEFILE_HAUTEUR_MAX && debut.elapsed() < duree {
            if let Facon::Auto(sens) = facon {
                fenetres::molette(sens, crans);
            }
            std::thread::sleep(attente);
            let vue = lire()?;
            if vue.dimensions() != premiere.dimensions() {
                return Err("La zone relue n'a plus sa taille.".to_string());
            }
            let etait_perdue = perdue;
            let pas = suivi.ajouter(&vue);
            match pas {
                defilante::Pas::Decale(_) => {
                    vues += 1;
                    (immobiles, echecs, perdue) = (0, 0, false);
                }
                defilante::Pas::Immobile => immobiles += 1,
                defilante::Pas::Introuvable => {
                    echecs += 1;
                    // Le signal rouge demande un geste : en automatique, personne n'a a revenir en arriere (ADR-019).
                    perdue = !automatique;
                }
            }
            if matches!(pas, defilante::Pas::Decale(_)) || perdue != etait_perdue {
                let _ = app.emit_to(ETIQUETTE_SELECTION, EVENEMENT_DEFILE, Avancee { vues, longueur: suivi.longueur(), perdue });
            }
        }
        Ok((vues > 1).then(|| suivi.image()))
    })();
    if let Some(avant) = avant {
        fenetres::poser_curseur(avant.x.round() as i32, avant.y.round() as i32);
    }
    recollee
}

/// « Défiler » (ADR-015) : la selection laisse passer la souris, Rust fait
/// defiler la fenetre sous la zone et recolle. L'image recollee remplace la
/// zone ecrite d'office, qui part a la corbeille ; puis la selection se ferme
/// comme a Échap. Rien n'a defile : la zone reste telle quelle, et la pastille
/// le dit. `facon` : « bas », « haut », « droite » ou « manuel » (ADR-019).
#[tauri::command]
pub async fn captures_selection_defiler(app: tauri::AppHandle, facon: String) -> Result<(), String> {
    let facon = Facon::lire(&facon)?;
    let fenetre = app.get_webview_window(ETIQUETTE_SELECTION).ok_or("La sélection est fermée.")?;
    let arret = Arc::new(AtomicBool::new(false));
    {
        let etat = app.state::<EtatCaptures>();
        let mut defilement = etat.defilement.lock().map_err(|_| VERROU.to_string())?;
        if defilement.is_some() {
            return Err("Une capture défilante est déjà en cours.".to_string());
        }
        *defilement = Some(arret.clone());
    }
    let _ = fenetre.set_ignore_cursor_events(true);
    let recollee = hors_thread_principal(app.clone(), move |app| {
        let Some(image) = defiler(&app, &arret, facon)? else { return Ok(None) };
        let meta = {
            let etat = app.state::<EtatCaptures>();
            let selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
            selection.as_ref().ok_or("La sélection est fermée.")?.origine_de_la_prise()
        };
        // La nouvelle d'abord : si elle ne s'ecrit pas, l'ancienne reste.
        let (capture, avertissement) = finir(&app, &image, &meta, &serde_json::json!([]), None, false, None)?;
        let ancienne = {
            let etat = app.state::<EtatCaptures>();
            let mut selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
            let s = selection.as_mut().ok_or("La sélection est fermée.")?;
            s.prise = Some(Arc::new(image));
            s.enregistree.replace(capture.id)
        };
        if let Some(id) = ancienne {
            supprimer(&app, id)?;
        }
        Ok(Some(avertissement))
    })
    .await;
    if let Ok(mut defilement) = app.state::<EtatCaptures>().defilement.lock() {
        *defilement = None;
    }
    match recollee {
        Ok(Some(avertissement)) => {
            dire_copie_manquee(&app, avertissement);
            annoncer(&app);
            captures_selection_annuler(app).await
        }
        Ok(None) => {
            avertir(&app, if facon == Facon::Manuel { "Rien n'a défilé" } else { "Rien n'a défilé. Essayez « Je fais défiler moi-même »." });
            captures_selection_annuler(app).await
        }
        Err(motif) => {
            let _ = fenetre.set_ignore_cursor_events(false);
            Err(motif)
        }
    }
}

/// Entrée ou Échap pendant une capture defilante : elle s'arrete et garde ce
/// qui est recolle.
#[tauri::command]
pub fn captures_selection_arreter(app: tauri::AppHandle) {
    arreter_defilement(&app);
}

/// Entrée, « Terminer » ou « Épingler ». `rendu` : l'image annotee aplatie par la
/// page, absente s'il n'y a aucune forme. La zone deja ecrite recoit les formes ;
/// sans elle (l'ecriture d'office a echoue), la zone est ecrite ici. `epingler` :
/// une epingle se pose a la place exacte de la zone (ADR-011).
#[tauri::command]
pub async fn captures_selection_enregistrer(
    app: tauri::AppHandle,
    formes: serde_json::Value,
    rendu: Option<String>,
    epingler: Option<bool>,
) -> Result<Capture, String> {
    let ((capture, avertissement), place) = hors_thread_principal(app.clone(), move |app| {
        let (image, meta, id, place) = {
            let etat = app.state::<EtatCaptures>();
            let selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
            let s = selection.as_ref().ok_or("Aucune capture en cours.")?;
            let place = s.zone_prise.map(|z| (z.bureau.0, z.bureau.1, z.largeur, z.hauteur));
            (s.prise.clone().ok_or("Aucune zone n'est tracée.")?, s.origine_de_la_prise(), s.enregistree, place)
        };
        let ecrite = match id {
            Some(id) if rendu.is_some() => {
                // Une zone de la selection n'est jamais recadree ni orientee (ADR-018) : elle se retrace.
                let (capture, annotee) = modifier(&app, id, &formes, &serde_json::json!(0), &serde_json::Value::Null, &serde_json::Value::Null, rendu.as_deref())?;
                (capture, copier_image(&annotee).err())
            }
            // Ecrite et copiee brute au relachement : rien a refaire.
            Some(id) => (lire(&app, id)?, None),
            None => finir(&app, &image, &meta, &formes, rendu.as_deref(), false, None)?,
        };
        Ok((ecrite, place))
    })
    .await?;
    dire_copie_manquee(&app, avertissement);
    fermer_selection(&app);
    crate::vignettes::ajouter(&app, capture.clone());
    annoncer(&app);
    if let (Some(true), Some(place)) = (epingler, place) {
        // Apres la fermeture : la selection, protegee et devant, la couvrirait.
        let epinglee = capture.clone();
        if let Err(motif) = hors_thread_principal(app.clone(), move |app| crate::epingles::epingler(&app, &epinglee, Some(place)).map(|_| ())).await {
            avertir(&app, &motif);
        }
    }
    Ok(capture)
}

/// Entrée sans zone tracee : l'ecran entier que la selection couvre.
#[tauri::command]
pub async fn captures_selection_ecran_entier(app: tauri::AppHandle) -> Result<Capture, String> {
    let (capture, avertissement) = hors_thread_principal(app.clone(), move |app| {
        let (position, meta) = {
            let etat = app.state::<EtatCaptures>();
            let selection = etat.selection.lock().map_err(|_| VERROU.to_string())?;
            let s = selection.as_ref().ok_or("Aucune capture en cours.")?;
            (s.position, s.origine.clone())
        };
        let image = prendre_ecran(position)?;
        finir(&app, &image, &meta, &serde_json::json!([]), None, true, None)
    })
    .await?;
    dire_copie_manquee(&app, avertissement);
    fermer_selection(&app);
    annoncer(&app);
    Ok(capture)
}

// ---------------------------------------------------------------------------
// Commandes : ecran entier et galerie
// ---------------------------------------------------------------------------

/// Bouton « Écran entier » de la pastille : l'ecran sous le curseur, sans
/// passer par la selection ni l'annotation.
#[tauri::command]
pub async fn captures_ecran_entier(app: tauri::AppHandle) -> Result<Capture, String> {
    ecran_entier(app).await
}

#[tauri::command]
pub async fn captures_lister(app: tauri::AppHandle, recherche: Option<String>) -> Result<Vec<Capture>, String> {
    hors_thread_principal(app, move |app| {
        let dossier = dossier(&app)?;
        app.state::<AppState>().with_conn(|conn| stockage::lister(conn, &dossier, recherche.as_deref().unwrap_or("")))
    })
    .await
}

#[tauri::command]
pub async fn captures_noter(app: tauri::AppHandle, id: i64, note: String) -> Result<(), String> {
    hors_thread_principal(app, move |app| app.state::<AppState>().with_conn(|conn| stockage::noter(conn, id, &note))).await
}

/// Le titre que la galerie montre : celui de la fenetre d'origine, ou celui qu'on lui donne.
#[tauri::command]
pub async fn captures_renommer(app: tauri::AppHandle, id: i64, titre: String) -> Result<(), String> {
    let titre = titre.trim().to_string();
    hors_thread_principal(app, move |app| app.state::<AppState>().with_conn(|conn| stockage::renommer(conn, id, &titre))).await
}

/// Remplace les formes, l'orientation et le recadrage d'une capture et refait son rendu.
/// `rendu` absent : plus aucune forme, le rendu redevient l'original oriente, rogne au
/// recadrage. `orientation` : un entier de 0 a 7, exige (ADR-018). `recadrage` absent ou
/// `null` : l'image entiere. `fond` absent ou `null` : sans fond.
#[tauri::command]
pub async fn captures_modifier(
    app: tauri::AppHandle,
    id: i64,
    formes: serde_json::Value,
    orientation: Option<serde_json::Value>,
    recadrage: Option<serde_json::Value>,
    fond: Option<serde_json::Value>,
    rendu: Option<String>,
) -> Result<Capture, String> {
    // Absente, l'orientation vaut `null`, que la validation refuse avec un message clair.
    let orientation = orientation.unwrap_or(serde_json::Value::Null);
    let recadrage = recadrage.unwrap_or(serde_json::Value::Null);
    let fond = fond.unwrap_or(serde_json::Value::Null);
    let capture = hors_thread_principal(app.clone(), move |app| {
        modifier(&app, id, &formes, &orientation, &recadrage, &fond, rendu.as_deref()).map(|(capture, _)| capture)
    })
    .await?;
    annoncer(&app);
    crate::vignettes::remplacer(&app, capture.clone());
    Ok(capture)
}

#[tauri::command]
pub async fn captures_supprimer(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    hors_thread_principal(app.clone(), move |app| supprimer(&app, id)).await?;
    annoncer(&app);
    crate::vignettes::retirer(&app, id);
    Ok(())
}

/// Remet l'image annotee d'une capture dans le presse-papiers, ou le fichier
/// d'une video.
#[tauri::command]
pub async fn captures_copier(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    hors_thread_principal(app, move |app| {
        let capture = lire(&app, id)?;
        match capture.duree_ms {
            Some(_) => copier_fichier(std::path::Path::new(&capture.origine)),
            None => copier_image(&stockage::lire_png(std::path::Path::new(&capture.rendu))?),
        }
    })
    .await
}

/// « Copier » de la barre de selection : les fichiers que l'Explorateur montrerait
/// (l'image annotee, ou le MP4), dans l'ordre donne.
#[tauri::command]
pub async fn captures_copier_plusieurs(app: tauri::AppHandle, ids: Vec<i64>) -> Result<(), String> {
    hors_thread_principal(app, move |app| {
        let chemins = ids
            .iter()
            .map(|&id| lire(&app, id).map(|capture| PathBuf::from(fichier_montre(&capture))))
            .collect::<Result<Vec<_>, _>>()?;
        if chemins.is_empty() {
            return Err("Aucune capture choisie.".to_string());
        }
        copier_fichiers(&chemins)
    })
    .await
}

/// Le chemin du dossier des captures, pour le montrer dans les reglages.
#[tauri::command]
pub async fn captures_dossier(app: tauri::AppHandle) -> Result<String, String> {
    hors_thread_principal(app, |app| Ok(dossier(&app)?.to_string_lossy().into_owned())).await
}

/// Le fichier que l'Explorateur selectionne : le MP4 d'une video, l'image annotee sinon
/// (celle que « Copier » met au presse-papiers).
fn fichier_montre(capture: &Capture) -> &str {
    match capture.duree_ms {
        Some(_) => &capture.origine,
        None => &capture.rendu,
    }
}

fn montrer_dans_l_explorateur(app: &tauri::AppHandle, id: i64) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let capture = lire(app, id)?;
    app.opener()
        .reveal_item_in_dir(fichier_montre(&capture))
        .map_err(|e| format!("Ouverture de l'Explorateur impossible : {e}"))
}

/// « Ouvrir le dossier » de la fiche : l'Explorateur, le fichier selectionne.
#[tauri::command]
pub async fn captures_montrer(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    hors_thread_principal(app, move |app| montrer_dans_l_explorateur(&app, id)).await
}

/// Prefixe des entrees du menu d'une capture (`capture-menu-<action>@<id>`).
pub const PREFIXE_MENU: &str = "capture-menu-";

/// Le clic droit sur une carte de la galerie ou une vignette : le menu natif,
/// sous le curseur. Ses clics passent par le menu de notification, puis `sur_menu`.
#[tauri::command]
pub async fn captures_menu(app: tauri::AppHandle, fenetre: tauri::WebviewWindow, id: i64) -> Result<(), String> {
    use tauri::menu::{Menu, MenuItem};
    let montrer = MenuItem::with_id(&app, format!("{PREFIXE_MENU}montrer@{id}"), "Afficher dans l'Explorateur", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let menu = Menu::with_items(&app, &[&montrer]).map_err(|e| format!("Menu de la capture impossible : {e}"))?;
    fenetre.popup_menu(&menu).map_err(|e| format!("Menu de la capture impossible à ouvrir : {e}"))
}

/// Un clic du menu d'une capture, sur le thread principal : le travail part a cote.
pub fn sur_menu(app: &tauri::AppHandle, id_menu: &str) {
    let Some(("montrer", id)) = id_menu.strip_prefix(PREFIXE_MENU).and_then(|reste| reste.split_once('@')) else { return };
    let Ok(id) = id.parse::<i64>() else { return };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(motif) = montrer_dans_l_explorateur(&app, id) {
            avertir(&app, &motif);
        }
    });
}

#[tauri::command]
pub async fn captures_ouvrir_dossier(app: tauri::AppHandle) -> Result<String, String> {
    hors_thread_principal(app, |app| {
        use tauri_plugin_opener::OpenerExt;
        let dossier = dossier(&app)?;
        app.opener()
            .open_path(dossier.to_string_lossy().to_string(), None::<&str>)
            .map_err(|e| format!("Ouverture du dossier impossible : {e}"))?;
        Ok(dossier.to_string_lossy().into_owned())
    })
    .await
}

/// Ce que « Changer… » propose de deplacer, a confirmer dans les Reglages.
#[derive(serde::Serialize)]
pub struct Deplacement {
    cible: String,
    fichiers: usize,
}

/// « Changer… » (ADR-020) : le choix de dossier de Windows, puis ce qui
/// partirait. `None` si l'on annule ; un refus avant tout deplacement.
#[tauri::command]
pub async fn captures_choisir_dossier(app: tauri::AppHandle) -> Result<Option<Deplacement>, String> {
    hors_thread_principal(app, |app| {
        use tauri_plugin_dialog::DialogExt;
        let mut choix = app.dialog().file().set_title("Nouveau dossier des captures");
        if let Some(fenetre) = app.get_webview_window(crate::pastille::ETIQUETTE_PRINCIPALE) {
            choix = choix.set_parent(&fenetre);
        }
        let Some(cible) = choix.blocking_pick_folder() else { return Ok(None) };
        let cible = cible.into_path().map_err(|e| format!("Dossier illisible : {e}"))?;
        let ancien = dossier(&app)?;
        let fichiers = deplacer::a_deplacer(&ancien)?;
        if let Some(motif) = deplacer::refus(&ancien, &cible, &fichiers) {
            return Err(motif);
        }
        Ok(Some(Deplacement { cible: cible.to_string_lossy().into_owned(), fichiers: fichiers.len() }))
    })
    .await
}

/// Deplace les captures vers `cible` et en fait le dossier des captures. Le
/// deplacement attend les ecritures en vol (`ecriture`), puis tient les verrous
/// du debut a la fin : une capture prise pendant la copie attend, puis va dans
/// le nouveau dossier.
#[tauri::command]
pub async fn captures_deplacer_dossier(app: tauri::AppHandle, cible: String) -> Result<String, String> {
    hors_thread_principal(app, move |app| {
        let cible = PathBuf::from(cible);
        let captures = app.state::<EtatCaptures>();
        let _ecritures = captures.ecritures.write().map_err(|_| VERROU.to_string())?;
        let mut choisi = captures.choisi.lock().map_err(|_| VERROU.to_string())?;
        let ancien = choisi.clone();
        let fichiers = deplacer::a_deplacer(&ancien)?;
        if let Some(motif) = deplacer::refus(&ancien, &cible, &fichiers) {
            return Err(motif);
        }
        std::fs::create_dir_all(&cible).map_err(|e| format!("Création de {} impossible : {e}", cible.display()))?;
        // La portee d'abord : si le reglage echoue ensuite, une portee de trop ne
        // gene rien ; l'inverse laisserait la base pointer un dossier vide.
        deplacer::deplacer(&ancien, &cible, &fichiers, || {
            app.asset_protocol_scope()
                .allow_directory(&cible, true)
                .map_err(|e| format!("Portée des captures refusée pour {} : {e}", cible.display()))?;
            app.state::<AppState>().with_conn(|conn| db::regler(conn, CLE_DOSSIER, &cible.to_string_lossy()))
        })?;
        *choisi = cible.clone();
        drop(choisi);
        let _ = app.emit(EVENEMENT_CHANGEE, ());
        Ok(cible.to_string_lossy().into_owned())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Essai reel, a la main : `cargo test --lib fichier_seul_au_presse_papiers -- --ignored`.
    /// Ecrase le presse-papiers. Un texte pose par OLE (comme un navigateur) puis
    /// la copie d'un fichier : les lecteurs OLE (Explorateur, .NET) ne doivent
    /// voir que le fichier (vu le 10 octobre 2026 : sans vidage, le texte seul).
    #[test]
    fn explorateur_montre_le_mp4_d_une_video_et_le_rendu_d_une_image() {
        let mut capture = Capture { origine: "a.png".into(), rendu: "a.rendu.png".into(), ..Default::default() };
        assert_eq!(fichier_montre(&capture), "a.rendu.png");
        capture.origine = "a.mp4".into();
        capture.duree_ms = Some(1000);
        assert_eq!(fichier_montre(&capture), "a.mp4");
    }

    #[test]
    #[ignore = "presse-papiers reel"]
    fn fichier_seul_au_presse_papiers() {
        let ps = |commande: &str| {
            let sortie = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-STA", "-Command", commande])
                .output()
                .unwrap();
            String::from_utf8_lossy(&sortie.stdout).trim().to_string()
        };
        let fichier = std::env::temp_dir().join("equishot-essai-presse-papiers.mp4");
        std::fs::write(&fichier, b"essai").unwrap();
        ps("Set-Clipboard -Value 'texte avant'");
        copier_fichier(&fichier).unwrap();
        let formats = ps("Add-Type -A System.Windows.Forms; [Windows.Forms.Clipboard]::GetDataObject().GetFormats() -join ','");
        let _ = std::fs::remove_file(&fichier);
        assert!(formats.split(',').any(|f| f == "FileDrop"), "formats vus : {formats}");
        assert!(!formats.contains("Text"), "l'ancien texte reste : {formats}");
    }

    #[test]
    fn plusieurs_chemins_comme_l_explorateur() {
        let chemins = [PathBuf::from(r"C:\a b\1.png"), PathBuf::from(r"C:\2.mp4")];
        assert_eq!(chemins_en_texte(&chemins), "\"C:\\a b\\1.png\"\r\n\"C:\\2.mp4\"");
    }

    /// Essai reel : `cargo test --lib plusieurs_fichiers_au_presse_papiers -- --ignored`.
    /// Ecrase le presse-papiers. Les lecteurs OLE doivent voir les deux fichiers et
    /// leurs chemins en texte.
    #[test]
    #[ignore = "presse-papiers reel"]
    fn plusieurs_fichiers_au_presse_papiers() {
        let ps = |commande: &str| {
            let sortie = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-STA", "-Command", commande])
                .output()
                .unwrap();
            String::from_utf8_lossy(&sortie.stdout).trim().to_string()
        };
        let chemins: Vec<PathBuf> = ["png", "mp4"].iter().map(|e| std::env::temp_dir().join(format!("equishot-essai-plusieurs.{e}"))).collect();
        for c in &chemins {
            std::fs::write(c, b"essai").unwrap();
        }
        copier_fichiers(&chemins).unwrap();
        let lu = |quoi: &str| ps(&format!("Add-Type -A System.Windows.Forms; [Windows.Forms.Clipboard]::{quoi}"));
        let fichiers = lu("GetFileDropList() -join '|'");
        let texte = lu("GetText()");
        for c in &chemins {
            let _ = std::fs::remove_file(c);
        }
        let attendus = chemins.iter().map(|c| c.display().to_string()).collect::<Vec<_>>();
        assert_eq!(fichiers.to_lowercase(), attendus.join("|").to_lowercase(), "fichiers vus");
        assert_eq!(texte.replace("\r\n", "\n"), chemins_en_texte(&chemins).replace("\r\n", "\n"), "texte vu");
    }

    #[test]
    fn defilante_facon_lue_des_quatre_chaines() {
        use defilante::Sens;
        assert_eq!(Facon::lire("bas"), Ok(Facon::Auto(Sens::Bas)));
        assert_eq!(Facon::lire("haut"), Ok(Facon::Auto(Sens::Haut)));
        assert_eq!(Facon::lire("droite"), Ok(Facon::Auto(Sens::Droite)));
        assert_eq!(Facon::lire("manuel"), Ok(Facon::Manuel));
        assert!(Facon::lire("gauche").is_err());
    }

    #[test]
    fn captures_texte_une_lecture_ratee_garde_la_zone() {
        assert_eq!(texte_ou_vide(Ok("Facture".into())), ("Facture".to_string(), None));
        assert_eq!(
            texte_ou_vide(Err("Aucune langue".into())),
            (String::new(), Some("Aucune langue".to_string())),
            "la zone s'écrit avec un texte vide, le motif est dit à part"
        );
    }

    #[test]
    fn captures_differe_un_seul_a_la_fois() {
        let mut d = Differe::default();
        let premier = d.commencer().expect("libre");
        assert_eq!(d.commencer(), None, "un second différé est refusé");
        assert!(d.encore(premier));
        assert!(d.finir(premier), "l'attente arrivée au bout prend l'écran");
        assert!(!d.encore(premier));
        assert!(d.commencer().is_some(), "libre à nouveau");
    }

    #[test]
    fn captures_differe_annule_n_est_pas_pris() {
        let mut d = Differe::default();
        assert!(!d.annuler(), "rien à annuler");
        let premier = d.commencer().expect("libre");
        assert!(d.annuler());
        assert!(!d.encore(premier), "l'attente voit l'annulation à la seconde suivante");
        assert!(!d.finir(premier), "et ne prend rien");
        // Annulé puis relancé dans la même seconde : l'ancienne attente ne prend
        // pas l'écran du nouveau, ni ne l'arrête.
        let second = d.commencer().expect("libre");
        assert_ne!(premier, second);
        assert!(!d.encore(premier) && !d.finir(premier));
        assert!(d.encore(second));
    }

    #[test]
    fn captures_differe_delais_du_menu_seulement() {
        for permis in [3, 5, 10] {
            assert_eq!(delai_permis(permis), Ok(permis));
        }
        for refuse in [0, 1, 4, 11, 3600] {
            assert!(delai_permis(refuse).is_err(), "{refuse} s");
        }
    }

    #[test]
    fn captures_bmp_entete_et_lignes_de_bas_en_haut() {
        // 3 x 2 : la ligne fait 9 octets, completee a 12.
        let mut image = RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]));
        image.put_pixel(0, 1, image::Rgba([200, 100, 50, 255]));
        let octets = bmp(&image);
        assert_eq!(&octets[..2], b"BM");
        assert_eq!(octets.len(), 54 + 24);
        assert_eq!(u32::from_le_bytes(octets[2..6].try_into().unwrap()), 78);
        assert_eq!(i32::from_le_bytes(octets[18..22].try_into().unwrap()), 3);
        assert_eq!(i32::from_le_bytes(octets[22..26].try_into().unwrap()), 2);
        assert_eq!(u16::from_le_bytes(octets[28..30].try_into().unwrap()), 24);
        // Premier pixel du fichier : celui du bas a gauche, en bleu, vert, rouge.
        assert_eq!(&octets[54..57], &[50, 100, 200]);
        assert_eq!(&octets[54 + 12..54 + 15], &[30, 20, 10], "la ligne du haut vient après");
    }

    #[test]
    fn captures_rendu_de_la_taille_de_l_original_seulement() {
        let image = RgbaImage::from_pixel(4, 3, image::Rgba([1, 2, 3, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).expect("png");
        let encode = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
        assert_eq!(decoder_rendu(&encode, (4, 3)).expect("rendu").dimensions(), (4, 3));
        assert!(decoder_rendu(&encode, (4, 4)).is_err(), "une autre taille est refusée");
        assert!(decoder_rendu("pas du base64 !", (4, 3)).is_err());
    }

    #[test]
    fn captures_image_a_rendre_orientee_ou_controlee() {
        let dossier = std::env::temp_dir().join(format!("equishot-orientation-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).expect("dossier");
        let origine = dossier.join("o.png");
        // 4 x 3, chaque pixel numerote : apres un quart de tour horaire, le haut gauche est
        // le bas gauche de l'original (y = 2, x = 0, soit 8).
        let image = RgbaImage::from_fn(4, 3, |x, y| image::Rgba([(y * 4 + x) as u8, 0, 0, 255]));
        image.save_with_format(&origine, image::ImageFormat::Png).expect("png");
        let sans_forme = image_a_rendre(None, &origine, (4, 3), 1).expect("orientée");
        assert_eq!(sans_forme.dimensions(), (3, 4), "la taille orientée");
        assert_eq!(sans_forme.get_pixel(0, 0).0[0], 8, "le bas gauche de l'original en haut à gauche");
        assert_eq!(image_a_rendre(None, &origine, (4, 3), 0).expect("telle quelle"), image);
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).expect("png");
        let encode = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
        assert!(image_a_rendre(Some(&encode), &origine, (4, 3), 1).is_err(), "un rendu 4 x 3 pour une capture pivotée (3 x 4) est refusé");
        assert_eq!(image_a_rendre(Some(&encode), &origine, (4, 3), 0).expect("rendu").dimensions(), (4, 3));
        let _ = std::fs::remove_dir_all(&dossier);
    }
}
