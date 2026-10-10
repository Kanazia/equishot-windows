//! Fichiers et lignes des captures.
//!
//! Une capture, c'est trois fichiers PNG cote a cote (l'original, jamais
//! modifie, le rendu qui porte les annotations, la vignette de la galerie) et
//! une ligne de la table `captures`, doublee d'une ligne de `captures_fts`.

use std::path::{Path, PathBuf};

use image::RgbaImage;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// Sous-dossier du dossier des captures ou partent les fichiers supprimes.
pub const CORBEILLE: &str = "corbeille";
/// Plus grand cote d'une vignette de la galerie, en pixels.
const COTE_VIGNETTE: u32 = 360;

/// Zone tracee dans la fenetre de selection, en pixels CSS de cette fenetre.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
pub struct ZoneCss {
    pub x: f64,
    pub y: f64,
    pub largeur: f64,
    pub hauteur: f64,
}

/// La meme zone dans l'image figee de l'ecran, en pixels physiques, et la
/// position de son coin sur le bureau (l'origine d'un ecran place a gauche du
/// principal est negative).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ZonePhysique {
    pub x: u32,
    pub y: u32,
    pub largeur: u32,
    pub hauteur: u32,
    pub bureau: (i32, i32),
}

/// Passe d'une zone CSS a une zone physique, bornee a l'image. `facteur` est
/// le nombre de pixels physiques par pixel CSS de la fenetre de selection.
/// `None` si la zone est vide. La page fait le meme arrondi (`zonePhysique`
/// de `formes.ts`) : les deux bords sont arrondis, pas la largeur.
pub fn zone_physique(zone: ZoneCss, facteur: f64, origine: (i32, i32), image: (u32, u32)) -> Option<ZonePhysique> {
    if !(facteur.is_finite() && facteur > 0.0) {
        return None;
    }
    let borne = |valeur: f64, max: u32| (valeur * facteur).round().clamp(0.0, max as f64) as u32;
    let (x0, y0) = (borne(zone.x, image.0), borne(zone.y, image.1));
    let (x1, y1) = (borne(zone.x + zone.largeur, image.0), borne(zone.y + zone.hauteur, image.1));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(ZonePhysique {
        x: x0,
        y: y0,
        largeur: x1 - x0,
        hauteur: y1 - y0,
        bureau: (origine.0 + x0 as i32, origine.1 + y0 as i32),
    })
}

/// Le cadre `(x, y, largeur, hauteur)` d'une fenetre sur le bureau, rogne a
/// l'ecran dont le coin est `position` (ADR-008). `None` s'il n'en reste rien.
pub fn rogner_a_l_ecran(cadre: (i32, i32, u32, u32), position: (i32, i32), taille: (u32, u32)) -> Option<ZonePhysique> {
    let (x, y, largeur, hauteur) = (cadre.0 as i64, cadre.1 as i64, cadre.2 as i64, cadre.3 as i64);
    let (px, py) = (position.0 as i64, position.1 as i64);
    let (x0, y0) = (x.max(px), y.max(py));
    let (x1, y1) = ((x + largeur).min(px + taille.0 as i64), (y + hauteur).min(py + taille.1 as i64));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(ZonePhysique {
        x: (x0 - px) as u32,
        y: (y0 - py) as u32,
        largeur: (x1 - x0) as u32,
        hauteur: (y1 - y0) as u32,
        bureau: (x0 as i32, y0 as i32),
    })
}

/// Cote du carre de la loupe, en pixels physiques : impair, le curseur au milieu.
pub const COTE_LOUPE: u32 = 15;

/// La region de l'ecran que lit la loupe autour du curseur (sur le bureau), et
/// la place de son coin dans le carre : au bord de l'ecran, la region est
/// tronquee et le reste du carre reste vide. `None` si le curseur est hors de l'ecran.
pub fn cadre_loupe(curseur: (i32, i32), position: (i32, i32), taille: (u32, u32)) -> Option<(ZonePhysique, (u32, u32))> {
    let demi = (COTE_LOUPE / 2) as i64;
    let (cx, cy) = (curseur.0 as i64 - position.0 as i64, curseur.1 as i64 - position.1 as i64);
    let (l, h) = (taille.0 as i64, taille.1 as i64);
    if cx < 0 || cy < 0 || cx >= l || cy >= h {
        return None;
    }
    let (x0, y0) = ((cx - demi).max(0), (cy - demi).max(0));
    let (x1, y1) = ((cx + demi + 1).min(l), (cy + demi + 1).min(h));
    let zone = ZonePhysique {
        x: x0 as u32,
        y: y0 as u32,
        largeur: (x1 - x0) as u32,
        hauteur: (y1 - y0) as u32,
        bureau: (position.0 + x0 as i32, position.1 + y0 as i32),
    };
    Some((zone, ((x0 - (cx - demi)) as u32, (y0 - (cy - demi)) as u32)))
}

/// Les octets que la page recoit de la loupe : x et y du curseur dans l'ecran
/// (i32 petit-boutistes), puis le carre en RGBA, transparent hors de l'ecran.
pub fn assembler_loupe(lue: &RgbaImage, decalage: (u32, u32), curseur: (i32, i32)) -> Vec<u8> {
    let mut carre = RgbaImage::new(COTE_LOUPE, COTE_LOUPE);
    image::imageops::replace(&mut carre, lue, decalage.0 as i64, decalage.1 as i64);
    let mut octets = Vec::with_capacity(8 + carre.as_raw().len());
    octets.extend_from_slice(&curseur.0.to_le_bytes());
    octets.extend_from_slice(&curseur.1.to_le_bytes());
    octets.extend_from_slice(carre.as_raw());
    octets
}

/// Le point le plus proche de `point` sur l'ecran dont le coin est `position`.
pub fn borner_a_l_ecran(point: (i32, i32), position: (i32, i32), taille: (u32, u32)) -> (i32, i32) {
    (
        point.0.clamp(position.0, position.0 + taille.0 as i32 - 1),
        point.1.clamp(position.1, position.1 + taille.1 as i32 - 1),
    )
}

/// La derniere zone figee (ADR-009), relative a son ecran, en pixels physiques,
/// avec ce qu'il faut pour retrouver cet ecran et remplir la fiche.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DerniereZone {
    pub x: u32,
    pub y: u32,
    pub largeur: u32,
    pub hauteur: u32,
    /// Coin de l'ecran sur le bureau et sa taille, en pixels physiques.
    pub position: (i32, i32),
    pub taille: (u32, u32),
    pub ecran: String,
    pub echelle: f64,
}

/// Le rang de l'ecran `(position, taille)` qui est encore celui de la zone
/// gardee, si elle y tient. Un ecran deplace, redimensionne ou debranche ne
/// se reprend pas : on prendrait une autre partie de l'ecran sans le dire.
pub fn ecran_de_la_zone(ecrans: &[((i32, i32), (u32, u32))], gardee: &DerniereZone) -> Option<usize> {
    let tient = gardee.largeur > 0
        && gardee.hauteur > 0
        && gardee.x as u64 + gardee.largeur as u64 <= gardee.taille.0 as u64
        && gardee.y as u64 + gardee.hauteur as u64 <= gardee.taille.1 as u64;
    if !tient {
        return None;
    }
    ecrans.iter().position(|&(position, taille)| position == gardee.position && taille == gardee.taille)
}

/// Les trois fichiers d'une capture.
pub struct Chemins {
    pub origine: PathBuf,
    pub rendu: PathBuf,
    pub vignette: PathBuf,
}

/// `fichier` est le chemin de l'original, relatif au dossier des captures.
pub fn chemins(dossier: &Path, fichier: &str) -> Chemins {
    let origine = dossier.join(fichier);
    Chemins {
        rendu: origine.with_extension("rendu.png"),
        vignette: origine.with_extension("vignette.png"),
        origine,
    }
}

/// `AAAA-MM/AAAAMMJJ-HHMMSS-mmm.png`, relatif au dossier des captures.
pub fn nom_fichier(quand: &chrono::DateTime<chrono::Local>) -> String {
    quand.format("%Y-%m/%Y%m%d-%H%M%S-%3f.png").to_string()
}

fn ecrire_png(chemin: &Path, image: &RgbaImage) -> Result<(), String> {
    image
        .save_with_format(chemin, image::ImageFormat::Png)
        .map_err(|e| format!("Écriture de {} impossible : {e}", chemin.display()))
}

fn vignette(image: &RgbaImage) -> RgbaImage {
    let (largeur, hauteur) = image.dimensions();
    let grand = largeur.max(hauteur);
    if grand <= COTE_VIGNETTE {
        return image.clone();
    }
    let reduit = |cote: u32| ((cote as f64 * COTE_VIGNETTE as f64 / grand as f64).round() as u32).max(1);
    image::imageops::thumbnail(image, reduit(largeur), reduit(hauteur))
}

/// Ecrit l'original d'une capture neuve. Refuse d'ecraser : l'original ne se
/// modifie jamais.
pub fn ecrire_origine(dossier: &Path, fichier: &str, origine: &RgbaImage) -> Result<(), String> {
    let chemin = chemins(dossier, fichier).origine;
    if let Some(parent) = chemin.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Création de {} impossible : {e}", parent.display()))?;
    }
    if chemin.exists() {
        return Err(format!("{} existe déjà : une capture ne s'écrase pas.", chemin.display()));
    }
    ecrire_png(&chemin, origine)
}

/// Ecrit le rendu et la vignette, a l'enregistrement et a chaque modification
/// des formes.
pub fn ecrire_rendu(dossier: &Path, fichier: &str, rendu: &RgbaImage) -> Result<(), String> {
    let c = chemins(dossier, fichier);
    ecrire_png(&c.rendu, rendu)?;
    ecrire_png(&c.vignette, &vignette(rendu))
}

