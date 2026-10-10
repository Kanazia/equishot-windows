//! Deplacer le dossier des captures (ADR-020) : copier tout, basculer, puis
//! effacer dans l'ancien dossier ce qui a ete copie, et rien d'autre. Un echec
//! avant la bascule laisse l'ancien dossier intact et en service.

use std::path::{Path, PathBuf};

use super::stockage::CORBEILLE;

/// Les sous-dossiers d'EquiShot : `AAAA-MM` (`stockage::nom_fichier`) et la
/// corbeille. Rien d'autre ne part : l'ancien dossier peut etre partage.
fn est_a_equishot(nom: &str) -> bool {
    let b = nom.as_bytes();
    nom == CORBEILLE || (b.len() == 7 && b[4] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || c.is_ascii_digit()))
}

/// Les fichiers a deplacer, relatifs a `ancien`, tries. Un ancien dossier absent
/// n'a rien a deplacer.
pub fn a_deplacer(ancien: &Path) -> Result<Vec<PathBuf>, String> {
    let mut fichiers = Vec::new();
    let entrees = match std::fs::read_dir(ancien) {
        Ok(entrees) => entrees,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(fichiers),
        Err(e) => return Err(format!("Lecture de {} impossible : {e}", ancien.display())),
    };
    for entree in entrees {
        let entree = entree.map_err(|e| format!("Lecture de {} impossible : {e}", ancien.display()))?;
        let dossier = entree.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if dossier && entree.file_name().to_str().is_some_and(est_a_equishot) {
            parcourir(ancien, &entree.path(), &mut fichiers)?;
        }
    }
    fichiers.sort();
    Ok(fichiers)
}

fn parcourir(racine: &Path, dossier: &Path, fichiers: &mut Vec<PathBuf>) -> Result<(), String> {
    let lire = |e: std::io::Error| format!("Lecture de {} impossible : {e}", dossier.display());
    for entree in std::fs::read_dir(dossier).map_err(lire)? {
        let chemin = entree.map_err(lire)?.path();
        if chemin.is_dir() {
            parcourir(racine, &chemin, fichiers)?;
        } else if let Ok(relatif) = chemin.strip_prefix(racine) {
            fichiers.push(relatif.to_path_buf());
        }
    }
    Ok(())
}

/// Le chemin canonique (casse du disque, `\\?\`), meme s'il n'existe pas encore :
/// le plus proche parent existant est canonise, le reste lui est rajoute.
fn norme(p: &Path) -> PathBuf {
    match (std::fs::canonicalize(p), p.parent(), p.file_name()) {
        (Ok(c), _, _) => c,
        (Err(_), Some(parent), Some(nom)) => norme(parent).join(nom),
        _ => p.to_path_buf(),
    }
}

/// Pourquoi le deplacement est refuse, avant de toucher quoi que ce soit.
pub fn refus(ancien: &Path, cible: &Path, fichiers: &[PathBuf]) -> Option<String> {
    let (a, c) = (norme(ancien), norme(cible));
    if a == c {
        return Some("C'est déjà le dossier des captures.".to_string());
    }
    if c.starts_with(&a) || a.starts_with(&c) {
        return Some(format!(
            "{} et {} sont l'un dans l'autre : choisissez un dossier à part.",
            ancien.display(),
            cible.display()
        ));
    }
    fichiers
        .iter()
        .map(|f| cible.join(f))
        .find(|f| f.exists())
        .map(|f| format!("{} existe déjà : rien n'a été déplacé.", f.display()))
}

