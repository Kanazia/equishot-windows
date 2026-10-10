//! La bulle de camera (ADR-017, 0.18) : une fenetre ronde a part, filmee avec
//! l'ecran, montree sans activer et jamais touchee par un setter de drapeau de
//! tao. Rust pose l'autorisation de la camera, fait la geometrie en pixels
//! physiques ; la page allume la camera, enumere et glisse.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

use super::moteur::Reglages;
use crate::fenetres;

pub const ETIQUETTE: &str = "video-camera";
pub const TITRE: &str = "EquiShot, caméra";
/// Emis vers la page de la bulle a chaque reglage : camera, miroir, diametre.
pub const EVENEMENT_BULLE: &str = "video://bulle";
pub const ZONE_TROP_PETITE: &str = "Zone trop petite pour la bulle de caméra.";

/// Une camera vue par la page de la bulle (`enumerateDevices`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Camera {
    pub id: String,
    pub nom: String,
}

/// Ce que la page recoit : la camera a ouvrir, le miroir, le diametre en pixels CSS.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBulle {
    pub camera_id: Option<String>,
    pub miroir: bool,
    pub diametre: f64,
}

/// L'etat de la bulle ouverte, pixels physiques du bureau. `depart` : la place
/// au debut d'un glisser (les deltas de la page sont cumules depuis l'appui).
pub struct Bulle {
    pub autorisee: Arc<AtomicBool>,
    pub taille: Taille,
    pub diametre: i32,
    pub pos: (i32, i32),
    pub depart: Option<(i32, i32)>,
}

pub fn config(r: &Reglages, taille: Taille) -> ConfigBulle {
    ConfigBulle { camera_id: r.camera_id.clone(), miroir: r.camera_miroir, diametre: taille.diametre_css() }
}