/// Lit un PNG du disque en RGBA.
pub fn lire_png(chemin: &Path) -> Result<RgbaImage, String> {
    image::open(chemin)
        .map(|image| image.to_rgba8())
        .map_err(|e| format!("Lecture de {} impossible : {e}", chemin.display()))
}

/// Ce que la capture sait de son origine, pose une fois pour toutes.
#[derive(Debug, Clone, Default)]
pub struct Origine {
    pub ecran: String,
    pub echelle: f64,
    pub fenetre: String,
    pub application: String,
}

/// Une capture telle que la page la recoit : chemins absolus, formes en JSON.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capture {
    pub id: i64,
    pub cree_le: String,
    pub modifie_le: String,
    pub origine: String,
    pub rendu: String,
    pub vignette: String,
    pub largeur: u32,
    pub hauteur: u32,
    pub ecran: String,
    pub echelle: f64,
    pub fenetre: String,
    pub application: String,
    pub note: String,
    pub formes: serde_json::Value,
    /// La partie de l'original que montre le rendu (ADR-007), `null` pour l'image entiere.
    pub recadrage: Option<Recadrage>,
    /// Le texte reconnu (ADR-010) : `null` pas encore lu, vide si rien n'est reconnu.
    pub texte: Option<String>,
    /// Le fond autour du rendu (ADR-012), `null` sans fond.
    pub fond: Option<Fond>,
    /// La duree d'une video (ADR-016), `null` pour une image. `origine` est alors le MP4.
    pub duree_ms: Option<i64>,
    /// L'orientation (ADR-018), 0 a 7 : `(miroir ? 4 : 0) + quarts de tour horaires`, 0 telle que prise.
    pub orientation: u8,
}

impl Capture {
    /// La taille du rendu : le cadre, sinon l'original, plus deux marges du fond.
    pub fn taille_rendu(&self) -> (u32, u32) {
        // Le cadre vit dans l'espace oriente (ADR-018) ; sans cadre, l'original oriente.
        let (l, h) = self.recadrage.map_or(taille_orientee((self.largeur, self.hauteur), self.orientation), |r| (r.largeur, r.hauteur));
        let m = self.fond.as_ref().map_or(0, |f| marge_px((l, h), f));
        (l + 2 * m, h + 2 * m)
    }
}

/// Un rectangle de l'original, en pixels entiers, non vide et dans l'image.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Recadrage {
    pub x: u32,
    pub y: u32,
    pub largeur: u32,
    pub hauteur: u32,
}

/// Le recadrage envoye par la page, controle contre la taille de l'original.
/// `null` et l'image entiere valent « aucun recadrage ».
pub fn valider_recadrage(valeur: &serde_json::Value, taille: (u32, u32)) -> Result<Option<Recadrage>, String> {
    if valeur.is_null() {
        return Ok(None);
    }
    // Un objet nomme : serde lirait aussi un tableau de quatre nombres.
    if !valeur.is_object() {
        return Err("Le recadrage est un objet { x, y, largeur, hauteur }.".to_string());
    }
    let r: Recadrage = serde_json::from_value(valeur.clone()).map_err(|e| format!("Recadrage illisible : {e}"))?;
    let dedans = |debut: u32, longueur: u32, max: u32| longueur > 0 && debut.checked_add(longueur).is_some_and(|fin| fin <= max);
    if !(dedans(r.x, r.largeur, taille.0) && dedans(r.y, r.hauteur, taille.1)) {
        return Err(format!("Le recadrage sort de l'image de {} x {} ou ne garde rien : refusé.", taille.0, taille.1));
    }
    Ok((r != Recadrage { x: 0, y: 0, largeur: taille.0, hauteur: taille.1 }).then_some(r))
}

/// L'orientation envoyee par la page (ADR-018) : un entier de 0 a 7, rien d'autre.
/// `null` et absent sont refuses aussi : la page de la 0.17.1 l'envoie toujours.
pub fn valider_orientation(valeur: &serde_json::Value) -> Result<u8, String> {
    match valeur.as_u64() {
        Some(v @ 0..=7) => Ok(v as u8),
        _ => Err("L'orientation va de 0 à 7.".to_string()),
    }
}

/// Largeur et hauteur une fois orientee : un quart de tour les echange. Miroir de `tailleOrientee`.
pub fn taille_orientee((largeur, hauteur): (u32, u32), orientation: u8) -> (u32, u32) {
    if orientation % 2 == 1 { (hauteur, largeur) } else { (largeur, hauteur) }
}

/// L'image orientee : le miroir horizontal d'abord, puis les quarts de tour horaires
/// (`rotate90` tourne dans le sens horaire, `image` 0.25.10). Orientation 0 : un clone.
pub fn orienter(image: &RgbaImage, orientation: u8) -> RgbaImage {
    let base = if orientation >= 4 { image::imageops::flip_horizontal(image) } else { image.clone() };
    match orientation % 4 {
        1 => image::imageops::rotate90(&base),
        2 => image::imageops::rotate180(&base),
        3 => image::imageops::rotate270(&base),
        _ => base,
    }
}

/// La partie de l'image que garde le recadrage, deja valide.
pub fn recadrer(image: &RgbaImage, recadrage: Option<Recadrage>) -> RgbaImage {
    match recadrage {
        Some(r) => image::imageops::crop_imm(image, r.x, r.y, r.largeur, r.hauteur).to_image(),
        None => image.clone(),
    }
}

/// Le fond d'une capture (ADR-012) : deux couleurs en degrade diagonal (deux fois
/// la meme pour un fond uni), une marge en pourcents du plus grand cote de l'image
/// montree, un arrondi des coins en pourcents de son plus petit cote, une ombre.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fond {
    pub couleurs: [String; 2],
    pub marge: u32,
    pub arrondi: u32,
    pub ombre: bool,
}

const MARGE_MAX: u32 = 25;
const ARRONDI_MAX: u32 = 10;

/// Une couleur `#rrggbb` : celles des formes, du fond (ADR-012) et de la pipette (ADR-013).
pub fn est_couleur(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|h| h.is_ascii_hexdigit())
}

/// Le fond envoye par la page. `null` : aucun fond.
pub fn valider_fond(valeur: &serde_json::Value) -> Result<Option<Fond>, String> {
    if valeur.is_null() {
        return Ok(None);
    }
    let fond: Fond = serde_json::from_value(valeur.clone()).map_err(|e| format!("Fond illisible : {e}"))?;
    if !fond.couleurs.iter().all(|c| est_couleur(c)) {
        return Err("Les couleurs du fond s'écrivent #rrggbb.".to_string());
    }
    if fond.marge > MARGE_MAX || fond.arrondi > ARRONDI_MAX {
        return Err(format!("La marge du fond va de 0 à {MARGE_MAX}, son arrondi de 0 à {ARRONDI_MAX}."));
    }
    Ok(Some(fond))
}

/// La marge en pixels, pour une image montree de `taille`.
pub fn marge_px(taille: (u32, u32), fond: &Fond) -> u32 {
    (taille.0.max(taille.1) as f64 * fond.marge as f64 / 100.0).round() as u32
}

fn rayon_px(taille: (u32, u32), fond: &Fond) -> u32 {
    (taille.0.min(taille.1) as f64 * fond.arrondi as f64 / 100.0).round() as u32
}

fn rvb(c: &str) -> [f32; 3] {
    let v = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).unwrap_or(0) as f32;
    [v(1), v(3), v(5)]
}

/// Part du pixel (x, y) couverte par un rectangle de `taille` aux coins de `rayon`,
/// anticrenelee sur un pixel.
fn couverture(x: u32, y: u32, taille: (u32, u32), rayon: f32) -> f32 {
    if rayon <= 0.0 {
        return 1.0;
    }
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (cx, cy) = (px.clamp(rayon, taille.0 as f32 - rayon), py.clamp(rayon, taille.1 as f32 - rayon));
    (rayon - ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() + 0.5).clamp(0.0, 1.0)
}

/// Part d'ombre au plus, sous le centre de l'image.
const FORCE_OMBRE: f32 = 0.45;

/// L'image posee sur son fond (ADR-012) : le degrade, l'ombre floutee et decalee
/// vers le bas, puis l'image aux coins arrondis. Sans fond, l'image telle quelle.
pub fn habiller(image: &RgbaImage, fond: Option<&Fond>) -> RgbaImage {
    let Some(fond) = fond else { return image.clone() };
    let taille = image.dimensions();
    let (m, rayon) = (marge_px(taille, fond), rayon_px(taille, fond) as f32);
    let (tl, th) = (taille.0 + 2 * m, taille.1 + 2 * m);
    let (a, b) = (rvb(&fond.couleurs[0]), rvb(&fond.couleurs[1]));
    let diagonale = ((tl - 1) + (th - 1)).max(1) as f32;
    let mut toile = RgbaImage::from_fn(tl, th, |x, y| {
        let t = (x + y) as f32 / diagonale;
        let c = |i: usize| (a[i] + (b[i] - a[i]) * t).round() as u8;
        image::Rgba([c(0), c(1), c(2), 255])
    });
    if fond.ombre && m > 0 {
        let decalage = m / 4;
        let mut masque = image::GrayImage::new(tl, th);
        for y in 0..taille.1.min(th - m - decalage) {
            for x in 0..taille.0 {
                let k = couverture(x, y, taille, rayon);
                masque.put_pixel(x + m, y + m + decalage, image::Luma([(k * 255.0).round() as u8]));
            }
        }
        let flou = image::imageops::fast_blur(&masque, (m as f32 / 3.0).max(1.0));
        for (x, y, p) in toile.enumerate_pixels_mut() {
            let k = flou.get_pixel(x, y).0[0] as f32 / 255.0 * FORCE_OMBRE;
            for c in &mut p.0[..3] {
                *c = (*c as f32 * (1.0 - k)).round() as u8;
            }
        }
    }
    let mut posee = image.clone();
    if rayon > 0.0 {
        for (x, y, p) in posee.enumerate_pixels_mut() {
            let k = couverture(x, y, taille, rayon);
            if k < 1.0 {
                p.0[3] = (p.0[3] as f32 * k).round() as u8;
            }
        }
    }
    image::imageops::overlay(&mut toile, &posee, m as i64, m as i64);
    toile
}

