//! EquiShot : la capture d'ecran en application autonome (ADR-001).
//!
//! Une pastille flottante, une icone dans la zone de notification, un raccourci
//! systeme. La fenetre « EquiShot » (galerie et reglages) n'existe que quand on
//! l'ouvre.

mod captures;
mod db;
mod demarrage;
mod epingles;
mod fenetres;
mod maj;
mod pastille;
pub mod video;
mod vignettes;

use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;
use tauri::Manager;

pub struct AppState {
    db: Mutex<Connection>,
    pub data_dir: PathBuf,
    pub dev: bool,
}

impl AppState {
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        let conn = self.db.lock().map_err(|_| "La base locale est dans un état incohérent.".to_string())?;
        f(&conn)
    }
}

/// Toute commande qui touche un disque, capture un ecran ou encode une image
/// passe par ici : une commande sur le thread principal figerait la pastille.
pub(crate) async fn hors_thread_principal<T: Send + 'static>(
    app: tauri::AppHandle,
    travail: impl FnOnce(tauri::AppHandle) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(move || travail(app))
        .await
        .map_err(|e| format!("Travail interrompu : {e}"))?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Instance {
    dev: bool,
    data_dir: String,
}

#[tauri::command]
fn equishot_instance(etat: tauri::State<'_, AppState>) -> Instance {
    Instance { dev: etat.dev, data_dir: etat.data_dir.to_string_lossy().into_owned() }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let dev = match db::instance_dev() {
        Ok(dev) => dev,
        Err(motif) => {
            eprintln!("{motif}");
            std::process::exit(2);
        }
    };
    // L'instance de developpement a son debogueur, sauf si le lanceur en pose un.
    if dev && std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").is_none() {
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", format!("--remote-debugging-port={}", db::PORT_DEV));
    }
    let mut contexte = tauri::generate_context!();
    // Un autre identifiant, et tout suit : dossier de donnees, profil WebView2,
    // verrou d'instance unique (lu dans la source du greffon 2.4.5). La
    // production et l'instance dev tournent cote a cote sans se voir.
    if dev {
        contexte.config_mut().identifier = db::IDENTIFIANT_DEV.to_string();
    }

    tauri::Builder::default()
        // En premier : un second lancement s'arrete avant d'ouvrir quoi que ce soit.
        .plugin(tauri_plugin_single_instance::init(|app, _arguments, _dossier| pastille::montrer(app)))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Aucune combinaison donnee ici : le `Builder` les enregistre avec un `?`,
        // une combinaison deja prise empecherait EquiShot de demarrer.
        // `captures::initialiser` l'enregistre a chaud.
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, raccourci, evenement| {
                    if evenement.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        captures::sur_raccourci(app, raccourci);
                    }
                })
                .build(),
        )
        .setup(move |app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let conn = db::ouvrir(&data_dir.join("equishot.db"))?;
            app.manage(AppState { db: Mutex::new(conn), data_dir, dev });
            app.manage(captures::EtatCaptures::default());
            captures::choisir_dossier(app.handle());
            app.manage(vignettes::EtatVignettes::default());
            app.manage(epingles::EtatEpingles::default());
            app.manage(video::EtatVideo::default());
            app.manage(maj::EtatMajs::default());
            let handle = app.handle().clone();
            pastille::creer(&handle)?;
            pastille::icone_de_notification(&handle)?;
            maj::demarrer(&handle);
            captures::initialiser(&handle);
            let demarre = handle.clone();
            tauri::async_runtime::spawn_blocking(move || demarrage::initialiser(&demarre));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            equishot_instance,
            pastille::pastille_ranger,
            pastille::pastille_menu_differe,
            pastille::pastille_filmer,
            pastille::pastille_montrer,
            pastille::pastille_contractee,
            pastille::pastille_contracter,
            pastille::pastille_menu_filmer_lu,
            pastille::principale_ouvrir,
            pastille::principale_modifier,
            pastille::equishot_quitter,
            captures::captures_etat_raccourci,
            captures::captures_suspendre_raccourci,
            captures::captures_regler_raccourci,
            captures::captures_sonde_souris,
            captures::captures_ouvrir_selection,
            captures::captures_ouvrir_selection_texte,
            captures::captures_ouvrir_selection_video,
            captures::captures_selection_texte,
            captures::captures_copier_texte,
            captures::captures_copier_couleur,
            captures::captures_selection_infos,
            captures::captures_selection_prete,
            captures::captures_selection_prendre,
            captures::captures_selection_fenetres,
            captures::captures_selection_loupe,
            captures::captures_selection_curseur,
            captures::captures_selection_garder,
            captures::captures_selection_annuler,
            captures::captures_selection_jeter,
            captures::captures_selection_defiler,
            captures::captures_selection_arreter,
            captures::captures_selection_enregistrer,
            captures::captures_selection_ecran_entier,
            captures::captures_ecran_entier,
            captures::captures_differer,
            captures::captures_differe_annuler,
            captures::captures_lister,
            captures::captures_noter,
            captures::captures_renommer,
            captures::captures_modifier,
            captures::captures_supprimer,
            captures::captures_copier,
            captures::captures_copier_plusieurs,
            captures::captures_dossier,
            captures::captures_ouvrir_dossier,
            captures::captures_montrer,
            captures::captures_menu,
            captures::captures_choisir_dossier,
            captures::captures_deplacer_dossier,
            demarrage::demarrage_lire,
            demarrage::demarrage_regler,
            maj::maj_etat,
            maj::maj_rechercher,
            maj::maj_installer,
            vignettes::vignette_glisser,
            vignettes::vignettes_lister,
            vignettes::vignette_fermer,
            vignettes::vignettes_tout_fermer,
            vignettes::vignettes_actives,
            vignettes::vignettes_activer,
            vignettes::vignettes_avec_pastille,
            vignettes::vignettes_ranger_avec_pastille,
            epingles::epingle_infos,
            epingles::epingle_taille,
            epingles::epingle_copier,
            epingles::epingle_menu,
            epingles::epingle_fermer,
            epingles::epingle_ouvrir,
            video::video_demarrer,
            video::video_arreter,
            video::video_abandonner,
            video::video_pauser,
            video::video_etat,
            video::video_reglages,
            video::video_regler,
            video::video_dossier,
            video::video_ouvrir_dossier,
            video::video_gif,
            video::video_peripheriques,
            video::video_lancer,
            video::video_bulle_config,
            video::video_bulle_cameras,
            video::video_bulle_perdue,
            video::video_bulle_deplacer,
            video::video_ecrans,
            video::video_sonde_viser,
            video::video_scenes,
            video::video_scene_enregistrer,
            video::video_scene_renommer,
            video::video_scene_supprimer,
            video::video_scene_appliquer,
        ])
        .build(contexte)
        .expect("EquiShot n'a pas pu démarrer")
        .run(|_app, evenement| {
            // La fenetre EquiShot fermee ne ferme pas l'application : seul
            // « Quitter » (code pose) l'arrete.
            if let tauri::RunEvent::ExitRequested { api, code: None, .. } = evenement {
                api.prevent_exit();
            }
        });
}