/// Vrai des que `drapeau` l'est, faux passe `delai`. Bloquant : hors du thread principal.
// ponytail: une boucle de 20 ms plutot qu'un Condvar ; une seule attente par bulle.
pub fn attendre_le_drapeau(drapeau: &AtomicBool, delai: Duration) -> bool {
    let fin = Instant::now() + delai;
    while !drapeau.load(Ordering::Acquire) {
        if Instant::now() >= fin {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

/// Ouvre la bulle dans `zone` : transparente, sans cadre ni ombre, toujours devant,
/// hors de la barre des taches, non focalisable, **pas** protegee (elle est filmee),
/// creee cachee puis montree sans activer. `Ok(None)` : meme la petite ne tient pas.
pub fn ouvrir(app: &tauri::AppHandle, zone: Rect, echelle: f64, r: &Reglages) -> Result<Option<Bulle>, String> {
    let Some((taille, diametre)) = taille_possible(r.camera_taille, zone, echelle) else { return Ok(None) };
    let pos = place_initiale(zone, diametre, r.camera_place, echelle);
    let css = taille.diametre_css();
    let bulle = WebviewWindowBuilder::new(app, ETIQUETTE, WebviewUrl::App("camera.html".into()))
        .title(TITRE)
        .inner_size(css, css)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focusable(false)
        // Sinon wry appelle `MoveFocus` a la creation : la bulle prenait le premier plan au
        // panneau, et Entree n'y arrivait plus (etape 30 f, 7 octobre).
        .focused(false)
        .visible(false)
        .build()
        .map_err(|e| format!("Bulle de caméra impossible à ouvrir : {e}"))?;
    fenetres::sans_menu_alt(&bulle);
    fenetres::hors_occlusion(&bulle);
    if let Err(e) = bulle.set_position(tauri::PhysicalPosition::new(pos.0, pos.1)) {
        // Fermee tout de suite : cachee, elle garderait l'etiquette et sa page sans gestionnaire d'autorisation.
        let _ = bulle.close();
        return Err(format!("Bulle de caméra impossible à placer : {e}"));
    }
    let autorisee = Arc::new(AtomicBool::new(false));
    poser_autorisation(&bulle, autorisee.clone());
    fenetres::montrer_sans_activer(&bulle, true);
    Ok(Some(Bulle { autorisee, taille, diametre, pos, depart: None }))
}

/// Ferme la bulle si elle est la : la WebView detruite libere la camera (la page
/// arrete aussi ses pistes sur `pagehide`).
pub fn fermer(app: &tauri::AppHandle) {
    if let Some(bulle) = app.get_webview_window(ETIQUETTE) {
        fenetres::avant_fermeture(&bulle);
        let _ = bulle.close();
    }
}

/// `(x, y, largeur, hauteur)`, pixels physiques du bureau.
pub type Rect = (i32, i32, i32, i32);

/// Place par defaut : a 24 px CSS du bord gauche et du bord bas de la zone.
pub const MARGE_CSS: f64 = 24.0;

/// Le diametre de la bulle, en pixels CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Taille {
    Petite,
    #[default]
    Moyenne,
    Grande,
}

impl Taille {
    pub fn diametre_css(self) -> f64 {
        match self {
            Taille::Petite => 160.0,
            Taille::Moyenne => 240.0,
            Taille::Grande => 360.0,
        }
    }

    pub fn en_px(self, echelle: f64) -> i32 {
        (self.diametre_css() * echelle).round() as i32
    }

    pub fn plus_petite(self) -> Option<Taille> {
        match self {
            Taille::Grande => Some(Taille::Moyenne),
            Taille::Moyenne => Some(Taille::Petite),
            Taille::Petite => None,
        }
    }
}

/// La taille retenue et son diametre en pixels : celle voulue si elle tient dans
/// la zone, sinon la taille en dessous, jusqu'a « petite » ; `None` quand meme
/// la petite ne tient pas (la bulle est refusee, le panneau le dit).
pub fn taille_possible(voulue: Taille, zone: Rect, echelle: f64) -> Option<(Taille, i32)> {
    let mut taille = voulue;
    loop {
        let d = taille.en_px(echelle);
        if d <= zone.2 && d <= zone.3 {
            return Some((taille, d));
        }
        taille = taille.plus_petite()?;
    }
}

/// Aucun `clamp` : `Ord::clamp` panique si la zone est plus etroite que la bulle.
pub fn borner(p: (i32, i32), diametre: i32, zone: Rect) -> (i32, i32) {
    (p.0.min(zone.0 + zone.2 - diametre).max(zone.0), p.1.min(zone.1 + zone.3 - diametre).max(zone.1))
}

/// Ou poser la bulle : a la proportion gardee de l'espace libre, ou en bas a
/// gauche a 24 px CSS des bords ; toujours bornee a la zone.
pub fn place_initiale(zone: Rect, diametre: i32, proportion: Option<(f64, f64)>, echelle: f64) -> (i32, i32) {
    let voulu = match proportion {
        Some((px, py)) => {
            // Bornes constantes 0 et 1 : ce `clamp` de f64 ne peut pas se croiser.
            let part = |libre: i32, p: f64| (libre as f64 * p.clamp(0.0, 1.0)).round() as i32;
            (zone.0 + part(zone.2 - diametre, px), zone.1 + part(zone.3 - diametre, py))
        }
        None => {
            let marge = (MARGE_CSS * echelle).round() as i32;
            (zone.0 + marge, zone.1 + zone.3 - marge - diametre)
        }
    };
    borner(voulu, diametre, zone)
}

/// La place en proportion de l'espace libre (0 a 1 sur chaque axe) : ce que les
/// reglages gardent, et qu'une autre zone ou une autre taille relit.
pub fn proportion(p: (i32, i32), diametre: i32, zone: Rect) -> (f64, f64) {
    let part = |pos: i32, debut: i32, libre: i32| if libre <= 0 { 0.0 } else { ((pos - debut) as f64 / libre as f64).clamp(0.0, 1.0) };
    (part(p.0, zone.0, zone.2 - diametre), part(p.1, zone.1, zone.3 - diametre))
}

/// Seule la page de la bulle d'EquiShot recoit la camera : `tauri.localhost` en
/// production, `localhost` avec le serveur Vite. Le chemin est compare apres
/// normalisation par `Url` (un `..` ne passe pas).
pub fn camera_autorisee(uri: &str) -> bool {
    let Ok(url) = tauri::Url::parse(uri) else { return false };
    url.scheme() == "http" && matches!(url.host_str(), Some("tauri.localhost") | Some("localhost")) && url.path() == "/camera.html"
}

/// Pose le gestionnaire `PermissionRequested` de WebView2 sur la bulle : la camera
/// est accordee a `/camera.html` (adresse lue par `Source()` de la WebView, `Uri`
/// ne donnant que l'origine), rien d'autre n'est touche (modele : `wry` 0.55.1,
/// `webview2/mod.rs:497-515`). `with_webview` est poste au fil principal apres
/// `build()` : `pose` passe a vrai quand le gestionnaire est la, et la page attend
/// ce drapeau (`video_bulle_config`) avant `getUserMedia`. Aucun dialogue ne peut
/// donc s'ouvrir avant lui.
pub fn poser_autorisation<R: tauri::Runtime>(fenetre: &tauri::WebviewWindow<R>, pose: Arc<AtomicBool>) {
    let _ = fenetre.with_webview(move |webview| {
        #[cfg(windows)]
        {
            use webview2_com::Microsoft::Web::WebView2::Win32::{
                ICoreWebView2PermissionRequestedEventArgs3, COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_CAMERA,
                COREWEBVIEW2_PERMISSION_STATE_ALLOW,
            };
            use windows::core::Interface;
            use webview2_com::{take_pwstr, PermissionRequestedEventHandler};
            let mut jeton = 0i64;
            // SAFETY: appels COM sur le fil principal, celui de la WebView ; le gestionnaire
            // est tenu par WebView2 tant que la WebView vit, et la fermeture ne le lit plus.
            let resultat = unsafe {
                webview.controller().CoreWebView2().and_then(|coeur| {
                    coeur.add_PermissionRequested(
                        &PermissionRequestedEventHandler::create(Box::new(|demandeur, args| {
                            let (Some(demandeur), Some(args)) = (demandeur, args) else { return Ok(()) };
                            let mut genre = COREWEBVIEW2_PERMISSION_KIND::default();
                            args.PermissionKind(&mut genre)?;
                            // `args.Uri` n'est que l'origine (`http://tauri.localhost/`, mesure le
                            // 7 octobre 2026) : l'adresse de la page vient de la WebView qui demande.
                            let mut page = windows::core::PWSTR::null();
                            demandeur.Source(&mut page)?;
                            if genre == COREWEBVIEW2_PERMISSION_KIND_CAMERA && camera_autorisee(&take_pwstr(page)) {
                                // Garde par defaut dans le profil, l'accord vaudrait pour toute l'origine
                                // (galerie, epingles) aux sessions suivantes. Un runtime sans `Args3` ne garde rien.
                                if let Ok(args3) = args.cast::<ICoreWebView2PermissionRequestedEventArgs3>() {
                                    args3.SetSavesInProfile(false)?;
                                }
                                args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
                            }
                            Ok(())
                        })),
                        &mut jeton,
                    )
                })
            };
            if resultat.is_ok() {
                pose.store(true, Ordering::Release);
            }
        }
        #[cfg(not(windows))]
        {
            let _ = webview;
            pose.store(true, Ordering::Release);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZONE: Rect = (100, 100, 800, 600);

    #[test]
    fn attendre_le_drapeau_borne_a_deux_secondes() {
        let pose = AtomicBool::new(true);
        assert!(attendre_le_drapeau(&pose, std::time::Duration::from_secs(2)));
        let jamais = AtomicBool::new(false);
        let debut = std::time::Instant::now();
        assert!(!attendre_le_drapeau(&jamais, std::time::Duration::from_millis(60)));
        assert!(debut.elapsed() < std::time::Duration::from_millis(500), "rend la main des le delai passe");
    }

    #[test]
    fn config_prend_la_camera_le_miroir_et_le_diametre() {
        let r = Reglages { camera: true, camera_id: Some("c1".into()), camera_miroir: false, ..Reglages::default() };
        assert_eq!(config(&r, Taille::Grande), ConfigBulle { camera_id: Some("c1".into()), miroir: false, diametre: 360.0 });
    }

    #[test]
    fn la_taille_descend_jusqu_a_petite() {
        assert_eq!(taille_possible(Taille::Grande, ZONE, 1.0), Some((Taille::Grande, 360)));
        assert_eq!(taille_possible(Taille::Grande, (0, 0, 300, 500), 1.0), Some((Taille::Moyenne, 240)));
        assert_eq!(taille_possible(Taille::Grande, (0, 0, 200, 200), 1.0), Some((Taille::Petite, 160)));
        // 150 % : la petite fait 240 px, elle tient encore dans 240.
        assert_eq!(taille_possible(Taille::Moyenne, (0, 0, 240, 400), 1.5), Some((Taille::Petite, 240)));
    }

    #[test]
    fn zone_trop_petite_refuse_la_bulle() {
        assert_eq!(taille_possible(Taille::Petite, (0, 0, 100, 100), 1.0), None);
        assert_eq!(taille_possible(Taille::Grande, (0, 0, 159, 2000), 1.0), None);
    }

    #[test]
    fn place_par_defaut_en_bas_a_gauche_a_24_px() {
        assert_eq!(place_initiale(ZONE, 240, None, 1.0), (124, 436));
        assert_eq!(place_initiale(ZONE, 240, None, 2.0), (148, 412));
    }

    #[test]
    fn place_par_proportion_de_l_espace_libre() {
        assert_eq!(place_initiale(ZONE, 240, Some((0.5, 0.0)), 1.0), (380, 100));
        assert_eq!(place_initiale(ZONE, 240, Some((1.0, 1.0)), 1.0), (660, 460));
        // Une proportion hors de 0..1 (fichier modifie a la main) est ramenee.
        assert_eq!(place_initiale(ZONE, 240, Some((7.0, -3.0)), 1.0), (660, 100));
    }

    #[test]
    fn borner_garde_la_bulle_entierement_dans_la_zone() {
        assert_eq!(borner((10_000, -50), 240, ZONE), (660, 100));
        assert_eq!(borner((300, 300), 240, ZONE), (300, 300));
    }

    #[test]
    fn borner_sans_panique_zone_plus_etroite_que_la_bulle() {
        assert_eq!(borner((50, 50), 160, (0, 0, 100, 100)), (0, 0));
    }

    #[test]
    fn proportion_inverse_de_la_place() {
        assert_eq!(proportion((380, 100), 240, ZONE), (0.5, 0.0));
        assert_eq!(proportion((660, 460), 240, ZONE), (1.0, 1.0));
        assert_eq!(proportion((0, 0), 160, (0, 0, 100, 100)), (0.0, 0.0));
    }

    #[test]
    fn seule_la_page_de_la_bulle_est_autorisee() {
        assert!(camera_autorisee("http://tauri.localhost/camera.html"));
        assert!(camera_autorisee("http://localhost:1430/camera.html"));
        assert!(!camera_autorisee("http://tauri.localhost/enregistrement.html"));
        assert!(!camera_autorisee("http://tauri.localhost/camera.html/../index.html"));
        assert!(!camera_autorisee("https://exemple.fr/camera.html"));
        assert!(!camera_autorisee("pas un uri"));
    }

    /// tao 0.35.3 : un setter de drapeau (`apply_diff`) fait `SW_HIDE` sur une fenetre qu'il
    /// croit cachee. La bulle est montree par `montrer_sans_activer` : aucun setter, jamais.
    #[test]
    fn aucun_setter_de_drapeau_de_tao_sur_la_bulle() {
        let code = include_str!("bulle.rs").split("#[cfg(test)]").next().unwrap_or_default();
        for setter in ["set_focusable(", "set_ignore_cursor_events(", "set_always_on_top(", "set_resizable(", "set_decorations(", "set_focus(", "show(", "hide("] {
            assert!(!code.contains(&format!("bulle.{setter}")), "bulle.{setter}");
        }
    }
}
