//! La pastille flottante, l'icone de la zone de notification et la fenetre
//! « EquiShot » (galerie et reglages), ouverte a la demande.
//!
//! La pastille est protegee (`content_protected`) : elle n'apparait dans aucune
//! capture ni aucun partage d'ecran. Elle n'est pas focalisable : un clic sur
//! elle laisse le premier plan a l'application ou l'on travaille.

use tauri::menu::{ContextMenu, IsMenuItem, Menu, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use crate::{captures, db, AppState};

pub const ETIQUETTE: &str = "pastille";
pub const ETIQUETTE_PRINCIPALE: &str = "principale";
/// Reglage : position de la pastille, `x,y` en pixels physiques du bureau.
const CLE_POSITION: &str = "pastille_position";
/// Reglage : `1` si la pastille est contractee (icones seules).
const CLE_CONTRACTEE: &str = "pastille_contractee";
/// Taille de la pastille, en pixels CSS.
// 404 tenait juste avec quatre outils au survol ; le minuteur en ajoute un (ADR-009),
// le texte d'une zone un autre (ADR-010), la video un autre (ADR-016).
// Contractee : poignee, deux icones, « deplier » et « ranger » (0.21).
const LARGEUR: f64 = 500.0;
const LARGEUR_CONTRACTEE: f64 = 194.0;
const HAUTEUR: f64 = 56.0;
/// Vrai quand la pastille est rangee. Pose avant `ShowWindowAsync`, qui agit plus
/// tard sur son fil : la pile qui relisait `is_visible` lisait l'etat d'avant.
static RANGEE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Vrai si la pastille est rangee (ou en train de l'etre).
pub fn rangee() -> bool {
    RANGEE.load(std::sync::atomic::Ordering::SeqCst)
}

/// Le premier plan d'avant le menu « Filmer » de la pastille : muda le donne a la
/// pastille avant `TrackPopupMenu`. Pris par le premier evenement de menu qui suit.
static DEVANT_MENU: std::sync::Mutex<Option<(isize, std::time::Instant)>> = std::sync::Mutex::new(None);

/// Passe ce delai, le releve est perime : son menu s'est ferme sans choix.
// ponytail: un delai, faute de savoir quand le menu se ferme ; Echap puis le menu de
// l'icone dans ce delai reprend l'ancien releve. Marquer les identifiants de la pastille sinon.
const DEVANT_MENU_VALIDE: std::time::Duration = std::time::Duration::from_secs(30);

/// Le premier plan a rendre apres un choix dans un menu : celui d'avant le menu de
/// la pastille, sinon celui de maintenant (menu de l'icone).
fn devant_du_menu() -> isize {
    match DEVANT_MENU.lock().ok().and_then(|mut d| d.take()) {
        Some((d, quand)) if quand.elapsed() < DEVANT_MENU_VALIDE => d,
        _ => crate::fenetres::premier_plan(),
    }
}

fn lire_position(brut: &str) -> Option<(i32, i32)> {
    let (x, y) = brut.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

fn largeur(contractee: bool) -> f64 {
    if contractee { LARGEUR_CONTRACTEE } else { LARGEUR }
}

/// Le x qui garde une fenetre de `largeur` pixels dans la zone `[zx, zx + zl)` :
/// le bord gauche ne bouge que si le droit sortirait de l'ecran.
fn x_dans_zone(x: i32, largeur: i32, zx: i32, zl: i32) -> i32 {
    x.min(zx + zl - largeur).max(zx)
}

fn contractee(app: &tauri::AppHandle) -> bool {
    app.state::<AppState>().with_conn(|conn| db::reglage(conn, CLE_CONTRACTEE)).map(|v| v == "1").unwrap_or(false)
}

/// Cree la pastille, a sa derniere position si elle est encore sur un ecran.
pub fn creer(app: &tauri::AppHandle) -> tauri::Result<()> {
    let position = app
        .state::<AppState>()
        .with_conn(|conn| db::reglage(conn, CLE_POSITION))
        .ok()
        .and_then(|brut| lire_position(&brut))
        .filter(|&(x, y)| app.monitor_from_point(x as f64 + 8.0, y as f64 + 8.0).ok().flatten().is_some());

    let fenetre = WebviewWindowBuilder::new(app, ETIQUETTE, WebviewUrl::App("pastille.html".into()))
        .title("EquiShot")
        .inner_size(largeur(contractee(app)), HAUTEUR)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        .content_protected(crate::db::fenetres_protegees())
        .visible(false)
        .build()?;
    match position {
        Some((x, y)) => fenetre.set_position(tauri::PhysicalPosition::new(x, y))?,
        None => {
            // Premier lancement : en haut, au centre de l'ecran principal.
            if let Some(ecran) = app.primary_monitor()? {
                let echelle = ecran.scale_factor();
                let x = ecran.position().x + (ecran.size().width as f64 / 2.0 - LARGEUR * echelle / 2.0) as i32;
                fenetre.set_position(tauri::PhysicalPosition::new(x, ecran.position().y + (24.0 * echelle) as i32))?;
            }
        }
    }
    let poignee = app.clone();
    fenetre.on_window_event(move |evenement| {
        if let tauri::WindowEvent::Moved(position) = evenement {
            crate::vignettes::suivre(&poignee);
            // Ecrite hors du thread principal : un glisser en emet beaucoup.
            let (app, valeur) = (poignee.clone(), format!("{},{}", position.x, position.y));
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(motif) = app.state::<AppState>().with_conn(|conn| db::regler(conn, CLE_POSITION, &valeur)) {
                    eprintln!("position de la pastille non écrite : {motif}");
                }
            });
        }
    });
    fenetre.show()?;
    Ok(())
}

/// Remontre la pastille rangee, sans lui donner le premier plan.
pub fn montrer(app: &tauri::AppHandle) {
    if let Some(fenetre) = app.get_webview_window(ETIQUETTE) {
        RANGEE.store(false, std::sync::atomic::Ordering::SeqCst);
        crate::fenetres::montrer_sans_activer(&fenetre, true);
        crate::vignettes::suivre(app);
    }
}

/// Ouvre la fenetre « EquiShot », ou la ramene devant si elle l'est deja.
/// `section` : `galerie` ou `reglages`.
pub fn ouvrir_principale(app: &tauri::AppHandle, section: &str) -> Result<(), String> {
    let adresse = format!("index.html#{section}");
    if let Some(fenetre) = app.get_webview_window(ETIQUETTE_PRINCIPALE) {
        let _ = fenetre.eval(format!("window.location.hash = '{section}'"));
        let _ = fenetre.unminimize();
        return fenetre.show().and_then(|_| fenetre.set_focus()).map_err(|e| format!("Fenêtre EquiShot impossible à montrer : {e}"));
    }
    WebviewWindowBuilder::new(app, ETIQUETTE_PRINCIPALE, WebviewUrl::App(adresse.into()))
        .title("EquiShot")
        .inner_size(1100.0, 740.0)
        .min_inner_size(720.0, 480.0)
        .theme(Some(tauri::Theme::Dark))
        .build()
        .map(|fenetre| {
            crate::fenetres::sans_menu_alt(&fenetre);
            let app = app.clone();
            fenetre.on_window_event(move |evenement| {
                if let tauri::WindowEvent::Destroyed = evenement {
                    crate::captures::reprendre_raccourci(&app);
                }
            });
        })
        .map_err(|e| format!("Fenêtre EquiShot impossible à ouvrir : {e}"))
}

const ICONE: &str = "equishot";
const INFOBULLE: &str = concat!("EquiShot ", env!("CARGO_PKG_VERSION"));
/// Les delais de l'ecran entier differe (ADR-009), dans le menu et la pastille.
const DELAIS: [u32; 3] = [3, 5, 10];

/// Les entrees du menu de notification qui se grisent : gardees pour les suivre.
struct EntreesDuMenu {
    derniere: MenuItem<tauri::Wry>,
    annuler: MenuItem<tauri::Wry>,
    /// Le sous-menu « Filmer » (0.19) : refait a chaque changement des scenes.
    filmer: Submenu<tauri::Wry>,
    /// Deux reconstructions ne s'entrelacent pas (entrees en double).
    refaire: std::sync::Mutex<()>,
    /// Le menu, et « Installer la mise a jour » (ADR-020), pose au-dessus de
    /// « Quitter » seulement quand une version est trouvee.
    menu: Menu<tauri::Wry>,
    maj: MenuItem<tauri::Wry>,
    maj_posee: std::sync::Mutex<bool>,
}

/// La place de « Installer la mise a jour » : juste avant « Quitter », le dernier.
const POSITION_MAJ: usize = 10;

/// La mise a jour trouvee (ADR-020) : l'entree se pose au-dessus de « Quitter »
/// avec sa version, ou part. Jamais depuis le thread principal.
pub fn proposer_maj(app: &tauri::AppHandle, version: Option<&str>) {
    let Some(entrees) = app.try_state::<EntreesDuMenu>() else { return };
    let Ok(mut posee) = entrees.maj_posee.lock() else { return };
    let resultat = match version {
        Some(version) => entrees.maj.set_text(format!("Installer la mise à jour {version}")).and_then(|()| {
            if !*posee {
                entrees.menu.insert(&entrees.maj, POSITION_MAJ)?;
                *posee = true;
            }
            Ok(())
        }),
        None if *posee => entrees.menu.remove(&entrees.maj).map(|()| *posee = false),
        None => Ok(()),
    };
    if let Err(e) = resultat {
        eprintln!("menu de la mise à jour : {e}");
    }
}

/// Le choix du delai, dans le menu de notification et dans le menu surgissant
/// de la pastille : les memes identifiants, le meme gestionnaire (global).
fn sous_menu_differe(app: &tauri::AppHandle) -> tauri::Result<Submenu<tauri::Wry>> {
    let delais = DELAIS
        .iter()
        .map(|s| MenuItem::with_id(app, format!("differe-{s}"), format!("{s} secondes"), true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = delais.iter().map(|m| m as _).collect();
    Submenu::with_items(app, "Écran entier dans…", true, &refs)
}

/// Les entrees d'un menu « Filmer » (icone, pastille).
fn items_filmer(app: &tauri::AppHandle, scenes: &[(i64, String)]) -> tauri::Result<Vec<MenuItemKind<tauri::Wry>>> {
    use crate::video::scenes::{entrees_filmer, Entree, LIBELLE_AUCUNE, LIBELLE_ZONE, PREFIXE_MENU};
    entrees_filmer(scenes)
        .into_iter()
        .map(|entree| {
            Ok(match entree {
                Entree::Zone => MenuItemKind::MenuItem(MenuItem::with_id(app, "video", LIBELLE_ZONE, true, None::<&str>)?),
                Entree::Separateur => MenuItemKind::Predefined(PredefinedMenuItem::separator(app)?),
                Entree::Aucune => MenuItemKind::MenuItem(MenuItem::with_id(app, "scene-aucune", LIBELLE_AUCUNE, false, None::<&str>)?),
                Entree::Scene { id, libelle } => MenuItemKind::MenuItem(MenuItem::with_id(app, format!("{PREFIXE_MENU}{id}"), libelle, true, None::<&str>)?),
            })
        })
        .collect()
}

fn refs(items: &[MenuItemKind<tauri::Wry>]) -> Vec<&dyn IsMenuItem<tauri::Wry>> {
    items.iter().map(|m| m as _).collect()
}

/// La tete du sous-menu (la zone, le separateur) ne change jamais : elle reste.
/// ponytail: on ne refait que la queue ; un separateur s'ajoute sous l'identifiant que
/// Windows peut ignorer (`AppendMenuW`, `MF_SEPARATOR`), et `RemoveMenu` le cherche par lui.
const TETE_FILMER: usize = 2;

/// Vide la queue du sous-menu et la remplit. Chaque appel de menu passe par le thread
/// principal et l'attend : jamais depuis lui, verrou `refaire` tenu.
fn refaire_filmer(app: &tauri::AppHandle, entrees: &EntreesDuMenu, scenes: &[(i64, String)]) -> Result<(), String> {
    let refaire = || -> tauri::Result<()> {
        for ancienne in entrees.filmer.items()?.into_iter().skip(TETE_FILMER) {
            entrees.filmer.remove(&ancienne)?;
        }
        for neuve in items_filmer(app, scenes)?.into_iter().skip(TETE_FILMER) {
            entrees.filmer.append(&neuve)?;
        }
        Ok(())
    };
    refaire().map_err(|e| format!("Menu « Filmer » impossible à refaire : {e}"))
}

pub fn rafraichir_filmer_avec(app: &tauri::AppHandle, scenes: &[(i64, String)]) -> Result<(), String> {
    let entrees = app.try_state::<EntreesDuMenu>().ok_or("Menu de notification absent.")?;
    let _tour = entrees.refaire.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    refaire_filmer(app, &entrees, scenes)
}

/// Les scenes ont change : le sous-menu suit la base, lue sous le meme verrou.
pub fn rafraichir_filmer(app: &tauri::AppHandle) -> Result<(), String> {
    let entrees = app.try_state::<EntreesDuMenu>().ok_or("Menu de notification absent.")?;
    let _tour = entrees.refaire.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let scenes = crate::video::noms_des_scenes(app)?;
    refaire_filmer(app, &entrees, &scenes)
}

/// Les libelles du sous-menu, lus dans le menu natif de Windows.
#[cfg(windows)]
fn textes_du_menu(hmenu: isize) -> Vec<String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetMenuItemCount, GetMenuStringW, HMENU, MF_BYPOSITION};
    let menu = hmenu as HMENU;
    // SAFETY: HMENU vivant tant que `EntreesDuMenu` tient le sous-menu ; tampon borne a sa longueur.
    let n = unsafe { GetMenuItemCount(menu) };
    (0..n.max(0) as u32)
        .map(|i| {
            let mut tampon = [0u16; 128];
            let lu = unsafe { GetMenuStringW(menu, i, tampon.as_mut_ptr(), tampon.len() as i32, MF_BYPOSITION) };
            String::from_utf16_lossy(&tampon[..lu.clamp(0, 127) as usize])
        })
        .collect()
}

/// Sonde et parcours seulement (exe de debug, instance dev) : avec `noms`, le sous-menu
/// « Filmer » est refait avec ces fausses scenes ; rend ses libelles lus dans le menu
/// natif (`GetMenuStringW`), pas dans le modele de muda.
// ponytail: une couture de test, refusee ailleurs.
#[tauri::command]
pub async fn pastille_menu_filmer_lu(app: tauri::AppHandle, noms: Option<Vec<String>>) -> Result<Vec<String>, String> {
    let permis = cfg!(debug_assertions) && app.state::<AppState>().dev;
    if !permis {
        return Err("Réservé au parcours.".to_string());
    }
    crate::hors_thread_principal(app, move |app| {
        if let Some(noms) = noms {
            let fausses: Vec<(i64, String)> = noms.into_iter().enumerate().map(|(i, n)| (i as i64 + 1, n)).collect();
            rafraichir_filmer_avec(&app, &fausses)?;
        }
        // Un `#[cfg]` sur une expression de fin de bloc ne compile pas : deux `let`.
        #[cfg(windows)]
        let lus = {
            let entrees = app.try_state::<EntreesDuMenu>().ok_or("Menu de notification absent.")?;
            textes_du_menu(entrees.filmer.hpopupmenu().map_err(|e| e.to_string())?)
        };
        #[cfg(not(windows))]
        let lus: Vec<String> = Vec::new();
        Ok(lus)
    })
    .await
}

/// L'icone de la zone de notification : un clic remontre la pastille, le menu
/// porte les captures, la galerie et « Quitter ».
pub fn icone_de_notification(app: &tauri::AppHandle) -> tauri::Result<()> {
    let zone = MenuItem::with_id(app, "zone", "Capturer une zone", true, None::<&str>)?;
    let gardee = captures::derniere_zone_gardee(app).is_some();
    let derniere = MenuItem::with_id(app, "derniere", "Reprendre la dernière zone", gardee, None::<&str>)?;
    let ecran = MenuItem::with_id(app, "ecran", "Écran entier", true, None::<&str>)?;
    let differe = sous_menu_differe(app)?;
    let annuler = MenuItem::with_id(app, "differe-annuler", "Annuler la capture différée", false, None::<&str>)?;
    let galerie = MenuItem::with_id(app, "galerie", "Galerie", true, None::<&str>)?;
    let reglages = MenuItem::with_id(app, "reglages", "Réglages", true, None::<&str>)?;
    let quitter = MenuItem::with_id(app, "quitter", "Quitter", true, None::<&str>)?;
    let separateur = PredefinedMenuItem::separator(app)?;
    let texte = MenuItem::with_id(app, "texte", "Copier le texte d'une zone", true, None::<&str>)?;
    // 0.19 : « Filmer » liste la zone puis les scenes ; la base se lit ici, au demarrage.
    let scenes_au_demarrage = crate::video::noms_des_scenes(app).unwrap_or_default();
    let filmer = Submenu::with_items(app, "Filmer", true, &refs(&items_filmer(app, &scenes_au_demarrage)?))?;
    let entrees: [&dyn IsMenuItem<tauri::Wry>; POSITION_MAJ + 1] = [&zone, &texte, &filmer, &derniere, &ecran, &differe, &annuler, &separateur, &galerie, &reglages, &quitter];
    let menu = Menu::with_items(app, &entrees)?;
    let maj = MenuItem::with_id(app, "maj", "Installer la mise à jour", true, None::<&str>)?;
    app.manage(EntreesDuMenu { derniere, annuler, filmer, refaire: std::sync::Mutex::new(()), menu: menu.clone(), maj, maj_posee: std::sync::Mutex::new(false) });
    let mut icone = TrayIconBuilder::with_id(ICONE)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(INFOBULLE)
        .on_menu_event(|app, evenement| {
            // Pris a chaque evenement : un releve laisse par un menu ferme sans choix
            // ne sert pas au suivant.
            let devant = devant_du_menu();
            match evenement.id.as_ref() {
                "zone" => captures::lancer_selection(app),
                "texte" => captures::lancer_selection_texte(app),
                "video" => captures::lancer_selection_video(app, devant),
                "derniere" => captures::lancer_derniere_zone(app),
                "ecran" => captures::lancer_ecran_entier(app),
                // Hors du thread principal : l'annulation grise une entree du menu,
                // ce qui attend le thread principal.
                "differe-annuler" => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move { captures::annuler_differe(&app) });
                }
                section @ ("galerie" | "reglages") => {
                    if let Err(motif) = ouvrir_principale(app, section) {
                        eprintln!("{motif}");
                    }
                }
                "quitter" => app.exit(0),
                "maj" => crate::maj::installer_detache(app),
                // Le menu surgissant d'une epingle (ADR-011) : ses clics arrivent ici, comme ceux du minuteur.
                autre if autre.starts_with(crate::epingles::PREFIXE) => crate::epingles::sur_menu(app, autre),
                autre => {
                    if let Some(id) = crate::video::scenes::id_du_menu(autre) {
                        crate::video::lancer_scene_detache(app, id, devant);
                    } else if let Some(secondes) = autre.strip_prefix("differe-").and_then(|s| s.parse().ok()) {
                        captures::lancer_differe(app, secondes);
                    }
                }
            }
        })
        .on_tray_icon_event(|icone, evenement| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = evenement {
                montrer(icone.app_handle());
            }
        });
    if let Some(image) = app.default_window_icon() {
        icone = icone.icon(image.clone());
    }
    icone.build(app)?;
    Ok(())
}

