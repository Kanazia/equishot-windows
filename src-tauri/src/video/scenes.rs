//! Les scenes (ADR-017, 0.19) : des mises en scene nommees, gardees dans la table
//! `scenes`, appliquees depuis le panneau ou choisies dans un menu « Filmer ». Ici :
//! les types, la validation, l'ecran retrouve, les entrees du menu, la base. Rien
//! de Tauri : tout se teste sans fenetre.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::bulle::Taille;
use super::moteur::Reglages;

/// Dit dans le panneau quand l'ecran d'une scene n'est plus la.
pub const REPLI_ECRAN: &str = "L'écran de la scène n'est plus branché : écran principal entier.";

/// L'ecran d'une scene : son nom Windows (`\\.\DISPLAY1`, celui de tao et de
/// `Origine.ecran`), son coin et sa taille en pixels physiques.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ecran {
    pub nom: String,
    pub position: (i32, i32),
    pub taille: (u32, u32),
}

/// Ce que filme une scene.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "genre", rename_all = "camelCase")]
pub enum Cible {
    /// La selection s'ouvre, on trace a chaque fois.
    ATracer,
    Ecran { ecran: Ecran },
    /// Pixels physiques, relatifs a l'ecran, deja de taille paire.
    Zone { ecran: Ecran, zone: (u32, u32, u32, u32) },
}

/// Un ecran branche, tel que Tauri le donne.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EcranVu {
    pub nom: String,
    pub position: (i32, i32),
    pub taille: (u32, u32),
    pub echelle: f64,
    pub principal: bool,
}

/// L'ecran de la scene parmi ceux branches : meme nom et meme taille, sinon meme
/// place et meme taille (comme la derniere zone, ADR-009). D'une autre taille, ce
/// n'est plus lui : on filmerait autre chose sans le dire.
pub fn retrouver<'a>(voulu: &Ecran, ecrans: &'a [EcranVu]) -> Option<&'a EcranVu> {
    ecrans
        .iter()
        .find(|e| e.nom == voulu.nom && e.taille == voulu.taille)
        .or_else(|| ecrans.iter().find(|e| e.position == voulu.position && e.taille == voulu.taille))
}

/// Ce que filme une cible fixe : l'ecran retrouve et la zone, ou l'ecran principal
/// (a defaut le premier) entier et la ligne de repli. `None` sans aucun ecran.
pub fn viser(voulu: &Ecran, zone: Option<(u32, u32, u32, u32)>, ecrans: &[EcranVu]) -> Option<(EcranVu, Option<(u32, u32, u32, u32)>, Option<&'static str>)> {
    if let Some(e) = retrouver(voulu, ecrans) {
        return Some((e.clone(), zone, None));
    }
    // ponytail: la zone ne se reporte pas sur un autre ecran ; l'ecran principal entier, dit.
    ecrans.iter().find(|e| e.principal).or(ecrans.first()).map(|e| (e.clone(), None, Some(REPLI_ECRAN)))
}

/// Les menus « Filmer » : la zone, puis les scenes. Identifiants partages par
/// l'icone et la pastille, lus par le gestionnaire global de l'icone.
pub const PREFIXE_MENU: &str = "scene-";
pub const LIBELLE_ZONE: &str = "Une zone ou une fenêtre…";
pub const LIBELLE_AUCUNE: &str = "Aucune scène enregistrée";

#[derive(Debug, Clone, PartialEq)]
pub enum Entree {
    Zone,
    Separateur,
    /// Grisee : rien a choisir.
    Aucune,
    Scene { id: i64, libelle: String },
}

/// Un « & » double : Windows lit « &x » comme la lettre x soulignee.
pub fn entrees_filmer(scenes: &[(i64, String)]) -> Vec<Entree> {
    let mut entrees = vec![Entree::Zone, Entree::Separateur];
    if scenes.is_empty() {
        entrees.push(Entree::Aucune);
    }
    entrees.extend(scenes.iter().map(|(id, nom)| Entree::Scene { id: *id, libelle: nom.replace('&', "&&") }));
    entrees
}

