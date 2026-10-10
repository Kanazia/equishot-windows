//! Base SQLite d'EquiShot : les captures, leur index de recherche, les reglages.

use std::path::Path;

use rusqlite::Connection;

/// Variable d'environnement de l'instance de developpement.
pub const VAR_INSTANCE: &str = "EQUISHOT_INSTANCE";
/// Identifiant de l'instance de developpement : dossier de donnees, profil
/// WebView2 et verrou d'instance unique en derivent, a part de la production.
pub const IDENTIFIANT_DEV: &str = "fr.equinox.equishot-dev";
/// Port du debogueur de la WebView sur l'instance de developpement.
pub const PORT_DEV: u16 = 9334;

/// `Ok(true)` si `EQUISHOT_INSTANCE=dev`. Toute autre valeur posee est une
/// erreur : une faute de frappe ne doit jamais retomber sur la production.
pub fn instance_dev() -> Result<bool, String> {
    lire_instance(std::env::var(VAR_INSTANCE).ok().as_deref())
}

fn lire_instance(valeur: Option<&str>) -> Result<bool, String> {
    match valeur {
        None | Some("") => Ok(false),
        Some("dev") => Ok(true),
        Some(autre) => Err(format!("{VAR_INSTANCE} vaut « {autre} » : seule la valeur « dev » est reconnue, EquiShot ne démarre pas.")),
    }
}

/// `false` seulement sur l'instance dev avec `EQUISHOT_DEMO=1` : la pastille, les
/// vignettes et la barre vidéo deviennent visibles en capture, pour les visuels du
/// site. Jamais la sélection : l'écran est pris pendant qu'elle est affichée, et
/// c'est sa protection qui la garde hors de l'image.
pub fn fenetres_protegees() -> bool {
    lire_protection(instance_dev().unwrap_or(false), std::env::var("EQUISHOT_DEMO").ok().as_deref())
}

fn lire_protection(dev: bool, demo: Option<&str>) -> bool {
    !(dev && demo == Some("1"))
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS reglages (
    cle    TEXT PRIMARY KEY,
    valeur TEXT NOT NULL
);

-- `fichier` est le chemin de
-- l'image d'origine, relatif au dossier des captures : le rendu et la vignette
-- s'en deduisent. Les formes restent des donnees, l'original n'est jamais modifie.
CREATE TABLE IF NOT EXISTS captures (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    cree_le     TEXT NOT NULL,
    modifie_le  TEXT NOT NULL,
    fichier     TEXT NOT NULL,
    largeur     INTEGER NOT NULL,
    hauteur     INTEGER NOT NULL,
    ecran       TEXT NOT NULL DEFAULT '',
    echelle     REAL NOT NULL DEFAULT 1,
    fenetre     TEXT NOT NULL DEFAULT '',
    application TEXT NOT NULL DEFAULT '',
    note        TEXT NOT NULL DEFAULT '',
    formes      TEXT NOT NULL DEFAULT '[]'
);

-- ADR-017 (0.19) : les scenes, des mises en scene nommees. `donnees` : le JSON de
-- `video::scenes::Donnees`, valide a la lecture. Nom unique, casse ASCII ignoree.
CREATE TABLE IF NOT EXISTS scenes (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    nom     TEXT NOT NULL UNIQUE COLLATE NOCASE,
    donnees TEXT NOT NULL,
    maj     TEXT NOT NULL
);

"#;

// ponytail: table FTS5 autonome alimentee a la main, rowid = captures.id.
// `jour` porte la date pour la chercher aussi, `texte` le texte reconnu (ADR-010).
const INDEX: &str = "CREATE VIRTUAL TABLE captures_fts USING fts5(
    note,
    fenetre,
    application,
    jour,
    texte,
    tokenize = 'unicode61 remove_diacritics 2'
)";

fn a_colonne(conn: &Connection, table: &str, colonne: &str) -> Result<bool, String> {
    conn.query_row(&format!("SELECT COUNT(*) > 0 FROM pragma_table_info('{table}') WHERE name = ?1"), [colonne], |l| l.get(0))
        .map_err(|e| format!("Lecture du schéma impossible : {e}"))
}