/// Copie `fichiers` de `ancien` vers `cible` en verifiant chaque taille, appelle
/// `basculer`, puis efface dans `ancien` les seuls fichiers copies. Rend le
/// nombre de fichiers deplaces. Un echec avant la fin de `basculer` retire les
/// copies de la cible et laisse `ancien` tel quel.
pub fn deplacer(ancien: &Path, cible: &Path, fichiers: &[PathBuf], basculer: impl FnOnce() -> Result<(), String>) -> Result<usize, String> {
    let mut copies = Vec::new();
    let mut copier = || -> Result<(), String> {
        for f in fichiers {
            let (de, vers) = (ancien.join(f), cible.join(f));
            if vers.exists() {
                return Err(format!("{} existe déjà : rien n'a été déplacé.", vers.display()));
            }
            if let Some(parent) = vers.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("Création de {} impossible : {e}", parent.display()))?;
            }
            // Inscrite avant la copie : une copie qui echoue en route laisse un
            // fichier partiel, que le nettoyage doit retirer aussi.
            // ponytail: un arret du processus en pleine copie le laisse, et la reprise est refusee (« existe deja ») ; l'effacer a la main.
            copies.push(f.clone());
            let ecrits = std::fs::copy(&de, &vers).map_err(|e| format!("Copie de {} impossible : {e}", de.display()))?;
            let attendue = std::fs::metadata(&de).map(|m| m.len()).map_err(|e| e.to_string())?;
            let posee = std::fs::metadata(&vers).map(|m| m.len()).map_err(|e| e.to_string())?;
            if ecrits != attendue || posee != attendue {
                return Err(format!("Copie incomplète de {} : rien n'a été déplacé.", de.display()));
            }
        }
        Ok(())
    };
    if let Err(motif) = copier().and_then(|()| basculer()) {
        effacer(cible, &copies);
        return Err(motif);
    }
    let restes = effacer(ancien, fichiers);
    if restes > 0 {
        eprintln!("deplacement : {restes} fichier(s) laisse(s) dans {}", ancien.display());
    }
    let _ = std::fs::remove_dir(ancien);
    Ok(fichiers.len())
}