pub fn id_du_menu(id: &str) -> Option<i64> {
    id.strip_prefix(PREFIXE_MENU)?.parse().ok()
}

pub const NOM_MAX: usize = 40;

/// Ce qu'une scene remet dans les reglages : le son, la bulle, sa place comprise, et
/// les effets (0.22).
/// Le curseur n'en est pas : c'est un gout, pas une scene.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Mise {
    pub micro: bool,
    pub micro_id: Option<String>,
    pub son_systeme: bool,
    pub sortie_id: Option<String>,
    pub volume_micro: f32,
    pub volume_son: f32,
    pub camera: bool,
    pub camera_id: Option<String>,
    pub camera_taille: Taille,
    pub camera_miroir: bool,
    pub camera_place: Option<(f64, f64)>,
    pub onde: bool,
    pub trainee: bool,
    pub curseur_taille: f32,
}

impl Default for Mise {
    fn default() -> Self {
        Mise::de(&Reglages::default())
    }
}

impl Mise {
    pub fn de(r: &Reglages) -> Mise {
        Mise {
            micro: r.micro,
            micro_id: r.micro_id.clone(),
            son_systeme: r.son_systeme,
            sortie_id: r.sortie_id.clone(),
            volume_micro: r.volume_micro,
            volume_son: r.volume_son,
            camera: r.camera,
            camera_id: r.camera_id.clone(),
            camera_taille: r.camera_taille,
            camera_miroir: r.camera_miroir,
            camera_place: r.camera_place,
            onde: r.onde,
            trainee: r.trainee,
            curseur_taille: r.curseur_taille,
        }
    }

    /// Sans `..` : un champ ajoute aux reglages ne compile pas tant qu'on n'a pas
    /// decide s'il fait partie d'une scene.
    pub fn appliquer(&self, r: &Reglages) -> Reglages {
        Reglages {
            curseur: r.curseur,
            micro: self.micro,
            micro_id: self.micro_id.clone(),
            son_systeme: self.son_systeme,
            sortie_id: self.sortie_id.clone(),
            volume_micro: self.volume_micro,
            volume_son: self.volume_son,
            camera: self.camera,
            camera_id: self.camera_id.clone(),
            camera_taille: self.camera_taille,
            camera_miroir: self.camera_miroir,
            camera_place: self.camera_place,
            onde: self.onde,
            trainee: self.trainee,
            curseur_taille: self.curseur_taille,
        }
    }
}