/// Une zone vient d'etre gardee : « Reprendre la derniere zone » s'active.
/// Jamais depuis le thread principal (voir `run_item_main_thread` de Tauri).
pub fn derniere_zone_prete(app: &tauri::AppHandle) {
    if let Some(entrees) = app.try_state::<EntreesDuMenu>() {
        let _ = entrees.derniere.set_enabled(true);
    }
}

/// Le compte a rebours du differe dans le menu et l'infobulle de l'icone, que
/// l'on voit aussi pastille rangee. Jamais depuis le thread principal.
pub fn suivre_differe(app: &tauri::AppHandle, reste: Option<u32>) {
    if let Some(entrees) = app.try_state::<EntreesDuMenu>() {
        let _ = entrees.annuler.set_enabled(reste.is_some());
    }
    if let Some(icone) = app.tray_by_id(ICONE) {
        let texte = reste.map_or(INFOBULLE.to_string(), |n| format!("EquiShot : capture dans {n} s"));
        let _ = icone.set_tooltip(Some(texte));
    }
}

// ---------------------------------------------------------------------------
// Commandes
// ---------------------------------------------------------------------------

/// Le minuteur de la pastille : le choix du delai en menu surgissant, sous le
/// curseur. Le clic part au gestionnaire du menu de notification.
#[tauri::command]
pub async fn pastille_menu_differe(app: tauri::AppHandle) -> Result<(), String> {
    let fenetre = app.get_webview_window(ETIQUETTE).ok_or("La pastille est introuvable.")?;
    let menu = sous_menu_differe(&app).map_err(|e| format!("Menu du différé impossible : {e}"))?;
    fenetre.popup_menu(&menu).map_err(|e| format!("Menu du différé impossible à ouvrir : {e}"))
}