/// FTS5 ne sait pas ajouter une colonne : un index sans `texte` (ou absent) est
/// refait et rempli depuis `captures`, d'un bloc.
fn refaire_index(conn: &Connection) -> Result<(), String> {
    let erreur = |e: rusqlite::Error| format!("Index de recherche impossible à refaire : {e}");
    let tx = conn.unchecked_transaction().map_err(erreur)?;
    tx.execute_batch(&format!("DROP TABLE IF EXISTS captures_fts; {INDEX};")).map_err(erreur)?;
    let lignes: Vec<(i64, String, String, String, String, Option<String>)> = {
        let mut stmt = tx.prepare("SELECT id, note, fenetre, application, cree_le, texte FROM captures").map_err(erreur)?;
        let lignes = stmt.query_map([], |l| Ok((l.get(0)?, l.get(1)?, l.get(2)?, l.get(3)?, l.get(4)?, l.get(5)?))).map_err(erreur)?;
        lignes.collect::<Result<_, _>>().map_err(erreur)?
    };
    for (id, note, fenetre, application, cree_le, texte) in lignes {
        tx.execute(
            "INSERT INTO captures_fts (rowid, note, fenetre, application, jour, texte) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![id, note, fenetre, application, crate::captures::stockage::jour_de_cree_le(&cree_le), texte.unwrap_or_default()],
        )
        .map_err(erreur)?;
    }
    tx.commit().map_err(erreur)
}

// ponytail: un schema en `IF NOT EXISTS` et une colonne ajoutee quand elle
// manque, pas de table de migrations. A poser le jour ou une colonne change de sens.
fn preparer(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(SCHEMA).map_err(|e| format!("Schéma de la base impossible à poser : {e}"))?;
    // ADR-007 : le recadrage, `NULL` sans recadrage. Un exe plus ancien ignore la colonne.
    if !a_colonne(conn, "captures", "recadrage")? {
        conn.execute_batch("ALTER TABLE captures ADD COLUMN recadrage TEXT")
            .map_err(|e| format!("Colonne du recadrage impossible à poser : {e}"))?;
    }
    // ADR-012 : le fond, `NULL` sans fond.
    if !a_colonne(conn, "captures", "fond")? {
        conn.execute_batch("ALTER TABLE captures ADD COLUMN fond TEXT")
            .map_err(|e| format!("Colonne du fond impossible à poser : {e}"))?;
    }
    // ADR-010 : le texte reconnu, `NULL` tant qu'il n'est pas lu.
    if !a_colonne(conn, "captures", "texte")? {
        conn.execute_batch("ALTER TABLE captures ADD COLUMN texte TEXT")
            .map_err(|e| format!("Colonne du texte impossible à poser : {e}"))?;
    }
    // ADR-016 : la duree d'une video, `NULL` pour une image.
    if !a_colonne(conn, "captures", "duree_ms")? {
        conn.execute_batch("ALTER TABLE captures ADD COLUMN duree_ms INTEGER")
            .map_err(|e| format!("Colonne de la durée impossible à poser : {e}"))?;
    }
    // ADR-018 : l'orientation, 0 (telle que prise) par defaut. SQLite accepte une colonne
    // NOT NULL ajoutee avec un defaut constant. Un exe plus ancien ignore la colonne.
    if !a_colonne(conn, "captures", "orientation")? {
        conn.execute_batch("ALTER TABLE captures ADD COLUMN orientation INTEGER NOT NULL DEFAULT 0")
            .map_err(|e| format!("Colonne de l'orientation impossible à poser : {e}"))?;
    }
    if !a_colonne(conn, "captures_fts", "texte")? {
        refaire_index(conn)?;
    }
    Ok(())
}

