//! Mise a jour (ADR-020) : verifiee 30 s apres le lancement puis toutes les
//! 24 h, et sur « Rechercher » ; proposee dans le menu de l'icone et les
//! Reglages ; installee sur un clic. Le greffon telecharge, verifie la signature,
//! lance l'installeur NSIS en `passive` puis quitte EquiShot, que l'installeur
//! relance. L'instance dev ne verifie jamais.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::{captures, pastille, video, AppState};

/// Emis vers toutes les fenetres a chaque changement de l'etat.
pub const EVENEMENT_ETAT: &str = "maj://etat";
const PREMIERE: Duration = Duration::from_secs(30);
const INTERVALLE: Duration = Duration::from_secs(24 * 60 * 60);
/// Le greffon n'a aucun delai par defaut : un serveur muet figerait la boucle.
const DELAI_VERIFICATION: Duration = Duration::from_secs(30);
const DELAI_TELECHARGEMENT: Duration = Duration::from_secs(10 * 60);
/// Une installation a la fois : le menu de l'icone et les Reglages ne lancent
/// pas deux installeurs.
static INSTALLATION: AtomicBool = AtomicBool::new(false);
const EN_DEV: &str = "La mise à jour ne se vérifie pas dans l'instance de développement.";

/// Ce que la rangee « Mises a jour » des Reglages montre.
#[derive(Clone, Default, Serialize)]
pub struct EtatMaj {
    /// La version proposee, s'il y en a une.
    pub disponible: Option<String>,
    /// L'heure de la derniere verification aboutie, `HH:MM`.
    pub verifiee: Option<String>,
    /// La derniere verification a echoue : pourquoi.
    pub erreur: Option<String>,
    pub dev: bool,
}

#[derive(Default)]
pub struct EtatMajs {
    etat: Mutex<EtatMaj>,
    /// La mise a jour trouvee, installee telle quelle sur le clic.
    trouvee: Mutex<Option<Update>>,
}

/// Au demarrage : la boucle de verification, sauf en dev.
pub fn demarrer(app: &tauri::AppHandle) {
    if app.state::<AppState>().dev {
        if let Ok(mut etat) = app.state::<EtatMajs>().etat.lock() {
            etat.dev = true;
        }
        return;
    }
    let app = app.clone();
    // ponytail: un fil qui dort, pas de minuterie asynchrone (tokio n'est pas une dependance directe).
    std::thread::spawn(move || {
        std::thread::sleep(PREMIERE);
        loop {
            // Hors ligne ou serveur absent : rien a l'ecran, l'etat garde l'erreur.
            let _ = tauri::async_runtime::block_on(verifier(&app));
            std::thread::sleep(INTERVALLE);
        }
    });
}

/// Une verification : l'etat, le menu de l'icone et les pages suivent.
async fn verifier(app: &tauri::AppHandle) -> EtatMaj {
    let resultat = match app.updater_builder().timeout(DELAI_VERIFICATION).build() {
        Ok(updater) => updater.check().await.map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    let majs = app.state::<EtatMajs>();
    let etat = {
        let mut etat = majs.etat.lock().unwrap_or_else(|e| e.into_inner());
        match resultat {
            Ok(trouvee) => {
                etat.disponible = trouvee.as_ref().map(|u| u.version.clone());
                etat.verifiee = Some(chrono::Local::now().format("%H:%M").to_string());
                etat.erreur = None;
                *majs.trouvee.lock().unwrap_or_else(|e| e.into_inner()) = trouvee;
            }
            Err(motif) => etat.erreur = Some(format!("Vérification impossible : {motif}")),
        }
        etat.clone()
    };
    pastille::proposer_maj(app, etat.disponible.as_deref());
    let _ = app.emit(EVENEMENT_ETAT, etat.clone());
    etat
}

/// Installe la mise a jour trouvee. Ne rend pas la main si tout va bien : le
/// greffon quitte EquiShot une fois l'installeur lance.
async fn installer(app: &tauri::AppHandle) -> Result<(), String> {
    if app.state::<AppState>().dev {
        return Err(EN_DEV.to_string());
    }
    if video::en_cours(app) {
        return Err("Finissez l'enregistrement avant la mise à jour.".to_string());
    }
    let trouvee = app.state::<EtatMajs>().trouvee.lock().map_err(|_| "État de la mise à jour incohérent.".to_string())?.clone();
    let Some(mut trouvee) = trouvee else {
        return Err("Aucune mise à jour à installer : EquiShot est à jour.".to_string());
    };
    if INSTALLATION.swap(true, Ordering::SeqCst) {
        return Err("La mise à jour est déjà en cours d'installation.".to_string());
    }
    // `check` rend la mise a jour sans delai : il se pose ici.
    trouvee.timeout = Some(DELAI_TELECHARGEMENT);
    let echec = |e: tauri_plugin_updater::Error| format!("Mise à jour {} non installée : {e}", trouvee.version);
    let resultat = match trouvee.download(|_, _| {}, || {}).await {
        // Le verrou tient jusqu'a la sortie du processus : aucun deplacement du
        // dossier ni aucune ecriture n'est coupe par l'installeur.
        Ok(octets) => captures::sans_ecriture(app).and_then(|_exclusif| trouvee.install(octets).map_err(echec)),
        Err(e) => Err(echec(e)),
    };
    INSTALLATION.store(false, Ordering::SeqCst);
    resultat
}

/// Le clic du menu de l'icone : pas de page pour attendre, l'echec va a la pastille.
pub fn installer_detache(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(motif) = installer(&app).await {
            captures::avertir(&app, &motif);
        }
    });
}

#[tauri::command]
pub async fn maj_etat(app: tauri::AppHandle) -> Result<EtatMaj, String> {
    app.state::<EtatMajs>().etat.lock().map(|e| e.clone()).map_err(|_| "État de la mise à jour incohérent.".to_string())
}

#[tauri::command]
pub async fn maj_rechercher(app: tauri::AppHandle) -> Result<EtatMaj, String> {
    if app.state::<AppState>().dev {
        return Err(EN_DEV.to_string());
    }
    Ok(verifier(&app).await)
}

#[tauri::command]
pub async fn maj_installer(app: tauri::AppHandle) -> Result<(), String> {
    installer(&app).await
}