/// « Vidéo » de la pastille : sans scene, la selection comme avant ; avec des scenes,
/// le menu « Filmer » sous le curseur (la zone, puis les scenes). Le clic part au
/// gestionnaire global de l'icone.
#[tauri::command]
pub async fn pastille_filmer(app: tauri::AppHandle) -> Result<(), String> {
    let scenes = crate::hors_thread_principal(app.clone(), |app| crate::video::noms_des_scenes(&app)).await?;
    // Pendant un enregistrement, « Vidéo » l'arrete, comme avant les scenes.
    if scenes.is_empty() || crate::video::en_cours(&app) {
        return captures::ouvrir_selection(app, captures::Mode::Video).await;
    }
    let fenetre = app.get_webview_window(ETIQUETTE).ok_or("La pastille est introuvable.")?;
    let items = items_filmer(&app, &scenes).map_err(|e| format!("Menu « Filmer » impossible : {e}"))?;
    let filmer = Submenu::with_items(&app, "Filmer", true, &refs(&items)).map_err(|e| format!("Menu « Filmer » impossible : {e}"))?;
    if let Ok(mut d) = DEVANT_MENU.lock() {
        *d = Some((crate::fenetres::premier_plan(), std::time::Instant::now()));
    }
    fenetre.popup_menu(&filmer).map_err(|e| format!("Menu « Filmer » impossible à ouvrir : {e}"))
}