const COLONNES: &str =
    "c.id, c.cree_le, c.modifie_le, c.fichier, c.largeur, c.hauteur, c.ecran, c.echelle, c.fenetre, c.application, c.note, c.formes, c.recadrage, c.texte, c.fond, c.duree_ms, c.orientation";

fn depuis_ligne(dossier: &Path, ligne: &rusqlite::Row<'_>) -> rusqlite::Result<Capture> {
    let fichier: String = ligne.get(3)?;
    let c = chemins(dossier, &fichier);
    let formes: String = ligne.get(11)?;
    let texte = |chemin: PathBuf| chemin.to_string_lossy().into_owned();
    Ok(Capture {
        id: ligne.get(0)?,
        cree_le: ligne.get(1)?,
        modifie_le: ligne.get(2)?,
        origine: texte(c.origine),
        rendu: texte(c.rendu),
        vignette: texte(c.vignette),
        largeur: ligne.get(4)?,
        hauteur: ligne.get(5)?,
        ecran: ligne.get(6)?,
        echelle: ligne.get(7)?,
        fenetre: ligne.get(8)?,
        application: ligne.get(9)?,
        note: ligne.get(10)?,
        formes: serde_json::from_str(&formes).unwrap_or_else(|_| serde_json::json!([])),
        recadrage: ligne.get::<_, Option<String>>(12)?.and_then(|texte| serde_json::from_str(&texte).ok()),
        texte: ligne.get(13)?,
        fond: ligne.get::<_, Option<String>>(14)?.and_then(|texte| serde_json::from_str(&texte).ok()),
        duree_ms: ligne.get(15)?,
        // Une valeur hors de 0 a 7 (base touchee a la main) retombe sur 0, comme des formes illisibles.
        orientation: ligne.get::<_, i64>(16).ok().and_then(|v| u8::try_from(v).ok()).filter(|v| *v <= 7).unwrap_or(0),
    })
}

/// Points d'un trace de crayon au plus : miroir de `POINTS_MAX` dans `Annotation.tsx`.
pub const POINTS_MAX: usize = 4000;

/// Les formes sont une liste de formes connues (ADR-005) : un type, ses nombres
/// finis, une couleur `#rrggbb` et une taille de 1 a 3 facultatives. Une seule
/// forme fausse fait refuser la liste ; rien d'autre n'entre en base.
pub fn valider_formes(formes: &serde_json::Value) -> Result<String, String> {
    let liste = formes.as_array().ok_or("Les formes d'une capture sont une liste.")?;
    if liste.len() > 500 {
        return Err("Trop de formes sur une seule capture (500 au plus).".to_string());
    }
    for (i, forme) in liste.iter().enumerate() {
        valider_forme(forme).map_err(|e| format!("Forme {} refusée : {e}", i + 1))?;
    }
    Ok(formes.to_string())
}

fn valider_forme(forme: &serde_json::Value) -> Result<(), String> {
    let objet = forme.as_object().ok_or("ce n'est pas un objet.")?;
    let nombre = |cle: &str| match objet.get(cle).and_then(|v| v.as_f64()) {
        Some(n) if n.is_finite() => Ok(n),
        _ => Err(format!("« {cle} » manque ou n'est pas un nombre.")),
    };
    if let Some(couleur) = objet.get("couleur") {
        if !couleur.as_str().is_some_and(est_couleur) {
            return Err("la couleur s'écrit #rrggbb.".to_string());
        }
    }
    if let Some(taille) = objet.get("taille") {
        if !matches!(taille.as_u64(), Some(1..=3)) {
            return Err("la taille vaut 1, 2 ou 3.".to_string());
        }
    }
    match objet.get("type").and_then(|t| t.as_str()) {
        Some("fleche" | "ligne" | "surligneur") => ["x1", "y1", "x2", "y2"].iter().try_for_each(|c| nombre(c).map(|_| ())),
        Some("rectangle" | "ellipse" | "pixellisation" | "projecteur" | "loupe") => ["x", "y", "largeur", "hauteur"].iter().try_for_each(|c| nombre(c).map(|_| ())),
        Some("texte") => {
            nombre("x")?;
            nombre("y")?;
            match objet.get("texte").and_then(|t| t.as_str()) {
                Some(t) if !t.is_empty() && t.chars().count() <= 500 => Ok(()),
                _ => Err("le texte compte de 1 à 500 caractères.".to_string()),
            }
        }
        Some("tampon") => {
            nombre("x")?;
            nombre("y")?;
            match objet.get("symbole").and_then(|s| s.as_str()) {
                Some(s) if (1..=16).contains(&s.chars().count()) && !s.chars().any(char::is_control) => Ok(()),
                _ => Err("le symbole d'un tampon compte de 1 à 16 caractères.".to_string()),
            }
        }
        Some("etape") => {
            nombre("x")?;
            nombre("y")?;
            match objet.get("numero").and_then(|n| n.as_u64()) {
                Some(1..=999) => Ok(()),
                _ => Err("le numéro d'une étape va de 1 à 999.".to_string()),
            }
        }
        Some("crayon") => {
            let points = objet.get("points").and_then(|p| p.as_array()).ok_or("un crayon porte une liste de points.")?;
            let pair = !points.is_empty() && points.len() % 2 == 0 && points.len() <= 2 * POINTS_MAX;
            if pair && points.iter().all(|p| p.as_f64().is_some_and(f64::is_finite)) {
                Ok(())
            } else {
                Err(format!("un crayon porte de 1 à {POINTS_MAX} points, x puis y."))
            }
        }
        Some(autre) => Err(format!("type « {autre} » inconnu.")),
        None => Err("le type manque.".to_string()),
    }
}

/// Pose la ligne d'une capture neuve et sa ligne de recherche, et rend son
/// identifiant. Elle ne relit rien : un appelant qui efface les fichiers quand
/// la ligne ne s'ecrit pas ne doit pas les effacer pour une relecture manquee.
pub fn inserer(
    conn: &Connection,
    quand: &chrono::DateTime<chrono::Local>,
    fichier: &str,
    taille: (u32, u32),
    origine: &Origine,
    formes: &str,
    texte: Option<&str>,
) -> Result<i64, String> {
    poser_ligne(conn, quand, fichier, taille, origine, formes, texte, None)
}

#[allow(clippy::too_many_arguments)]
fn poser_ligne(
    conn: &Connection,
    quand: &chrono::DateTime<chrono::Local>,
    fichier: &str,
    taille: (u32, u32),
    origine: &Origine,
    formes: &str,
    texte: Option<&str>,
    duree_ms: Option<i64>,
) -> Result<i64, String> {
    let horodatage = quand.format("%Y-%m-%dT%H:%M:%S").to_string();
    // Les deux lignes ou aucune : une capture sans ligne de recherche serait
    // dans la galerie et introuvable.
    let tx = conn.unchecked_transaction().map_err(|e| format!("Transaction impossible : {e}"))?;
    tx.execute(
        "INSERT INTO captures (cree_le, modifie_le, fichier, largeur, hauteur, ecran, echelle, fenetre, application, formes, texte, duree_ms)
         VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![horodatage, fichier, taille.0, taille.1, origine.ecran, origine.echelle, origine.fenetre, origine.application, formes, texte, duree_ms],
    )
    .map_err(|e| format!("Enregistrement de la capture impossible : {e}"))?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO captures_fts (rowid, note, fenetre, application, jour, texte) VALUES (?1, '', ?2, ?3, ?4, ?5)",
        params![id, origine.fenetre, origine.application, jour_cherchable(&quand.date_naive()), texte.unwrap_or("")],
    )
    .map_err(|e| format!("Indexation de la capture impossible : {e}"))?;
    tx.commit().map_err(|e| format!("Enregistrement de la capture impossible : {e}"))?;
    Ok(id)
}

/// Pose la ligne d'une video neuve (ADR-016). `fichier` est le chemin absolu du
/// MP4, hors du dossier des captures. Son texte est vide d'office : la lecture
/// de texte (ADR-010) ne la prend jamais.
pub fn inserer_video(
    conn: &Connection,
    quand: &chrono::DateTime<chrono::Local>,
    fichier: &str,
    taille: (u32, u32),
    origine: &Origine,
    duree_ms: i64,
) -> Result<i64, String> {
    poser_ligne(conn, quand, fichier, taille, origine, "[]", Some(""), Some(duree_ms))
}

const MOIS: [&str; 12] =
    ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];

/// Le jour d'une capture, sous les formes qu'on tape pour le retrouver : la
/// date entiere `2026-10-02` ou `02/10/2026`, et le mois en toutes lettres.
/// Le jour seul n'y est pas : la recherche se fait par prefixe, et « 2 » est
/// le debut de « 2026 ». Seule la date entiere designe un jour.
fn jour_cherchable(quand: &chrono::NaiveDate) -> String {
    use chrono::Datelike;
    format!("{} {} {} {}", quand.format("%Y-%m-%d"), quand.format("%d/%m/%Y"), MOIS[quand.month0() as usize], quand.year())
}