pub fn ouvrir(chemin: &Path) -> Result<Connection, String> {
    let conn = Connection::open(chemin).map_err(|e| format!("Base {} impossible à ouvrir : {e}", chemin.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL").map_err(|e| format!("Mode WAL impossible : {e}"))?;
    preparer(&conn)?;
    Ok(conn)
}

#[cfg(test)]
pub fn open_in_memory() -> Result<Connection, String> {
    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    preparer(&conn)?;
    Ok(conn)
}

/// Un reglage, vide s'il n'est pas pose.
pub fn reglage(conn: &Connection, cle: &str) -> Result<String, String> {
    match conn.query_row("SELECT valeur FROM reglages WHERE cle = ?1", [cle], |l| l.get::<_, String>(0)) {
        Ok(valeur) => Ok(valeur),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(String::new()),
        Err(e) => Err(format!("Lecture du réglage {cle} impossible : {e}")),
    }
}

pub fn regler(conn: &Connection, cle: &str, valeur: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO reglages (cle, valeur) VALUES (?1, ?2) ON CONFLICT(cle) DO UPDATE SET valeur = excluded.valeur",
        [cle, valeur],
    )
    .map(|_| ())
    .map_err(|e| format!("Écriture du réglage {cle} impossible : {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_dev_seule_valeur_reconnue() {
        assert_eq!(lire_instance(None), Ok(false));
        assert_eq!(lire_instance(Some("")), Ok(false));
        assert_eq!(lire_instance(Some("dev")), Ok(true));
        assert!(lire_instance(Some("Dev")).is_err(), "une faute de frappe ne retombe pas sur la production");
    }

    #[test]
    fn demonstration_seulement_sur_l_instance_dev() {
        assert!(!lire_protection(true, Some("1")), "dev et EQUISHOT_DEMO=1 : visible en capture");
        assert!(lire_protection(false, Some("1")), "la production reste protégée, quel que soit le drapeau");
        assert!(lire_protection(true, None));
        assert!(lire_protection(true, Some("oui")), "seule la valeur 1 lève la protection");
    }

    #[test]
    fn base_d_avant_le_recadrage_recoit_sa_colonne() {
        let conn = Connection::open_in_memory().expect("base");
        conn.execute_batch(
            "CREATE TABLE captures (id INTEGER PRIMARY KEY AUTOINCREMENT, cree_le TEXT NOT NULL, modifie_le TEXT NOT NULL,
             fichier TEXT NOT NULL, largeur INTEGER NOT NULL, hauteur INTEGER NOT NULL, ecran TEXT NOT NULL DEFAULT '',
             echelle REAL NOT NULL DEFAULT 1, fenetre TEXT NOT NULL DEFAULT '', application TEXT NOT NULL DEFAULT '',
             note TEXT NOT NULL DEFAULT '', formes TEXT NOT NULL DEFAULT '[]');
             INSERT INTO captures (cree_le, modifie_le, fichier, largeur, hauteur) VALUES ('t', 't', 'a.png', 4, 3);",
        )
        .expect("base d'avant");
        preparer(&conn).expect("migrée");
        preparer(&conn).expect("une seconde fois sans faute");
        let recadrage: Option<String> = conn.query_row("SELECT recadrage FROM captures WHERE id = 1", [], |l| l.get(0)).expect("colonne");
        assert_eq!(recadrage, None, "la ligne d'avant n'est pas recadrée");
        let fond: Option<String> = conn.query_row("SELECT fond FROM captures WHERE id = 1", [], |l| l.get(0)).expect("colonne du fond");
        assert_eq!(fond, None, "la ligne d'avant n'a pas de fond");
        let orientation: i64 = conn.query_row("SELECT orientation FROM captures WHERE id = 1", [], |l| l.get(0)).expect("colonne de l'orientation");
        assert_eq!(orientation, 0, "la ligne d'avant est telle que prise");
    }

    #[test]
    fn db_texte_mise_a_niveau_d_une_base_0_8() {
        let conn = Connection::open_in_memory().expect("base");
        // La base telle que la 0.8.0 la laisse.
        conn.execute_batch(
            "CREATE TABLE captures (id INTEGER PRIMARY KEY AUTOINCREMENT, cree_le TEXT NOT NULL, modifie_le TEXT NOT NULL,
             fichier TEXT NOT NULL, largeur INTEGER NOT NULL, hauteur INTEGER NOT NULL, ecran TEXT NOT NULL DEFAULT '',
             echelle REAL NOT NULL DEFAULT 1, fenetre TEXT NOT NULL DEFAULT '', application TEXT NOT NULL DEFAULT '',
             note TEXT NOT NULL DEFAULT '', formes TEXT NOT NULL DEFAULT '[]', recadrage TEXT);
             CREATE VIRTUAL TABLE captures_fts USING fts5(note, fenetre, application, jour, tokenize = 'unicode61 remove_diacritics 2');
             INSERT INTO captures (cree_le, modifie_le, fichier, largeur, hauteur, fenetre, note)
                 VALUES ('2026-10-05T10:00:00', '2026-10-05T10:00:00', 'a.png', 1, 1, 'Bloc-notes', 'relance');
             INSERT INTO captures_fts (rowid, note, fenetre, application, jour) VALUES (1, 'relance', 'Bloc-notes', '', '2026-10-05');",
        )
        .expect("base 0.8.0");
        preparer(&conn).expect("mise à niveau");
        preparer(&conn).expect("une seconde fois sans faute");
        let texte: Option<String> = conn.query_row("SELECT texte FROM captures WHERE id = 1", [], |l| l.get(0)).expect("colonne");
        assert_eq!(texte, None, "une ancienne capture est à lire");
        let cherche = |requete: &str| -> Vec<i64> {
            let mut stmt = conn.prepare("SELECT rowid FROM captures_fts WHERE captures_fts MATCH ?1").expect("requête");
            stmt.query_map([requete], |l| l.get(0)).expect("lue").collect::<Result<_, _>>().expect("lignes")
        };
        assert_eq!(cherche("\"bloc\"*"), vec![1], "l'ancienne fenêtre est dans le nouvel index");
        assert_eq!(cherche("\"relance\"*"), vec![1], "l'ancienne note aussi");
        assert_eq!(cherche("\"octobre\"*"), vec![1], "et le jour, recalculé");
        conn.execute("UPDATE captures_fts SET texte = 'facture' WHERE rowid = 1", []).expect("texte posé");
        assert_eq!(cherche("\"factu\"*"), vec![1]);
        // Un exe 0.8.0 insère en nommant ses colonnes : il marche encore.
        conn.execute("INSERT INTO captures_fts (rowid, note, fenetre, application, jour) VALUES (2, '', 'x', '', 'j')", [])
            .expect("insertion à l'ancienne");
    }

    #[test]
    fn reglage_absent_vide_puis_ecrit_puis_remplace() {
        let conn = open_in_memory().expect("base");
        assert_eq!(reglage(&conn, "x").expect("lu"), "");
        regler(&conn, "x", "1").expect("écrit");
        regler(&conn, "x", "2").expect("remplacé");
        assert_eq!(reglage(&conn, "x").expect("relu"), "2");
    }

    #[test]
    fn une_base_d_avant_la_0_19_recoit_la_table_des_scenes() {
        let conn = Connection::open_in_memory().expect("base");
        conn.execute_batch("CREATE TABLE reglages (cle TEXT PRIMARY KEY, valeur TEXT NOT NULL);").expect("base d'avant");
        preparer(&conn).expect("migrée");
        preparer(&conn).expect("une seconde fois sans faute");
        conn.execute("INSERT INTO scenes (nom, donnees, maj) VALUES ('Démo', '{}', 't')", []).expect("table des scènes");
        assert!(conn.execute("INSERT INTO scenes (nom, donnees, maj) VALUES ('DÉMO', '{}', 't')", []).is_ok(), "NOCASE ne plie que l'ASCII : É n'est pas é");
        assert!(conn.execute("INSERT INTO scenes (nom, donnees, maj) VALUES ('démo', '{}', 't')", []).is_err(), "nom unique, casse ASCII ignorée");
    }
}