/// Le tiret de la pastille : elle se range dans la zone de notification. Le
/// raccourci marche toujours.
#[tauri::command]
pub fn pastille_ranger(app: tauri::AppHandle) -> Result<(), String> {
    let fenetre = app.get_webview_window(ETIQUETTE).ok_or("La pastille est introuvable.")?;
    RANGEE.store(true, std::sync::atomic::Ordering::SeqCst);
    crate::fenetres::montrer_sans_activer(&fenetre, false);
    // La pile se range avec elle si le reglage le veut.
    crate::vignettes::suivre(&app);
    Ok(())
}

/// Ce que le clic sur l'icone de la zone de notification appelle.
#[tauri::command]
pub fn pastille_montrer(app: tauri::AppHandle) {
    montrer(&app);
}

/// Vrai si la pastille est contractee. Asynchrone : la base n'est jamais lue sur
/// le thread principal.
#[tauri::command]
pub async fn pastille_contractee(app: tauri::AppHandle) -> bool {
    contractee(&app)
}

/// Contracte la pastille (icones seules) ou la deplie, le bord gauche fixe, sans
/// la laisser deborder de l'ecran, et retient l'etat pour le prochain lancement.
#[tauri::command]
pub async fn pastille_contracter(app: tauri::AppHandle, contractee: bool) -> Result<(), String> {
    let fenetre = app.get_webview_window(ETIQUETTE).ok_or("La pastille est introuvable.")?;
    let largeur = largeur(contractee);
    fenetre
        .set_size(tauri::LogicalSize::new(largeur, HAUTEUR))
        .map_err(|e| format!("Pastille impossible à redimensionner : {e}"))?;
    // La largeur se calcule plutot qu'elle ne se relit : le redimensionnement peut
    // ne pas etre encore applique quand on lirait la taille.
    let position = fenetre.outer_position().map_err(|e| e.to_string())?;
    if let Some(ecran) = app.monitor_from_point(position.x as f64 + 8.0, position.y as f64 + 8.0).ok().flatten() {
        let zone = ecran.work_area();
        let x = x_dans_zone(position.x, (largeur * ecran.scale_factor()).round() as i32, zone.position.x, zone.size.width as i32);
        if x != position.x {
            fenetre.set_position(tauri::PhysicalPosition::new(x, position.y)).map_err(|e| e.to_string())?;
        }
    }
    app.state::<AppState>()
        .with_conn(|conn| db::regler(conn, CLE_CONTRACTEE, if contractee { "1" } else { "0" }))
}