/// Le jour cherchable d'une ligne relue, depuis son `cree_le` (`2026-10-05T10:00:00`).
/// Vide si la date ne se lit pas.
pub(crate) fn jour_de_cree_le(cree_le: &str) -> String {
    cree_le
        .get(..10)
        .and_then(|jour| chrono::NaiveDate::parse_from_str(jour, "%Y-%m-%d").ok())
        .map(|jour| jour_cherchable(&jour))
        .unwrap_or_default()
}

/// Pose le texte reconnu d'une capture et le rend cherchable (ADR-010). Une
/// capture supprimee pendant sa lecture n'est pas une erreur.
pub fn poser_texte(conn: &Connection, id: i64, texte: &str) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| format!("Transaction impossible : {e}"))?;
    tx.execute("UPDATE captures SET texte = ?1 WHERE id = ?2", params![texte, id])
        .and_then(|_| tx.execute("UPDATE captures_fts SET texte = ?1 WHERE rowid = ?2", params![texte, id]))
        .map_err(|e| format!("Enregistrement du texte impossible : {e}"))?;
    tx.commit().map_err(|e| format!("Enregistrement du texte impossible : {e}"))
}

/// Le texte reconnu d'une capture : `None` tant qu'il n'est pas lu.
pub fn texte_de(conn: &Connection, id: i64) -> Result<Option<String>, String> {
    conn.query_row("SELECT texte FROM captures WHERE id = ?1", [id], |ligne| ligne.get(0))
        .map_err(|e| format!("Capture {id} introuvable : {e}"))
}

/// La capture la plus recente dont le texte n'est pas lu : une capture neuve
/// passe ainsi devant le rattrapage des anciennes. Son id et son fichier.
pub fn prochaine_a_lire(conn: &Connection) -> Result<Option<(i64, String)>, String> {
    match conn.query_row("SELECT id, fichier FROM captures WHERE texte IS NULL ORDER BY id DESC LIMIT 1", [], |l| {
        Ok((l.get(0)?, l.get(1)?))
    }) {
        Ok(ligne) => Ok(Some(ligne)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(format!("Lecture des captures à lire impossible : {e}")),
    }
}

pub fn lire(conn: &Connection, dossier: &Path, id: i64) -> Result<Capture, String> {
    conn.query_row(&format!("SELECT {COLONNES} FROM captures c WHERE c.id = ?1"), [id], |ligne| depuis_ligne(dossier, ligne))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => format!("Capture {id} introuvable."),
            autre => format!("Lecture de la capture {id} impossible : {autre}"),
        })
}

/// Traduit une saisie libre en requete FTS5 : un mot devient une phrase citee
/// suivie de `*`, ce qui cherche par prefixe. Les guillemets sont doubles, la
/// seule echappee que comprend FTS5.
fn requete_fts(texte: &str) -> String {
    texte.split_whitespace().map(|mot| format!("\"{}\"*", mot.replace('"', "\"\""))).collect::<Vec<_>>().join(" ")
}

/// La galerie, la plus recente en premier. Une recherche non vide passe par
/// l'index plein texte (note, fenetre, application, jour).
// ponytail: pas de pagination, la galerie lit tout. A paginer au-dela de
// quelques milliers de captures.
pub fn lister(conn: &Connection, dossier: &Path, recherche: &str) -> Result<Vec<Capture>, String> {
    let requete = requete_fts(recherche);
    let erreur = |e: rusqlite::Error| format!("Lecture des captures impossible : {e}");
    if requete.is_empty() {
        let mut stmt = conn.prepare(&format!("SELECT {COLONNES} FROM captures c ORDER BY c.id DESC")).map_err(erreur)?;
        let lignes = stmt.query_map([], |ligne| depuis_ligne(dossier, ligne)).map_err(erreur)?;
        return lignes.collect::<Result<Vec<_>, _>>().map_err(erreur);
    }
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLONNES} FROM captures c JOIN captures_fts f ON f.rowid = c.id
             WHERE captures_fts MATCH ?1 ORDER BY c.id DESC"
        ))
        .map_err(erreur)?;
    let lignes = stmt.query_map([requete], |ligne| depuis_ligne(dossier, ligne)).map_err(erreur)?;
    lignes.collect::<Result<Vec<_>, _>>().map_err(erreur)
}

/// Enregistre la note libre et la rend cherchable.
pub fn noter(conn: &Connection, id: i64, note: &str) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| format!("Transaction impossible : {e}"))?;
    let touchees = tx
        .execute("UPDATE captures SET note = ?1 WHERE id = ?2", params![note, id])
        .map_err(|e| format!("Enregistrement de la note impossible : {e}"))?;
    if touchees == 0 {
        return Err(format!("Capture {id} introuvable."));
    }
    tx.execute("UPDATE captures_fts SET note = ?1 WHERE rowid = ?2", params![note, id])
        .map_err(|e| format!("Indexation de la note impossible : {e}"))?;
    tx.commit().map_err(|e| format!("Enregistrement de la note impossible : {e}"))
}

/// Remplace les formes, l'orientation, le recadrage et le fond d'une capture. `quand`
/// change, et avec lui l'adresse des images dans la page, qui les recharge.
pub fn remplacer_formes(
    conn: &Connection,
    id: i64,
    formes: &str,
    orientation: u8,
    recadrage: Option<Recadrage>,
    fond: Option<&Fond>,
    quand: &chrono::DateTime<chrono::Local>,
) -> Result<(), String> {
    let recadrage = recadrage.map(|r| serde_json::json!(r).to_string());
    let fond = fond.map(|f| serde_json::json!(f).to_string());
    conn.execute(
        "UPDATE captures SET formes = ?1, orientation = ?2, recadrage = ?3, fond = ?4, modifie_le = ?5 WHERE id = ?6",
        params![formes, orientation, recadrage, fond, quand.format("%Y-%m-%dT%H:%M:%S%.3f").to_string(), id],
    )
    .map_err(|e| format!("Enregistrement des formes impossible : {e}"))?;
    Ok(())
}

// La suppression se fait en trois temps, pour que les fichiers ne bougent
// jamais sous le verrou de la base (un disque qui cale figerait la fenetre,
// dont les commandes synchrones attendent ce verrou) : `fichier_de`, puis
// `vers_corbeille` hors verrou, puis `retirer_ligne`.

/// Le chemin relatif de l'original d'une capture.
pub fn fichier_de(conn: &Connection, id: i64) -> Result<String, String> {
    conn.query_row("SELECT fichier FROM captures WHERE id = ?1", [id], |ligne| ligne.get(0))
        .map_err(|e| format!("Capture {id} introuvable : {e}"))
}

/// Pousse les trois fichiers d'une capture dans la corbeille du dossier des
/// captures. Rien n'est efface du disque.
pub fn vers_corbeille(dossier: &Path, fichier: &str) -> Result<(), String> {
    let corbeille = dossier.join(CORBEILLE);
    std::fs::create_dir_all(&corbeille).map_err(|e| format!("Création de {} impossible : {e}", corbeille.display()))?;
    let c = chemins(dossier, fichier);
    // Le GIF d'une video (ADR-016) suit son MP4.
    let gif = c.origine.with_extension("gif");
    for chemin in [c.origine, c.rendu, c.vignette, gif] {
        let Some(nom) = chemin.file_name() else { continue };
        // Un fichier deja absent n'empeche pas de retirer la carte.
        if chemin.exists() {
            std::fs::rename(&chemin, corbeille.join(nom))
                .map_err(|e| format!("Déplacement de {} vers la corbeille impossible : {e}", chemin.display()))?;
        }
    }
    Ok(())
}

/// Retire la capture de la galerie et de l'index de recherche.
pub fn retirer_ligne(conn: &Connection, id: i64) -> Result<(), String> {
    let tx = conn.unchecked_transaction().map_err(|e| format!("Transaction impossible : {e}"))?;
    tx.execute("DELETE FROM captures_fts WHERE rowid = ?1", [id])
        .and_then(|_| tx.execute("DELETE FROM captures WHERE id = ?1", [id]))
        .map_err(|e| format!("Suppression de la capture impossible : {e}"))?;
    tx.commit().map_err(|e| format!("Suppression de la capture impossible : {e}"))
}

