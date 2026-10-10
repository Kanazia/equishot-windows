//! Lancement avec Windows (ADR-020) : la valeur `<nom du produit>` de la cle
//! `Run` de l'utilisateur, celle que le desinstalleur NSIS de Tauri retire.
//! Ecrite en Rust, sans greffon. Le reglage est la verite ; la cle suit.
//! L'instance dev ne touche jamais au registre.

use std::path::Path;

use tauri::Manager;

use crate::{db, hors_thread_principal, AppState};

/// Reglage : `1` lance avec Windows, `0` non, vide = jamais regle.
pub const CLE_DEMARRAGE: &str = "demarrage_windows";
const SOUS_CLE: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// La commande de la cle `Run` : le chemin entre guillemets (il peut avoir des espaces).
fn ligne_run(exe: &Path) -> String {
    format!("\"{}\"", exe.display())
}

/// Vide (premiere installation) vaut coche : la case est cochee d'office.
fn actif(regle: &str) -> bool {
    regle != "0"
}

fn large(texte: &str) -> Vec<u16> {
    texte.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Pose ou retire la valeur de la cle `Run`. Bloquant (registre) : hors du thread principal.
/// `nom` : le nom du produit (`productName`), celui que le desinstalleur efface.
fn appliquer(nom: &str, actif: bool) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    let (sous_cle, valeur) = (large(SOUS_CLE), large(nom));
    let code = if actif {
        let exe = std::env::current_exe().map_err(|e| format!("Chemin d'EquiShot introuvable : {e}"))?;
        let ligne = large(&ligne_run(&exe));
        // SAFETY: chaines UTF-16 terminees par zero, vivantes pendant l'appel ;
        // `cbdata` compte le zero final en octets, comme REG_SZ l'exige.
        unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, sous_cle.as_ptr(), valeur.as_ptr(), REG_SZ, ligne.as_ptr().cast(), (ligne.len() * 2) as u32) }
    } else {
        // SAFETY: chaines UTF-16 terminees par zero, vivantes pendant l'appel.
        match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, sous_cle.as_ptr(), valeur.as_ptr()) } {
            ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
            code => code,
        }
    };
    match code {
        ERROR_SUCCESS => Ok(()),
        code => Err(format!("Lancement avec Windows non réglé (erreur {code} du registre).")),
    }
}

/// Au demarrage, hors du thread principal : un reglage vide devient `1` (la case
/// cochee d'office), et la cle est reecrite si la case est cochee, pour suivre une
/// mise a jour ou un deplacement de l'exe. Rien en dev.
pub fn initialiser(app: &tauri::AppHandle) {
    let etat = app.state::<AppState>();
    if etat.dev {
        return;
    }
    let regle = etat.with_conn(|conn| db::reglage(conn, CLE_DEMARRAGE)).unwrap_or_else(|_| "0".to_string());
    if regle.is_empty() {
        if let Err(motif) = etat.with_conn(|conn| db::regler(conn, CLE_DEMARRAGE, "1")) {
            eprintln!("lancement avec Windows non enregistré : {motif}");
        }
    }
    if actif(&regle) {
        if let Err(motif) = appliquer(&app.package_info().name, true) {
            eprintln!("{motif}");
        }
    }
}

#[tauri::command]
pub async fn demarrage_lire(app: tauri::AppHandle) -> Result<bool, String> {
    hors_thread_principal(app, |app| app.state::<AppState>().with_conn(|conn| db::reglage(conn, CLE_DEMARRAGE)).map(|r| actif(&r))).await
}

/// La case : la cle d'abord (un refus du registre ne laisse pas un reglage
/// mensonger), puis le reglage. En dev, le reglage seul.
#[tauri::command]
pub async fn demarrage_regler(app: tauri::AppHandle, actif: bool) -> Result<bool, String> {
    hors_thread_principal(app, move |app| {
        let etat = app.state::<AppState>();
        if !etat.dev {
            appliquer(&app.package_info().name, actif)?;
        }
        etat.with_conn(|conn| db::regler(conn, CLE_DEMARRAGE, if actif { "1" } else { "0" }))?;
        Ok(actif)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_ligne_run_met_le_chemin_entre_guillemets() {
        assert_eq!(ligne_run(Path::new(r"C:\Users\Jean Dupont\AppData\Local\EquiShot\EquiShot.exe")), r#""C:\Users\Jean Dupont\AppData\Local\EquiShot\EquiShot.exe""#);
    }

    #[test]
    fn la_case_est_cochee_d_office_puis_suit_le_reglage() {
        assert!(actif(""), "jamais réglé : coché d'office");
        assert!(actif("1"));
        assert!(!actif("0"));
    }
}