/// Asynchrone : une commande synchrone qui cree une fenetre bloque Windows.
#[tauri::command]
pub async fn principale_ouvrir(app: tauri::AppHandle, section: String) -> Result<(), String> {
    let section = if section == "reglages" { "reglages" } else { "galerie" };
    ouvrir_principale(&app, section)
}

/// Le crayon d'une vignette : la galerie s'ouvre sur la fiche de la capture,
/// en reedition (`#modifier-<id>`, lu par la galerie).
#[tauri::command]
pub async fn principale_modifier(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    ouvrir_principale(&app, &format!("modifier-{id}"))
}

#[tauri::command]
pub fn equishot_quitter(app: tauri::AppHandle) {
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_premier_plan_se_releve_avant_le_menu_filmer() {
        let code = include_str!("pastille.rs").split("#[cfg(test)]").next().unwrap_or_default();
        let releve = code.find("*d = Some((crate::fenetres::premier_plan()").expect("relevé");
        let menu = code.find("fenetre.popup_menu(&filmer)").expect("menu");
        assert!(releve < menu, "muda prend le premier plan avant TrackPopupMenu");
    }

    #[test]
    fn position_lue_ou_ignoree() {
        assert_eq!(lire_position("120,-40"), Some((120, -40)), "un écran à gauche a des x négatifs");
        assert_eq!(lire_position(" 3 , 4 "), Some((3, 4)));
        assert_eq!(lire_position(""), None);
        assert_eq!(lire_position("a,b"), None);
    }

    #[test]
    fn pastille_depliee_reste_dans_l_ecran() {
        // Ecran de 1920 a partir de 0 : depliee a 404 px pres du bord droit, elle recule.
        assert_eq!(x_dans_zone(1800, 404, 0, 1920), 1516);
        assert_eq!(x_dans_zone(100, 404, 0, 1920), 100, "loin du bord, le bord gauche ne bouge pas");
        // Ecran a gauche, x negatifs.
        assert_eq!(x_dans_zone(-200, 404, -1920, 1920), -404);
        assert_eq!(x_dans_zone(-2000, 404, -1920, 1920), -1920, "jamais a gauche de l'ecran");
        assert!(largeur(true) < largeur(false));
    }
}