/// Efface `fichiers` sous `racine`, puis leurs dossiers devenus vides (jamais
/// `racine` elle-meme). Rend le nombre de fichiers qui n'ont pas pu partir.
fn effacer(racine: &Path, fichiers: &[PathBuf]) -> usize {
    let restes = fichiers.iter().filter(|f| std::fs::remove_file(racine.join(f)).is_err()).count();
    for f in fichiers {
        // `remove_dir` refuse un dossier non vide : on remonte tant qu'il accepte.
        let mut parent = f.parent();
        while let Some(p) = parent.filter(|p| !p.as_os_str().is_empty()) {
            if std::fs::remove_dir(racine.join(p)).is_err() {
                break;
            }
            parent = p.parent();
        }
    }
    restes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un dossier temporaire propre au test, retire a la fin.
    struct Temp(PathBuf);
    impl Temp {
        fn neuf(nom: &str) -> Temp {
            let p = std::env::temp_dir().join(format!("equishot-deplacer-{nom}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).expect("temp");
            Temp(p)
        }
        fn ecrire(&self, relatif: &str, contenu: &str) {
            let p = self.0.join(relatif);
            std::fs::create_dir_all(p.parent().expect("parent")).expect("dossier");
            std::fs::write(p, contenu).expect("écrit");
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn rel(liste: &[&str]) -> Vec<PathBuf> {
        liste.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn seuls_les_dossiers_d_equishot_partent() {
        let a = Temp::neuf("filtre");
        a.ecrire("2026-10/a.png", "a");
        a.ecrire("2026-10/a.rendu.png", "ar");
        a.ecrire("corbeille/b.png", "b");
        a.ecrire("vacances/photo.jpg", "x");
        a.ecrire("2026-1/faux.png", "x");
        a.ecrire("2026-1x/faux.png", "x");
        a.ecrire("racine.png", "x");
        let attendus = rel(&["2026-10/a.png", "2026-10/a.rendu.png", "corbeille/b.png"]);
        assert_eq!(a_deplacer(&a.0).expect("liste"), attendus);
        assert_eq!(a_deplacer(&a.0.join("absent")).expect("absent"), Vec::<PathBuf>::new());
    }

    #[test]
    fn refus_avant_de_toucher_quoi_que_ce_soit() {
        let a = Temp::neuf("refus-a");
        let c = Temp::neuf("refus-c");
        a.ecrire("2026-10/a.png", "a");
        let fichiers = rel(&["2026-10/a.png"]);
        assert!(refus(&a.0, &a.0, &fichiers).expect("même").contains("déjà le dossier"));
        assert!(refus(&a.0, &a.0.join("sous"), &fichiers).expect("dedans").contains("l'un dans l'autre"));
        assert!(refus(&a.0.join("2026-10"), &a.0, &fichiers).expect("autour").contains("l'un dans l'autre"));
        assert_eq!(refus(&a.0, &c.0, &fichiers), None);
        c.ecrire("2026-10/a.png", "autre");
        assert!(refus(&a.0, &c.0, &fichiers).expect("collision").contains("existe déjà"));
    }

    #[test]
    fn deplace_copie_bascule_puis_vide_l_ancien() {
        let racine = Temp::neuf("ok");
        let (ancien, cible) = (racine.0.join("ancien"), racine.0.join("cible"));
        let a = Temp(ancien.clone());
        a.ecrire("2026-10/a.png", "aaaa");
        a.ecrire("2026-09/b.png", "bb");
        a.ecrire("corbeille/c.png", "c");
        std::fs::create_dir_all(&cible).expect("cible");
        let fichiers = a_deplacer(&ancien).expect("liste");
        let mut bascule = false;
        assert_eq!(deplacer(&ancien, &cible, &fichiers, || { bascule = true; Ok(()) }).expect("déplacé"), 3);
        assert!(bascule);
        assert_eq!(std::fs::read_to_string(cible.join("2026-10/a.png")).expect("a"), "aaaa");
        assert_eq!(std::fs::read_to_string(cible.join("corbeille/c.png")).expect("c"), "c");
        assert!(!ancien.exists(), "l'ancien dossier vidé est retiré");
        std::mem::forget(a);
    }

    #[test]
    fn un_dossier_partage_garde_ce_qui_n_est_pas_a_equishot() {
        let racine = Temp::neuf("partage");
        let (ancien, cible) = (racine.0.join("Images"), racine.0.join("cible"));
        let a = Temp(ancien.clone());
        a.ecrire("2026-10/a.png", "a");
        a.ecrire("vacances/photo.jpg", "x");
        std::fs::create_dir_all(&cible).expect("cible");
        let fichiers = a_deplacer(&ancien).expect("liste");
        deplacer(&ancien, &cible, &fichiers, || Ok(())).expect("déplacé");
        assert!(ancien.join("vacances/photo.jpg").exists(), "le reste de l'ancien dossier ne bouge pas");
        assert!(!ancien.join("2026-10").exists());
        std::mem::forget(a);
    }

    #[test]
    fn un_echec_laisse_l_ancien_intact_et_nettoie_la_cible() {
        let racine = Temp::neuf("echec");
        let (ancien, cible) = (racine.0.join("ancien"), racine.0.join("cible"));
        let a = Temp(ancien.clone());
        a.ecrire("2026-10/a.png", "a");
        a.ecrire("2026-10/b.png", "b");
        std::fs::create_dir_all(&cible).expect("cible");
        let fichiers = a_deplacer(&ancien).expect("liste");
        let motif = deplacer(&ancien, &cible, &fichiers, || Err("base fermée".to_string())).expect_err("refusé");
        assert_eq!(motif, "base fermée");
        assert_eq!(a_deplacer(&ancien).expect("ancien"), fichiers, "l'ancien est intact");
        assert_eq!(std::fs::read_dir(&cible).expect("cible").count(), 0, "la cible est nettoyée");
        // Un fichier source manquant (copie en echec au milieu) : meme issue.
        let manquant = rel(&["2026-10/a.png", "2026-10/disparu.png", "2026-10/b.png"]);
        assert!(deplacer(&ancien, &cible, &manquant, || Ok(())).is_err());
        assert_eq!(a_deplacer(&ancien).expect("ancien"), fichiers);
        assert_eq!(std::fs::read_dir(&cible).expect("cible").count(), 0);
        std::mem::forget(a);
    }
}