/// Le JSON de la colonne `donnees`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Donnees {
    pub cible: Cible,
    #[serde(default)]
    pub mise: Mise,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scene {
    pub id: i64,
    pub nom: String,
    pub maj: String,
    pub cible: Cible,
    pub mise: Mise,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Illisible {
    pub id: i64,
    pub nom: String,
    pub motif: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Liste {
    pub scenes: Vec<Scene>,
    pub illisibles: Vec<Illisible>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "etat", rename_all = "camelCase")]
pub enum Ecrite {
    Enregistree { id: i64 },
    /// Le nom est pris : le panneau propose de remplacer.
    NomPris,
}

pub fn nom_valide(nom: &str) -> Result<String, String> {
    let nom = nom.trim();
    match nom.chars().count() {
        0 => Err("Donnez un nom à la scène.".to_string()),
        n if n > NOM_MAX => Err(format!("{NOM_MAX} caractères au plus pour le nom d'une scène.")),
        _ => Ok(nom.to_string()),
    }
}

fn valider(d: &Donnees) -> Result<(), String> {
    let m = &d.mise;
    let volume = |v: f32| v.is_finite() && (0.0..=2.0).contains(&v);
    if !volume(m.volume_micro) || !volume(m.volume_son) {
        return Err("volume hors de 0 à 200 %".to_string());
    }
    let part = |p: f64| p.is_finite() && (0.0..=1.0).contains(&p);
    if m.camera_place.is_some_and(|(x, y)| !part(x) || !part(y)) {
        return Err("place de la bulle hors de la zone".to_string());
    }
    let ecran_sans_taille = |e: &Ecran| e.taille.0 == 0 || e.taille.1 == 0;
    match &d.cible {
        Cible::ATracer => Ok(()),
        Cible::Ecran { ecran } if ecran_sans_taille(ecran) => Err("écran sans taille".to_string()),
        Cible::Ecran { .. } => Ok(()),
        Cible::Zone { ecran, zone: (x, y, l, h) } => {
            let dehors = *x as u64 + *l as u64 > ecran.taille.0 as u64 || *y as u64 + *h as u64 > ecran.taille.1 as u64;
            if ecran_sans_taille(ecran) || *l < 16 || *h < 16 || dehors { Err("zone hors de son écran".to_string()) } else { Ok(()) }
        }
    }
}

/// Le JSON d'une scene, valide : jamais un plantage, un motif.
pub fn lire_donnees(brut: &str) -> Result<Donnees, String> {
    // Le message de serde_json est en anglais : il reste au journal, l'interface lit un motif.
    let d: Donnees = serde_json::from_str(brut).map_err(|e| {
        eprintln!("Scène illisible : {e}");
        "JSON illisible".to_string()
    })?;
    valider(&d)?;
    Ok(d)
}

fn erreur_base(e: rusqlite::Error) -> String {
    format!("Base des scènes illisible : {e}")
}

/// Les scenes lisibles par nom, et les illisibles a part (montrees dans les reglages).
pub fn lister(conn: &Connection) -> Result<Liste, String> {
    let mut stmt = conn.prepare("SELECT id, nom, donnees, maj FROM scenes ORDER BY nom COLLATE NOCASE").map_err(erreur_base)?;
    let lignes = stmt
        .query_map([], |l| Ok((l.get::<_, i64>(0)?, l.get::<_, String>(1)?, l.get::<_, String>(2)?, l.get::<_, String>(3)?)))
        .map_err(erreur_base)?;
    let mut liste = Liste::default();
    for ligne in lignes {
        let (id, nom, brut, maj) = ligne.map_err(erreur_base)?;
        match lire_donnees(&brut) {
            Ok(Donnees { cible, mise }) => liste.scenes.push(Scene { id, nom, maj, cible, mise }),
            Err(motif) => liste.illisibles.push(Illisible { id, nom, motif }),
        }
    }
    Ok(liste)
}

pub fn lire(conn: &Connection, id: i64) -> Result<Scene, String> {
    let ligne: Option<(String, String, String)> = conn
        .query_row("SELECT nom, donnees, maj FROM scenes WHERE id = ?1", [id], |l| Ok((l.get(0)?, l.get(1)?, l.get(2)?)))
        .optional()
        .map_err(erreur_base)?;
    let (nom, brut, maj) = ligne.ok_or("Cette scène n'existe plus.")?;
    let Donnees { cible, mise } = lire_donnees(&brut).map_err(|motif| format!("La scène « {nom} » est illisible : {motif}"))?;
    Ok(Scene { id, nom, maj, cible, mise })
}

/// Un nom pris rend `NomPris` sans rien ecrire, tant que `remplacer` n'est pas vrai.
pub fn enregistrer(conn: &Connection, nom: &str, d: &Donnees, remplacer: bool, maj: &str) -> Result<Ecrite, String> {
    let nom = nom_valide(nom)?;
    valider(d)?;
    let brut = serde_json::to_string(d).map_err(|e| e.to_string())?;
    let existante: Option<i64> = conn.query_row("SELECT id FROM scenes WHERE nom = ?1", [&nom], |l| l.get(0)).optional().map_err(erreur_base)?;
    match existante {
        Some(_) if !remplacer => Ok(Ecrite::NomPris),
        Some(id) => {
            conn.execute("UPDATE scenes SET nom = ?1, donnees = ?2, maj = ?3 WHERE id = ?4", params![nom, brut, maj, id]).map_err(erreur_base)?;
            Ok(Ecrite::Enregistree { id })
        }
        None => {
            conn.execute("INSERT INTO scenes (nom, donnees, maj) VALUES (?1, ?2, ?3)", params![nom, brut, maj]).map_err(erreur_base)?;
            Ok(Ecrite::Enregistree { id: conn.last_insert_rowid() })
        }
    }
}

pub fn renommer(conn: &Connection, id: i64, nom: &str, maj: &str) -> Result<(), String> {
    let nom = nom_valide(nom)?;
    let autre: Option<i64> = conn.query_row("SELECT id FROM scenes WHERE nom = ?1 AND id <> ?2", params![nom, id], |l| l.get(0)).optional().map_err(erreur_base)?;
    if autre.is_some() {
        return Err(format!("Une autre scène s'appelle déjà « {nom} »."));
    }
    match conn.execute("UPDATE scenes SET nom = ?1, maj = ?2 WHERE id = ?3", params![nom, maj, id]).map_err(erreur_base)? {
        0 => Err("Cette scène n'existe plus.".to_string()),
        _ => Ok(()),
    }
}

pub fn supprimer(conn: &Connection, id: i64) -> Result<(), String> {
    conn.execute("DELETE FROM scenes WHERE id = ?1", [id]).map(|_| ()).map_err(erreur_base)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vu(nom: &str, position: (i32, i32), taille: (u32, u32), principal: bool) -> EcranVu {
        EcranVu { nom: nom.into(), position, taille, echelle: 1.0, principal }
    }

    fn display2() -> Ecran {
        Ecran { nom: r"\\.\DISPLAY2".into(), position: (1920, 0), taille: (2560, 1440) }
    }

    fn donnees(cible: Cible) -> Donnees {
        Donnees { cible, mise: Mise::de(&Reglages { camera: true, volume_son: 0.5, camera_place: Some((1.0, 1.0)), ..Reglages::default() }) }
    }

    fn base() -> Connection {
        crate::db::open_in_memory().expect("base")
    }

    #[test]
    fn un_nom_se_valide() {
        assert_eq!(nom_valide("  Démo  "), Ok("Démo".to_string()));
        assert!(nom_valide("   ").is_err());
        assert!(nom_valide(&"é".repeat(40)).is_ok(), "40 caractères, pas 40 octets");
        assert!(nom_valide(&"é".repeat(41)).is_err());
    }

    #[test]
    fn le_json_d_une_scene_se_relit_tel_qu_ecrit() {
        for cible in [Cible::ATracer, Cible::Ecran { ecran: display2() }, Cible::Zone { ecran: display2(), zone: (100, 60, 640, 480) }] {
            let d = donnees(cible);
            assert_eq!(lire_donnees(&serde_json::to_string(&d).expect("écrites")), Ok(d));
        }
    }

    #[test]
    fn un_json_invalide_est_refuse_sans_panique() {
        assert_eq!(lire_donnees(""), Err("JSON illisible".to_string()), "le motif lu par l'interface, en français");
        assert!(lire_donnees("{}").is_err(), "sans cible");
        assert!(lire_donnees(r#"{"cible":{"genre":"zone"}}"#).is_err());
        let mut d = donnees(Cible::Zone { ecran: display2(), zone: (2400, 0, 640, 480) });
        assert!(lire_donnees(&serde_json::to_string(&d).expect("écrites")).is_err(), "zone hors de son écran");
        d.cible = Cible::ATracer;
        d.mise.volume_son = 7.0;
        assert!(lire_donnees(&serde_json::to_string(&d).expect("écrites")).is_err(), "volume hors de 0 à 200 %");
        d.mise.volume_son = 1.0;
        d.mise.camera_place = Some((1.5, 0.0));
        assert!(lire_donnees(&serde_json::to_string(&d).expect("écrites")).is_err(), "place hors de l'espace libre");
    }

    #[test]
    fn la_mise_garde_le_curseur_de_l_utilisateur() {
        let scene = Mise::de(&Reglages { curseur: false, micro: false, volume_micro: 0.3, camera: true, camera_taille: Taille::Petite, camera_place: Some((1.0, 1.0)), ..Reglages::default() });
        let r = scene.appliquer(&Reglages { curseur: true, ..Reglages::default() });
        assert_eq!((r.curseur, r.micro, r.volume_micro, r.camera, r.camera_taille, r.camera_place), (true, false, 0.3, true, Taille::Petite, Some((1.0, 1.0))));
    }

    #[test]
    fn enregistrer_remplacer_renommer_supprimer() {
        let conn = base();
        let d = donnees(Cible::Zone { ecran: display2(), zone: (100, 60, 640, 480) });
        let Ok(Ecrite::Enregistree { id }) = enregistrer(&conn, "Démo", &d, false, "t1") else { panic!("enregistrée") };
        let mut autre = d.clone();
        autre.mise.volume_son = 0.75;
        assert_eq!(enregistrer(&conn, "démo", &autre, false, "t2"), Ok(Ecrite::NomPris), "casse ASCII comprise");
        assert_eq!(lire(&conn, id).map(|s| s.mise.volume_son), Ok(0.5), "rien d'écrasé sans « remplacer »");
        assert_eq!(enregistrer(&conn, "démo", &autre, true, "t3"), Ok(Ecrite::Enregistree { id }));
        assert_eq!(lire(&conn, id).map(|s| (s.nom, s.mise.volume_son, s.maj)), Ok(("démo".to_string(), 0.75, "t3".to_string())));
        assert!(matches!(enregistrer(&conn, "Écran", &donnees(Cible::Ecran { ecran: display2() }), false, "t4"), Ok(Ecrite::Enregistree { .. })));
        assert!(renommer(&conn, id, "Écran", "t5").is_err(), "un nom pris par une autre scène est refusé");
        assert_eq!(renommer(&conn, id, "DÉMO", "t5"), Ok(()), "sa propre casse se change");
        assert_eq!(renommer(&conn, id, "Démo zone", "t5"), Ok(()));
        assert_eq!(lister(&conn).map(|l| l.scenes.into_iter().map(|s| s.nom).collect::<Vec<_>>()), Ok(vec!["Démo zone".to_string(), "Écran".to_string()]));
        assert_eq!(supprimer(&conn, id), Ok(()));
        assert!(lire(&conn, id).is_err());
        assert!(renommer(&conn, id, "x", "t6").is_err(), "une scène supprimée ne se renomme pas");
        assert!(enregistrer(&conn, " ", &d, false, "t7").is_err(), "sans nom");
    }

    #[test]
    fn une_scene_illisible_est_ignoree_et_signalee() {
        let conn = base();
        assert!(enregistrer(&conn, "Bonne", &donnees(Cible::ATracer), false, "t").is_ok());
        conn.execute("INSERT INTO scenes (nom, donnees, maj) VALUES ('Abîmée', '{\"cible\":', 't')", []).expect("ligne abîmée");
        let l = lister(&conn).expect("liste");
        assert_eq!(l.scenes.iter().map(|s| s.nom.as_str()).collect::<Vec<_>>(), vec!["Bonne"]);
        assert_eq!(l.illisibles.iter().map(|s| s.nom.as_str()).collect::<Vec<_>>(), vec!["Abîmée"]);
        assert!(lire(&conn, l.illisibles[0].id).is_err(), "elle ne s'applique pas");
        assert_eq!(supprimer(&conn, l.illisibles[0].id), Ok(()), "elle se supprime");
    }

    #[test]
    fn l_ecran_se_retrouve_par_son_nom_et_sa_taille_puis_par_sa_place() {
        let deplace = [vu(r"\\.\DISPLAY1", (0, 0), (1920, 1080), true), vu(r"\\.\DISPLAY2", (-2560, 0), (2560, 1440), false)];
        assert_eq!(retrouver(&display2(), &deplace).map(|e| e.position), Some((-2560, 0)), "déplacé, retrouvé par son nom");
        let renumerote = [vu(r"\\.\DISPLAY1", (0, 0), (1920, 1080), true), vu(r"\\.\DISPLAY3", (1920, 0), (2560, 1440), false)];
        assert_eq!(retrouver(&display2(), &renumerote).map(|e| e.nom.as_str()), Some(r"\\.\DISPLAY3"), "renuméroté, retrouvé par sa place");
        let autre_taille = [vu(r"\\.\DISPLAY2", (1920, 0), (1920, 1080), false)];
        assert_eq!(retrouver(&display2(), &autre_taille), None, "d'une autre résolution, ce n'est plus lui");
    }

    #[test]
    fn un_ecran_disparu_retombe_sur_le_principal_entier() {
        let zone = Some((100, 60, 640, 480));
        let present = [vu(r"\\.\DISPLAY2", (1920, 0), (2560, 1440), false)];
        assert_eq!(viser(&display2(), zone, &present).map(|(e, z, r)| (e.nom, z, r)), Some((r"\\.\DISPLAY2".to_string(), zone, None)));
        let sans_lui = [vu(r"\\.\DISPLAY1", (0, 0), (1920, 1080), false), vu(r"\\.\DISPLAY4", (0, 1080), (1920, 1080), true)];
        assert_eq!(viser(&display2(), zone, &sans_lui).map(|(e, z, r)| (e.nom, z, r)), Some((r"\\.\DISPLAY4".to_string(), None, Some(REPLI_ECRAN))), "la zone tombe, l'écran principal entier");
        let sans_principal = [vu(r"\\.\DISPLAY1", (0, 0), (1920, 1080), false)];
        assert_eq!(viser(&display2(), None, &sans_principal).map(|(e, _, _)| e.nom), Some(r"\\.\DISPLAY1".to_string()), "faute de principal, le premier");
        assert_eq!(viser(&display2(), None, &[]), None, "aucun écran");
    }

    #[test]
    fn la_cible_se_relit_telle_qu_ecrite() {
        for cible in [Cible::ATracer, Cible::Ecran { ecran: display2() }, Cible::Zone { ecran: display2(), zone: (100, 60, 640, 480) }] {
            let brut = serde_json::to_string(&cible).expect("écrite");
            assert_eq!(serde_json::from_str::<Cible>(&brut).expect("relue"), cible);
        }
        assert_eq!(serde_json::to_string(&Cible::ATracer).expect("écrite"), r#"{"genre":"aTracer"}"#);
    }

    #[test]
    fn le_menu_filmer_met_la_zone_puis_les_scenes() {
        assert_eq!(entrees_filmer(&[]), vec![Entree::Zone, Entree::Separateur, Entree::Aucune]);
        assert_eq!(
            entrees_filmer(&[(4, "R&D".into()), (2, "Démo".into())]),
            vec![Entree::Zone, Entree::Separateur, Entree::Scene { id: 4, libelle: "R&&D".into() }, Entree::Scene { id: 2, libelle: "Démo".into() }],
            "un « & » seul serait une lettre soulignée, effacée du menu"
        );
    }

    #[test]
    fn l_identifiant_d_une_scene_se_lit_dans_le_menu() {
        assert_eq!(id_du_menu("scene-12"), Some(12));
        assert_eq!(id_du_menu("scene-aucune"), None);
        assert_eq!(id_du_menu("differe-3"), None);
        assert_eq!(id_du_menu("video"), None);
    }
}