/// Efface les fichiers d'une capture dont la ligne n'a pas pu s'ecrire : sans
/// ligne, rien ne les montrerait ni ne les nettoierait.
pub fn effacer_fichiers(dossier: &Path, fichier: &str) {
    let c = chemins(dossier, fichier);
    for chemin in [c.origine, c.rendu, c.vignette] {
        let _ = std::fs::remove_file(chemin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn zone(x: f64, y: f64, largeur: f64, hauteur: f64) -> ZoneCss {
        ZoneCss { x, y, largeur, hauteur }
    }

    fn derniere(x: u32, y: u32, largeur: u32, hauteur: u32) -> DerniereZone {
        DerniereZone { x, y, largeur, hauteur, position: (-2880, 120), taille: (2880, 1620), ecran: r"\\.\DISPLAY2".into(), echelle: 1.5 }
    }

    #[test]
    fn captures_derniere_zone_relue_telle_qu_ecrite() {
        let gardee = derniere(150, 60, 450, 300);
        let json = serde_json::to_string(&gardee).expect("json");
        assert_eq!(serde_json::from_str::<DerniereZone>(&json).expect("relue"), gardee);
        assert!(serde_json::from_str::<DerniereZone>("").is_err(), "rien de gardé");
        assert!(serde_json::from_str::<DerniereZone>(r#"{"x":1}"#).is_err(), "une zone incomplète n'est pas une zone");
    }

    #[test]
    fn captures_derniere_zone_seulement_sur_le_meme_ecran() {
        let principal = ((0, 0), (1920, 1080));
        let gauche = ((-2880, 120), (2880, 1620));
        let gardee = derniere(150, 60, 450, 300);
        assert_eq!(ecran_de_la_zone(&[principal, gauche], &gardee), Some(1), "le même écran, quel que soit son rang");
        assert_eq!(ecran_de_la_zone(&[principal], &gardee), None, "écran débranché");
        assert_eq!(ecran_de_la_zone(&[principal, ((-2880, 120), (1920, 1080))], &gardee), None, "résolution changée");
        assert_eq!(ecran_de_la_zone(&[principal, ((-1920, 0), (2880, 1620))], &gardee), None, "écran déplacé");
        // Une base écrite à la main ne fait pas rogner hors de l'écran.
        assert_eq!(ecran_de_la_zone(&[gauche], &derniere(2500, 60, 450, 300)), None, "zone qui déborde");
        assert_eq!(ecran_de_la_zone(&[gauche], &derniere(150, 60, 0, 300)), None, "zone vide");
        assert_eq!(ecran_de_la_zone(&[gauche], &derniere(2430, 1320, 450, 300)), Some(0), "zone au ras du coin");
    }

    #[test]
    fn captures_zone_css_vers_physique_a_150_pour_cent_ecran_a_gauche() {
        // Ecran 2880 x 1620 a 150 %, place a gauche du principal : origine negative.
        let z = zone_physique(zone(100.0, 40.0, 300.0, 200.0), 1.5, (-2880, 120), (2880, 1620)).expect("zone");
        assert_eq!((z.x, z.y, z.largeur, z.hauteur), (150, 60, 450, 300));
        assert_eq!(z.bureau, (-2730, 180), "le coin sur le bureau part de l'origine de l'ecran");
    }

    #[test]
    fn captures_zone_arrondit_les_bords_pas_la_largeur() {
        // 10,4 a 20,4 CSS a 125 % : bords a 13 et 25,5 -> 13 et 26, largeur 13.
        let z = zone_physique(zone(10.4, 0.0, 10.0, 8.0), 1.25, (0, 0), (1000, 1000)).expect("zone");
        assert_eq!((z.x, z.largeur, z.hauteur), (13, 13, 10));
    }

    #[test]
    fn captures_zone_bornee_a_l_image_et_vide_refusee() {
        let z = zone_physique(zone(900.0, 500.0, 400.0, 400.0), 1.0, (0, 0), (1000, 600)).expect("zone");
        assert_eq!((z.x, z.y, z.largeur, z.hauteur), (900, 500, 100, 100));
        assert_eq!(zone_physique(zone(10.0, 10.0, 0.2, 50.0), 1.0, (0, 0), (1000, 600)), None);
        assert_eq!(zone_physique(zone(10.0, 10.0, 50.0, 50.0), 0.0, (0, 0), (1000, 600)), None);
        assert_eq!(zone_physique(zone(f64::NAN, 10.0, 50.0, 50.0), 1.0, (0, 0), (1000, 600)), None);
    }

    #[test]
    fn captures_fenetre_rognee_a_l_ecran_de_la_selection() {
        // Ecran secondaire a gauche du principal, 1920 x 1080 a (-1920, 0).
        let ecran = ((-1920, 0), (1920, 1080));
        let z = rogner_a_l_ecran((-1000, 100, 400, 300), ecran.0, ecran.1).expect("dedans");
        assert_eq!((z.x, z.y, z.largeur, z.hauteur, z.bureau), (920, 100, 400, 300, (-1000, 100)));
        // Agrandie, la fenetre deborde de 8 pixels de chaque cote : rognee a l'ecran.
        let z = rogner_a_l_ecran((-1928, -8, 1936, 1096), ecran.0, ecran.1).expect("agrandie");
        assert_eq!((z.x, z.y, z.largeur, z.hauteur, z.bureau), (0, 0, 1920, 1080, (-1920, 0)));
        // A cheval sur le principal : seule la part de cet ecran reste.
        let z = rogner_a_l_ecran((-200, 50, 500, 100), ecran.0, ecran.1).expect("a cheval");
        assert_eq!((z.x, z.largeur), (1720, 200));
        assert_eq!(rogner_a_l_ecran((10, 10, 300, 300), ecran.0, ecran.1), None, "sur l'autre ecran");
        assert_eq!(rogner_a_l_ecran((-500, 10, 0, 300), ecran.0, ecran.1), None, "vide");
    }

    #[test]
    fn captures_loupe_centree_puis_tronquee_aux_bords() {
        let (position, taille) = ((100, 50), (1000, 800));
        let (z, decalage) = cadre_loupe((600, 450), position, taille).expect("milieu");
        assert_eq!((z.x, z.y, z.largeur, z.hauteur, decalage), (493, 393, 15, 15, (0, 0)));
        // Au coin haut gauche de l'ecran : la loupe ne lit que 8 x 8, place en bas a droite.
        let (z, decalage) = cadre_loupe((100, 50), position, taille).expect("coin");
        assert_eq!((z.x, z.y, z.largeur, z.hauteur, decalage), (0, 0, 8, 8, (7, 7)));
        // Dernier pixel de l'ecran : 8 lus a gauche et au-dessus, rien apres.
        let (z, decalage) = cadre_loupe((1099, 849), position, taille).expect("dernier");
        assert_eq!((z.x, z.y, z.largeur, z.hauteur, decalage), (992, 792, 8, 8, (0, 0)));
        assert_eq!(cadre_loupe((1100, 450), position, taille), None, "hors de l'ecran");
    }

    #[test]
    fn captures_loupe_assemblee_le_pixel_du_curseur_au_centre() {
        // Region lue au coin haut gauche : 8 x 8, le curseur est son premier pixel,
        // place a (7, 7) dans le carre.
        let mut lue = RgbaImage::from_pixel(8, 8, image::Rgba([10, 10, 10, 255]));
        lue.put_pixel(0, 0, image::Rgba([200, 0, 0, 255]));
        let octets = assembler_loupe(&lue, (7, 7), (0, 0));
        assert_eq!(octets.len(), 8 + (COTE_LOUPE * COTE_LOUPE * 4) as usize);
        assert_eq!(&octets[..8], &[0, 0, 0, 0, 0, 0, 0, 0], "coordonnees du curseur dans l'ecran");
        let pixel = |x: u32, y: u32| {
            let i = 8 + ((y * COTE_LOUPE + x) * 4) as usize;
            [octets[i], octets[i + 1], octets[i + 2], octets[i + 3]]
        };
        assert_eq!(pixel(7, 7), [200, 0, 0, 255], "le pixel du curseur est au centre");
        assert_eq!(pixel(8, 8), [10, 10, 10, 255]);
        assert_eq!(pixel(14, 14), [10, 10, 10, 255], "le dernier pixel lu au coin du carre");
        assert_eq!(pixel(6, 6)[3], 0, "hors de l'ecran : transparent");
        assert_eq!(pixel(0, 0)[3], 0);
        let octets = assembler_loupe(&lue, (0, 0), (-3, 1200));
        assert_eq!(&octets[..8], &[253, 255, 255, 255, 176, 4, 0, 0], "x et y en i32 petit-boutiste");
    }

    #[test]
    fn captures_curseur_borne_a_l_ecran() {
        let (position, taille) = ((-1920, 0), (1920, 1080));
        assert_eq!(borner_a_l_ecran((-1000, 500), position, taille), (-1000, 500));
        assert_eq!(borner_a_l_ecran((-1925, -3), position, taille), (-1920, 0));
        assert_eq!(borner_a_l_ecran((0, 1080), position, taille), (-1, 1079), "le dernier pixel, pas le suivant");
    }

    fn quand(seconde: u32) -> chrono::DateTime<chrono::Local> {
        chrono::Local.with_ymd_and_hms(2026, 10, 2, 3, 15, seconde).single().expect("date")
    }

    fn origine() -> Origine {
        Origine { ecran: "DISPLAY1".into(), echelle: 1.0, fenetre: "Devis Dupont - Bloc-notes".into(), application: "Notepad".into() }
    }

    fn poser(conn: &Connection, dossier: &Path, seconde: u32) -> Capture {
        let q = quand(seconde);
        let id = inserer(conn, &q, &nom_fichier(&q), (40, 30), &origine(), "[]", None).expect("insertion");
        lire(conn, dossier, id).expect("relue")
    }

    #[test]
    fn stockage_texte_lu_puis_cherche() {
        let conn = crate::db::open_in_memory().expect("base");
        let dossier = Path::new("D:/captures");
        let a = inserer(&conn, &quand(1), "a.png", (10, 10), &origine(), "[]", None).expect("a");
        let b = inserer(&conn, &quand(2), "b.png", (10, 10), &origine(), "[]", Some("déjà lu")).expect("b");
        assert_eq!(prochaine_a_lire(&conn).expect("lue"), Some((a, "a.png".to_string())), "b a déjà son texte");
        assert_eq!(lire(&conn, dossier, a).expect("a relue").texte, None, "a est à lire");
        poser_texte(&conn, a, "Erreur 404 introuvable").expect("posé");
        assert_eq!(prochaine_a_lire(&conn).expect("lue"), None);
        assert_eq!(texte_de(&conn, a).expect("lu").as_deref(), Some("Erreur 404 introuvable"));
        let trouvees: Vec<i64> = lister(&conn, dossier, "introuv").expect("cherchée").iter().map(|c| c.id).collect();
        assert_eq!(trouvees, vec![a]);
        let deja: Vec<i64> = lister(&conn, dossier, "deja").expect("cherchée").iter().map(|c| c.id).collect();
        assert_eq!(deja, vec![b], "le texte posé à l'insertion est cherchable");
        poser_texte(&conn, 9_999, "rien").expect("capture supprimée entre-temps : sans effet ni erreur");
        assert_eq!(lire(&conn, dossier, b).expect("b relue").texte.as_deref(), Some("déjà lu"));
    }

    #[test]
    fn captures_chemins_et_nom_de_fichier() {
        let q = quand(7);
        assert_eq!(nom_fichier(&q), "2026-10/20261002-031507-000.png");
        let c = chemins(Path::new("D:/x"), &nom_fichier(&q));
        assert!(c.rendu.ends_with("2026-10/20261002-031507-000.rendu.png"));
        assert!(c.vignette.ends_with("2026-10/20261002-031507-000.vignette.png"));
    }

    #[test]
    fn captures_galerie_recente_d_abord_et_recherche() {
        let conn = crate::db::open_in_memory().expect("base");
        let dossier = Path::new("D:/captures");
        let premiere = poser(&conn, dossier, 1);
        let seconde = poser(&conn, dossier, 2);
        noter(&conn, seconde.id, "facture à relancer zorglub").expect("note");

        let toutes = lister(&conn, dossier, "").expect("liste");
        assert_eq!(toutes.iter().map(|c| c.id).collect::<Vec<_>>(), vec![seconde.id, premiere.id]);

        let par_note = lister(&conn, dossier, "zorglub").expect("recherche");
        assert_eq!(par_note.iter().map(|c| c.id).collect::<Vec<_>>(), vec![seconde.id], "la note rend sa seule capture");
        assert_eq!(par_note[0].note, "facture à relancer zorglub");
        assert!(lister(&conn, dossier, "introuvable").expect("recherche").is_empty());
        assert_eq!(lister(&conn, dossier, "dupont").expect("fenetre").len(), 2, "le titre de fenêtre se cherche");
        assert_eq!(lister(&conn, dossier, "notepad").expect("application").len(), 2);
        assert_eq!(lister(&conn, dossier, "2026-10-02").expect("jour").len(), 2, "la date se cherche");
        assert_eq!(lister(&conn, dossier, "02/10/2026").expect("jour").len(), 2, "comme on l'écrit en France");
        assert_eq!(lister(&conn, dossier, "octobre 2026").expect("mois").len(), 2, "le mois en toutes lettres");
        assert!(lister(&conn, dossier, "novembre").expect("mois").is_empty());
        assert!(lister(&conn, dossier, "03/10/2026").expect("jour").is_empty());
        assert_eq!(lister(&conn, dossier, "a relancer").expect("accents").len(), 1, "sans accent aussi");
        // Une saisie qui serait une faute de syntaxe FTS5 ne fait pas tomber la recherche.
        assert!(lister(&conn, dossier, "\"(").is_ok());
    }

    #[test]
    fn captures_note_d_une_capture_absente_refusee() {
        let conn = crate::db::open_in_memory().expect("base");
        assert!(noter(&conn, 99, "x").is_err());
    }

    #[test]
    fn captures_formes_une_liste_seulement() {
        let fleche = serde_json::json!([{"type": "fleche", "x1": 1, "y1": 2, "x2": 30.5, "y2": 40}]);
        assert_eq!(valider_formes(&fleche).expect("liste"), r#"[{"type":"fleche","x1":1,"x2":30.5,"y1":2,"y2":40}]"#);
        assert!(valider_formes(&serde_json::json!({"type": "fleche"})).is_err());
    }

    #[test]
    fn captures_formes_les_douze_types_avec_leur_style() {
        let formes = serde_json::json!([
            {"type": "fleche", "x1": 0, "y1": 0, "x2": 9, "y2": 9},
            {"type": "ligne", "x1": 0, "y1": 0, "x2": 9, "y2": 9, "couleur": "#007aff", "taille": 3},
            {"type": "surligneur", "x1": 0, "y1": 0, "x2": 9, "y2": 0, "couleur": "#FFCC00", "taille": 1},
            {"type": "rectangle", "x": 1, "y": 1, "largeur": 5, "hauteur": 5},
            {"type": "ellipse", "x": 1, "y": 1, "largeur": 5, "hauteur": 5, "taille": 2},
            {"type": "pixellisation", "x": 1, "y": 1, "largeur": 50, "hauteur": 20},
            {"type": "texte", "x": 3, "y": 4, "texte": "Ici"},
            {"type": "etape", "x": 3, "y": 4, "numero": 12, "couleur": "#1c1c1e"},
            {"type": "crayon", "points": [0, 0, 1.5, 2, 3, 4]},
            {"type": "projecteur", "x": 10, "y": 10, "largeur": 80, "hauteur": 40},
            {"type": "loupe", "x": 10, "y": 10, "largeur": 80, "hauteur": 40, "couleur": "#ffffff", "taille": 1},
            {"type": "tampon", "x": 10, "y": 10, "symbole": "⚠️", "taille": 3},
        ]);
        assert!(valider_formes(&formes).is_ok(), "{:?}", valider_formes(&formes));
        assert!(valider_formes(&serde_json::json!([{"type": "projecteur", "x": 1, "y": 1}])).is_err(), "un projecteur sans taille est refusé");
        assert!(valider_formes(&serde_json::json!([{"type": "loupe", "x": 1, "y": 1}])).is_err(), "une loupe sans taille est refusée");
        // La pipette (ADR-013) copie seulement ce que `est_couleur` accepte.
        assert!(est_couleur("#a0B1c2") && !est_couleur("#a0b1g2") && !est_couleur("a0b1c2") && !est_couleur("#a0b1c"));
        assert!(valider_formes(&serde_json::json!([])).is_ok());
    }

    #[test]
    fn captures_formes_mal_faites_refusees() {
        let mal = [
            serde_json::json!([{"type": "etoile", "x": 1, "y": 1}]),
            serde_json::json!([{"x1": 0, "y1": 0, "x2": 9, "y2": 9}]),
            serde_json::json!(["fleche"]),
            serde_json::json!([{"type": "fleche", "x1": 0, "y1": 0, "x2": 9}]),
            serde_json::json!([{"type": "fleche", "x1": "0", "y1": 0, "x2": 9, "y2": 9}]),
            serde_json::json!([{"type": "rectangle", "x": 1, "y": 1, "largeur": 5, "hauteur": 5, "couleur": "rouge"}]),
            serde_json::json!([{"type": "rectangle", "x": 1, "y": 1, "largeur": 5, "hauteur": 5, "couleur": "#ff3b3"}]),
            serde_json::json!([{"type": "rectangle", "x": 1, "y": 1, "largeur": 5, "hauteur": 5, "taille": 4}]),
            serde_json::json!([{"type": "rectangle", "x": 1, "y": 1, "largeur": 5, "hauteur": 5, "taille": 1.5}]),
            serde_json::json!([{"type": "texte", "x": 1, "y": 1, "texte": ""}]),
            serde_json::json!([{"type": "texte", "x": 1, "y": 1, "texte": "a".repeat(501)}]),
            serde_json::json!([{"type": "etape", "x": 1, "y": 1, "numero": 0}]),
            serde_json::json!([{"type": "etape", "x": 1, "y": 1, "numero": 2.5}]),
            serde_json::json!([{"type": "crayon", "points": [0, 0, 1]}]),
            serde_json::json!([{"type": "crayon", "points": []}]),
            serde_json::json!([{"type": "crayon", "points": [0, "1"]}]),
            serde_json::json!([{"type": "crayon", "points": vec![1.0; 2 * POINTS_MAX + 2]}]),
            serde_json::json!([{"type": "tampon", "x": 1, "y": 1, "symbole": ""}]),
            serde_json::json!([{"type": "tampon", "x": 1, "y": 1, "symbole": "x".repeat(17)}]),
            serde_json::json!([{"type": "tampon", "x": 1, "y": 1, "symbole": "a\nb"}]),
            serde_json::json!([{"type": "tampon", "x": 1, "y": 1}]),
            serde_json::json!([{"type": "tampon", "y": 1, "symbole": "✅"}]),
        ];
        for formes in mal {
            assert!(valider_formes(&formes).is_err(), "acceptée à tort : {formes}");
        }
        // Une seule forme fausse fait refuser toute la liste.
        let melange = serde_json::json!([{"type": "rectangle", "x": 1, "y": 1, "largeur": 5, "hauteur": 5}, {"type": "etoile"}]);
        assert!(valider_formes(&melange).is_err());
    }

    #[test]
    fn captures_recadrage_dans_l_image_entier_et_non_vide() {
        let r = |x: u32, y: u32, largeur: u32, hauteur: u32| Recadrage { x, y, largeur, hauteur };
        let valide = |v: serde_json::Value| valider_recadrage(&v, (40, 30));
        assert_eq!(valide(serde_json::Value::Null), Ok(None));
        assert_eq!(valide(serde_json::json!({"x": 10, "y": 5, "largeur": 20, "hauteur": 10})), Ok(Some(r(10, 5, 20, 10))));
        assert_eq!(valide(serde_json::json!({"x": 0, "y": 0, "largeur": 40, "hauteur": 30})), Ok(None), "l'image entière ne recadre rien");
        assert_eq!(valide(serde_json::json!({"x": 39, "y": 29, "largeur": 1, "hauteur": 1})), Ok(Some(r(39, 29, 1, 1))), "le dernier pixel");
        let mal = [
            serde_json::json!({"x": 30, "y": 0, "largeur": 11, "hauteur": 10}),
            serde_json::json!({"x": 0, "y": 25, "largeur": 10, "hauteur": 6}),
            serde_json::json!({"x": 0, "y": 0, "largeur": 0, "hauteur": 10}),
            serde_json::json!({"x": -1, "y": 0, "largeur": 10, "hauteur": 10}),
            serde_json::json!({"x": 1.5, "y": 0, "largeur": 10, "hauteur": 10}),
            serde_json::json!({"x": 0, "y": 0, "largeur": 10}),
            serde_json::json!([0, 0, 10, 10]),
            serde_json::json!({"x": 4294967295u32, "y": 0, "largeur": 10, "hauteur": 10}),
        ];
        for v in mal {
            assert!(valide(v.clone()).is_err(), "accepté à tort : {v}");
        }
    }

    #[test]
    fn captures_recadrer_au_pixel_pres() {
        let image = RgbaImage::from_fn(40, 30, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        let rogne = recadrer(&image, Some(Recadrage { x: 10, y: 5, largeur: 20, hauteur: 10 }));
        assert_eq!(rogne.dimensions(), (20, 10));
        assert_eq!(rogne.get_pixel(0, 0).0, [10, 5, 7, 255], "le coin du rendu est le coin du cadre");
        assert_eq!(rogne.get_pixel(19, 9).0, [29, 14, 7, 255]);
        assert_eq!(recadrer(&image, None), image, "sans recadrage, l'image entière");
    }

    #[test]
    fn captures_recadrage_garde_et_relu() {
        let conn = crate::db::open_in_memory().expect("base");
        let dossier = Path::new("D:/captures");
        let capture = poser(&conn, dossier, 3);
        assert_eq!(capture.recadrage, None, "une capture neuve n'est pas recadrée");
        let r = Recadrage { x: 2, y: 3, largeur: 10, hauteur: 8 };
        remplacer_formes(&conn, capture.id, "[]", 0, Some(r), None, &quand(4)).expect("recadrée");
        assert_eq!(lire(&conn, dossier, capture.id).expect("relue").recadrage, Some(r));
        assert_eq!(serde_json::to_value(lire(&conn, dossier, capture.id).expect("relue")).expect("json")["recadrage"], serde_json::json!({"x": 2, "y": 3, "largeur": 10, "hauteur": 8}));
        remplacer_formes(&conn, capture.id, "[]", 0, None, None, &quand(5)).expect("desserrée");
        assert_eq!(lire(&conn, dossier, capture.id).expect("relue").recadrage, None);
    }

    fn fond(couleurs: [&str; 2], marge: u32, arrondi: u32, ombre: bool) -> Fond {
        Fond { couleurs: couleurs.map(String::from), marge, arrondi, ombre }
    }

    #[test]
    fn fond_valide_ou_refuse() {
        assert_eq!(valider_fond(&serde_json::Value::Null), Ok(None));
        let bon = serde_json::json!({"couleurs": ["#112233", "#AABBCC"], "marge": 25, "arrondi": 10, "ombre": true});
        assert_eq!(valider_fond(&bon), Ok(Some(fond(["#112233", "#AABBCC"], 25, 10, true))));
        let mal = [
            serde_json::json!({"couleurs": ["#112233", "red"], "marge": 8, "arrondi": 2, "ombre": true}),
            serde_json::json!({"couleurs": ["#112233"], "marge": 8, "arrondi": 2, "ombre": true}),
            serde_json::json!({"couleurs": ["#112233", "#112233"], "marge": 26, "arrondi": 2, "ombre": true}),
            serde_json::json!({"couleurs": ["#112233", "#112233"], "marge": 8, "arrondi": 11, "ombre": true}),
            serde_json::json!({"couleurs": ["#112233", "#112233"], "marge": -1, "arrondi": 2, "ombre": true}),
            serde_json::json!({"couleurs": ["#112233", "#112233"], "marge": 8, "arrondi": 2}),
            serde_json::json!({"couleurs": ["#112233", "#112233"], "marge": 8, "arrondi": 2, "ombre": true, "image": "c:/x.png"}),
            serde_json::json!("bleu"),
        ];
        for v in mal {
            assert!(valider_fond(&v).is_err(), "accepté à tort : {v}");
        }
    }

    #[test]
    fn fond_marges_degrade_et_coins_arrondis() {
        let image = RgbaImage::from_pixel(100, 50, image::Rgba([200, 0, 0, 255]));
        let habillee = habiller(&image, Some(&fond(["#000000", "#ffffff"], 10, 10, false)));
        // Marge : 10 % de 100 = 10 px de chaque côté.
        assert_eq!(habillee.dimensions(), (120, 70));
        assert_eq!(habillee.get_pixel(0, 0).0, [0, 0, 0, 255], "le dégradé part de la première couleur");
        assert_eq!(habillee.get_pixel(119, 69).0, [255, 255, 255, 255], "et finit sur la seconde");
        assert_eq!(habillee.get_pixel(60, 35).0, [200, 0, 0, 255], "l'image au centre");
        assert_eq!(habillee.get_pixel(60, 10).0, [200, 0, 0, 255], "le bord haut de l'image, hors des coins");
        // Rayon : 10 % de 50 = 5 px ; le coin de l'image laisse voir le fond.
        assert_ne!(habillee.get_pixel(10, 10).0, [200, 0, 0, 255], "le coin de l'image est arrondi");
    }

    #[test]
    fn fond_ombre_sous_l_image() {
        let image = RgbaImage::from_pixel(100, 100, image::Rgba([200, 0, 0, 255]));
        let sans = habiller(&image, Some(&fond(["#808080", "#808080"], 20, 0, false)));
        let avec = habiller(&image, Some(&fond(["#808080", "#808080"], 20, 0, true)));
        // L'image va de 20 à 120 ; l'ombre, décalée de 5 px vers le bas, déborde dessous.
        assert_eq!(sans.get_pixel(70, 125).0, [128, 128, 128, 255]);
        let (dessous, dessus) = (avec.get_pixel(70, 125).0[0], avec.get_pixel(70, 15).0[0]);
        assert!(dessous < dessus, "plus sombre sous l'image ({dessous}) qu'au-dessus ({dessus})");
        assert!(dessous < 110, "une ombre visible : {dessous}");
        assert_eq!(avec.get_pixel(70, 70).0, [200, 0, 0, 255], "l'ombre reste sous l'image");
    }

    #[test]
    fn fond_nul_ou_absent_laisse_l_image_intacte() {
        let image = RgbaImage::from_fn(40, 30, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        assert_eq!(habiller(&image, None), image);
        assert_eq!(habiller(&image, Some(&fond(["#123456", "#654321"], 0, 0, false))), image);
    }

    #[test]
    fn fond_taille_du_rendu_cadre_plus_deux_marges() {
        let mut capture = Capture { largeur: 200, hauteur: 100, ..Default::default() };
        assert_eq!(capture.taille_rendu(), (200, 100));
        capture.recadrage = Some(Recadrage { x: 0, y: 0, largeur: 50, hauteur: 40 });
        assert_eq!(capture.taille_rendu(), (50, 40));
        capture.fond = Some(fond(["#000000", "#000000"], 10, 0, false));
        assert_eq!(capture.taille_rendu(), (60, 50), "5 px de marge : 10 % du plus grand côté du cadre");
        let image = RgbaImage::new(50, 40);
        assert_eq!(habiller(&image, capture.fond.as_ref()).dimensions(), capture.taille_rendu(), "une seule règle");
    }

    #[test]
    fn orientation_valide_ou_refusee() {
        for v in 0..=7u8 {
            assert_eq!(valider_orientation(&serde_json::json!(v)), Ok(v));
        }
        let mal = [serde_json::json!(8), serde_json::json!(-1), serde_json::json!(2.5), serde_json::json!("1"), serde_json::Value::Null];
        for v in mal {
            assert_eq!(valider_orientation(&v), Err("L'orientation va de 0 à 7.".to_string()), "accepté à tort : {v}");
        }
    }

    /// Une image 3 x 2 dont chaque pixel porte son numéro dans le rouge.
    fn etiquetee() -> RgbaImage {
        RgbaImage::from_fn(3, 2, |x, y| image::Rgba([(y * 3 + x) as u8, 0, 0, 255]))
    }

    fn etiquettes(img: &RgbaImage) -> Vec<Vec<u8>> {
        (0..img.height()).map(|y| (0..img.width()).map(|x| img.get_pixel(x, y).0[0]).collect()).collect()
    }

    #[test]
    fn orientation_huit_etats_pixel_par_pixel() {
        let img = etiquetee();
        // La même grille que le test Node : R^q(M^m(original)), miroir d'abord.
        let attendus: [Vec<Vec<u8>>; 8] = [
            vec![vec![0, 1, 2], vec![3, 4, 5]],
            vec![vec![3, 0], vec![4, 1], vec![5, 2]],
            vec![vec![5, 4, 3], vec![2, 1, 0]],
            vec![vec![2, 5], vec![1, 4], vec![0, 3]],
            vec![vec![2, 1, 0], vec![5, 4, 3]],
            vec![vec![5, 2], vec![4, 1], vec![3, 0]],
            vec![vec![3, 4, 5], vec![0, 1, 2]],
            vec![vec![0, 3], vec![1, 4], vec![2, 5]],
        ];
        for (o, attendu) in attendus.iter().enumerate() {
            assert_eq!(&etiquettes(&orienter(&img, o as u8)), attendu, "orientation {o}");
        }
        assert_eq!(orienter(&img, 0), img, "l'orientation 0 rend l'entrée");
        assert_eq!(orienter(&orienter(&img, 1), 3), img, "un quart puis trois quarts : l'entrée");
        // Mutant « rotate90 devenu rotate270 » : l'état 1 donnerait la grille de l'état 3.
    }

    #[test]
    fn orientation_taille_orientee_et_taille_du_rendu() {
        assert_eq!(taille_orientee((200, 100), 0), (200, 100));
        assert_eq!(taille_orientee((200, 100), 1), (100, 200));
        assert_eq!(taille_orientee((200, 100), 2), (200, 100));
        assert_eq!(taille_orientee((200, 100), 7), (100, 200));
        let mut capture = Capture { largeur: 200, hauteur: 100, orientation: 1, ..Default::default() };
        assert_eq!(capture.taille_rendu(), (100, 200), "un pivot échange les côtés");
        capture.fond = Some(fond(["#000000", "#000000"], 10, 0, false));
        assert_eq!(capture.taille_rendu(), (140, 240), "20 px de marge : 10 % du plus grand côté orienté");
        capture.recadrage = Some(Recadrage { x: 0, y: 0, largeur: 50, hauteur: 40 });
        assert_eq!(capture.taille_rendu(), (60, 50), "le cadre, déjà dans l'espace orienté, ne change pas");
    }

    #[test]
    fn orientation_recadrage_entier_oriente_vaut_aucun() {
        let taille = taille_orientee((200, 100), 1);
        assert_eq!(valider_recadrage(&serde_json::json!({"x": 0, "y": 0, "largeur": 100, "hauteur": 200}), taille), Ok(None), "l'image entière orientée : aucun recadrage");
        assert!(valider_recadrage(&serde_json::json!({"x": 0, "y": 0, "largeur": 200, "hauteur": 100}), taille).is_err(), "le cadre de l'original non orienté sort de l'image orientée");
    }

    #[test]
    fn orientation_gardee_relue_et_bornee() {
        let conn = crate::db::open_in_memory().expect("base");
        let dossier = Path::new("D:/captures");
        let capture = poser(&conn, dossier, 3);
        assert_eq!(capture.orientation, 0, "une capture neuve est telle que prise");
        remplacer_formes(&conn, capture.id, "[]", 5, None, None, &quand(4)).expect("orientée");
        assert_eq!(lire(&conn, dossier, capture.id).expect("relue").orientation, 5);
        assert_eq!(serde_json::to_value(lire(&conn, dossier, capture.id).expect("relue")).expect("json")["orientation"], serde_json::json!(5));
        conn.execute("UPDATE captures SET orientation = 9 WHERE id = ?1", [capture.id]).expect("posée à la main");
        assert_eq!(lire(&conn, dossier, capture.id).expect("relue").orientation, 0, "hors de 0 à 7 : 0, jamais un plantage");
    }

    #[test]
    fn fond_garde_et_relu() {
        let conn = crate::db::open_in_memory().expect("base");
        let dossier = Path::new("D:/captures");
        let capture = poser(&conn, dossier, 3);
        assert_eq!(capture.fond, None, "une capture neuve n'a pas de fond");
        let f = fond(["#112233", "#445566"], 8, 2, true);
        remplacer_formes(&conn, capture.id, "[]", 0, None, Some(&f), &quand(4)).expect("habillée");
        assert_eq!(lire(&conn, dossier, capture.id).expect("relue").fond, Some(f));
        remplacer_formes(&conn, capture.id, "[]", 0, None, None, &quand(5)).expect("sans fond");
        assert_eq!(lire(&conn, dossier, capture.id).expect("relue").fond, None);
    }

    #[test]
    fn captures_fichiers_ecrits_puis_pousses_a_la_corbeille() {
        let dossier = std::env::temp_dir().join(format!("equishot-captures-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        let conn = crate::db::open_in_memory().expect("base");
        let q = quand(9);
        let fichier = nom_fichier(&q);
        let image = RgbaImage::from_pixel(800, 400, image::Rgba([10, 200, 30, 255]));
        ecrire_origine(&dossier, &fichier, &image).expect("origine");
        assert!(ecrire_origine(&dossier, &fichier, &image).is_err(), "l'original ne s'écrase pas");
        ecrire_rendu(&dossier, &fichier, &image).expect("rendu");
        let id = inserer(&conn, &q, &fichier, image.dimensions(), &origine(), "[]", None).expect("ligne");
        let capture = lire(&conn, &dossier, id).expect("relue");

        let c = chemins(&dossier, &fichier);
        assert_eq!(lire_png(&c.origine).expect("relu").dimensions(), (800, 400));
        assert_eq!(lire_png(&c.vignette).expect("vignette").dimensions(), (360, 180), "vignette bornée à 360");

        assert_eq!(fichier_de(&conn, capture.id).expect("fichier"), fichier);
        vers_corbeille(&dossier, &fichier).expect("corbeille");
        retirer_ligne(&conn, capture.id).expect("suppression");
        assert!(fichier_de(&conn, capture.id).is_err());
        assert!(lister(&conn, &dossier, "").expect("liste").is_empty());
        assert!(lister(&conn, &dossier, "dupont").expect("recherche").is_empty(), "l'index est vidé aussi");
        assert!(!c.origine.exists() && !c.rendu.exists() && !c.vignette.exists());
        let corbeille = dossier.join(CORBEILLE);
        for nom in ["20261002-031509-000.png", "20261002-031509-000.rendu.png", "20261002-031509-000.vignette.png"] {
            assert!(corbeille.join(nom).exists(), "{nom} est dans la corbeille");
        }
        let _ = std::fs::remove_dir_all(&dossier);
    }

    #[test]
    fn captures_fichiers_sans_ligne_effaces() {
        let dossier = std::env::temp_dir().join(format!("equishot-captures-orphelins-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        let fichier = nom_fichier(&quand(11));
        let image = RgbaImage::from_pixel(20, 10, image::Rgba([1, 2, 3, 255]));
        ecrire_origine(&dossier, &fichier, &image).expect("origine");
        ecrire_rendu(&dossier, &fichier, &image).expect("rendu");
        let c = chemins(&dossier, &fichier);
        assert!(c.origine.exists() && c.rendu.exists() && c.vignette.exists());
        effacer_fichiers(&dossier, &fichier);
        assert!(!c.origine.exists() && !c.rendu.exists() && !c.vignette.exists());
        let _ = std::fs::remove_dir_all(&dossier);
    }

    #[test]
    fn video_chemins_d_un_absolu() {
        // Une video garde son chemin absolu : le dossier des captures n'y change rien.
        let c = chemins(Path::new("D:/captures"), r"C:\Users\x\Videos\EquiShot\2026-10\20261007-101500-000.mp4");
        assert_eq!(c.origine, PathBuf::from(r"C:\Users\x\Videos\EquiShot\2026-10\20261007-101500-000.mp4"));
        assert_eq!(c.vignette, PathBuf::from(r"C:\Users\x\Videos\EquiShot\2026-10\20261007-101500-000.vignette.png"));
        assert_eq!(c.rendu, PathBuf::from(r"C:\Users\x\Videos\EquiShot\2026-10\20261007-101500-000.rendu.png"));
    }

    #[test]
    fn video_inseree_listee_cherchee_jamais_lue_par_l_ocr() {
        let conn = crate::db::open_in_memory().expect("base");
        let dossier = Path::new("D:/captures");
        let image = inserer(&conn, &quand(1), "a.png", (10, 10), &origine(), "[]", Some("")).expect("image");
        let video = inserer_video(&conn, &quand(2), r"C:\v\b.mp4", (800, 600), &origine(), 8_000).expect("video");
        assert_eq!(prochaine_a_lire(&conn).expect("lue"), None, "une video n'est jamais lue");
        let liste = lister(&conn, dossier, "").expect("liste");
        assert_eq!(liste.iter().map(|c| (c.id, c.duree_ms)).collect::<Vec<_>>(), vec![(video, Some(8_000)), (image, None)]);
        assert_eq!(liste[0].origine, r"C:\v\b.mp4");
        assert_eq!((liste[0].largeur, liste[0].hauteur), (800, 600));
        noter(&conn, video, "Démo du devis").expect("note");
        let trouvees: Vec<i64> = lister(&conn, dossier, "demo").expect("cherchée").iter().map(|c| c.id).collect();
        assert_eq!(trouvees, vec![video]);
        let du_jour: Vec<i64> = lister(&conn, dossier, "2026-10-02").expect("par jour").iter().map(|c| c.id).collect();
        assert_eq!(du_jour, vec![video, image]);
    }

    #[test]
    fn video_supprimee_vers_la_corbeille_de_son_dossier() {
        let dossier = std::env::temp_dir().join(format!("equishot-videos-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dossier);
        let mp4 = dossier.join("2026-10").join("20261007-101500-000.mp4");
        std::fs::create_dir_all(mp4.parent().expect("parent")).expect("dossier");
        std::fs::write(&mp4, b"mp4").expect("mp4");
        std::fs::write(mp4.with_extension("gif"), b"gif").expect("gif");
        let fichier = mp4.to_string_lossy().into_owned();
        ecrire_rendu(&dossier, &fichier, &RgbaImage::from_pixel(20, 10, image::Rgba([1, 2, 3, 255]))).expect("rendu");
        let c = chemins(&dossier, &fichier);
        assert!(c.origine.exists() && c.rendu.exists() && c.vignette.exists());
        vers_corbeille(&dossier, &fichier).expect("corbeille");
        assert!(!c.origine.exists() && !c.rendu.exists() && !c.vignette.exists() && !mp4.with_extension("gif").exists(), "rien ne reste a sa place");
        for nom in ["20261007-101500-000.mp4", "20261007-101500-000.rendu.png", "20261007-101500-000.vignette.png", "20261007-101500-000.gif"] {
            assert!(dossier.join(CORBEILLE).join(nom).exists(), "{nom} est dans la corbeille");
        }
        let _ = std::fs::remove_dir_all(&dossier);
    }
}
