//! L'animation des effets (0.22, ADR-022), en fonctions pures : les evenements de
//! la souris et le temps donnent la scene a dessiner. Les temps sont en secondes
//! depuis l'origine de l'ecoute, les positions en pixels de la zone filmee.
//! 0.23 (ADR-023) : les touches frappees, et le zoom qui vient seul au clic.

use std::collections::VecDeque;

use super::effets::{pinceau, Couleur, Onde, Projecteur, Scene, Segment, Touches};

pub const JAUNE: Couleur = Couleur(1.0, 196.0 / 255.0, 0.0);
pub const BLEU: Couleur = Couleur(61.0 / 255.0, 155.0 / 255.0, 1.0);
/// Une onde : de 8 a 36 px en 0,5 s.
const ONDE_S: f64 = 0.5;
const ONDE_DE: f32 = 8.0;
const ONDE_A: f32 = 36.0;
const ONDE_EPAISSEUR: f32 = 3.0;
/// Chaque point de la trainee s'efface en 0,6 s.
const TRAINEE_S: f64 = 0.6;
const TRAINEE_EPAISSEUR: f32 = 6.0;
pub const ZOOM: f32 = 2.0;
const ZOOM_S: f64 = 0.35;
/// Le centre de la vue rattrape la souris avec cette constante de temps.
const SUIVI_S: f64 = 0.25;
const PROJECTEUR_S: f64 = 0.2;
const PROJECTEUR_RAYON: f32 = 160.0;
const PROJECTEUR_OMBRE: f32 = 0.6;
/// Le journal garde au plus un mouvement par image (30 i/s).
const JOURNAL_PAS_S: f64 = 1.0 / 30.0;
/// Le zoom venu seul au clic repart 2,5 s apres le dernier clic.
const ZOOM_AUTO_S: f64 = 2.5;
/// Les touches restent 1,5 s apres la derniere frappe, puis s'effacent en 0,3 s.
const TOUCHES_S: f64 = 1.5;
const TOUCHES_FONDU_S: f64 = 0.3;
const TOUCHES_TAILLE: f32 = 22.0;
/// La frappe tapee garde ses 32 derniers caracteres.
const TOUCHES_MAX: usize = 32;

/// Les touches montrees dans la video (0.23) : aucune, les raccourcis seulement
/// (une touche tenue avec Ctrl, Alt ou Win, et F1 a F24), ou toute la frappe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeTouches {
    Aucune,
    #[default]
    Raccourcis,
    Toutes,
}

/// Une touche du clavier. `nom` : ce qu'elle affiche dans un raccourci (« S », « F5 »,
/// « Entrée ») ; `caractere` : ce qu'elle tape, s'il est imprimable.
#[derive(Debug, Clone, PartialEq)]
pub struct Touche {
    pub t: f64,
    pub vk: u32,
    pub appui: bool,
    pub nom: Option<String>,
    pub caractere: Option<String>,
}

/// L'etat des modificateurs et ce que la pastille montre.
#[derive(Debug, Clone, Default)]
struct Clavier {
    ctrl: bool,
    alt: bool,
    /// Alt Gr (Alt de droite) : Windows y ajoute un faux Ctrl, et ce n'est pas un raccourci.
    altgr: bool,
    maj: bool,
    win: bool,
    texte: String,
    /// Le texte montre est un raccourci : la frappe suivante repart de zero.
    raccourci: bool,
    depuis: Option<f64>,
}

impl Clavier {
    fn recevoir(&mut self, k: &Touche, mode: ModeTouches) {
        let modificateur = match k.vk {
            0x10 | 0xA0 | 0xA1 => Some(&mut self.maj),
            0x11 | 0xA2 | 0xA3 => Some(&mut self.ctrl),
            0x12 | 0xA4 => Some(&mut self.alt),
            0xA5 => Some(&mut self.altgr),
            0x5B | 0x5C => Some(&mut self.win),
            _ => None,
        };
        if let Some(m) = modificateur {
            *m = k.appui;
            return;
        }
        if !k.appui || mode == ModeTouches::Aucune {
            return;
        }
        let Some(nom) = &k.nom else { return };
        let (ctrl, alt) = (self.ctrl && !self.altgr, self.alt && !self.altgr);
        let fonction = (0x70..=0x87).contains(&k.vk);
        if ctrl || alt || self.win || fonction {
            let morceaux: Vec<&str> = [(ctrl, "Ctrl"), (alt, "Alt"), (self.maj, "Maj"), (self.win, "Win")].into_iter().filter(|m| m.0).map(|m| m.1).chain([nom.as_str()]).collect();
            self.texte = morceaux.join(" + ");
            self.raccourci = true;
            self.depuis = Some(k.t);
            return;
        }
        if mode != ModeTouches::Toutes {
            return;
        }
        if self.raccourci || self.depuis.is_none_or(|d| k.t - d > TOUCHES_S + TOUCHES_FONDU_S) {
            self.texte.clear();
            self.raccourci = false;
        }
        if k.vk == 0x08 {
            self.texte.pop();
        } else if let Some(c) = &k.caractere {
            self.texte.push_str(c);
        } else {
            self.texte.push_str(&format!(" {nom} "));
        }
        let n = self.texte.chars().count();
        if n > TOUCHES_MAX {
            self.texte = self.texte.chars().skip(n - TOUCHES_MAX).collect();
        }
        self.depuis = Some(k.t);
    }

    fn touches(&self, t: f64, echelle: f32) -> Option<Touches> {
        let depuis = self.depuis?;
        let opacite = 1.0 - ((t - depuis - TOUCHES_S) / TOUCHES_FONDU_S).clamp(0.0, 1.0) as f32;
        (opacite > 0.0 && t >= depuis && !self.texte.trim().is_empty()).then(|| Touches { texte: self.texte.clone(), opacite, taille: TOUCHES_TAILLE * echelle })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Bouton {
    Gauche,
    Droit,
    Milieu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Geste {
    Bouge,
    Appui(Bouton),
    Relache(Bouton),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Evenement {
    pub t: f64,
    pub x: f32,
    pub y: f32,
    pub geste: Geste,
}

/// Ce que l'utilisateur a choisi dans le panneau.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub onde: bool,
    pub trainee: bool,
    /// L'echelle de l'ecran filme : les tailles en pixels CSS deviennent physiques.
    pub echelle: f32,
    /// Le zoom vient seul au clic gauche et repart apres 2,5 s sans clic (0.23).
    pub zoom_auto: bool,
    pub touches: ModeTouches,
}

fn adoucir(p: f64) -> f32 {
    let p = p.clamp(0.0, 1.0);
    (1.0 - (1.0 - p).powi(3)) as f32
}

/// Un effet qu'on allume et qu'on eteint, avec une transition de `duree`.
#[derive(Debug, Clone, Copy)]
struct Bascule {
    allume: bool,
    depuis: f64,
    /// La valeur au moment du dernier geste : une bascule en pleine transition repart d'ici.
    de: f32,
    duree: f64,
}

impl Bascule {
    fn new(duree: f64) -> Self {
        Bascule { allume: false, depuis: f64::NEG_INFINITY, de: 0.0, duree }
    }

    fn valeur(&self, t: f64) -> f32 {
        let cible = if self.allume { 1.0 } else { 0.0 };
        self.de + (cible - self.de) * adoucir((t - self.depuis) / self.duree)
    }

    fn basculer(&mut self, t: f64) {
        self.de = self.valeur(t);
        self.allume = !self.allume;
        self.depuis = t;
    }
}

/// Un point de la trainee : `trait_` separe deux glisses.
#[derive(Debug, Clone, Copy)]
struct Point {
    t: f64,
    x: f32,
    y: f32,
    trait_: u32,
}

pub struct Animateur {
    options: Options,
    souris: (f32, f32),
    ondes: VecDeque<(f64, f32, f32, Couleur)>,
    points: VecDeque<Point>,
    tenu: bool,
    traits: u32,
    zoom: Bascule,
    projecteur: Bascule,
    /// Le centre de la vue, qui suit la souris, et l'instant de son dernier calcul.
    centre: Option<(f32, f32)>,
    vu_a: f64,
    journal: Vec<Evenement>,
    /// Le zoom en cours est venu seul : il repart seul. Un geste a la main le reprend.
    zoom_auto: bool,
    dernier_clic: f64,
    clavier: Clavier,
}

impl Animateur {
    pub fn new(options: Options) -> Self {
        Animateur {
            options,
            souris: (0.0, 0.0),
            ondes: VecDeque::new(),
            points: VecDeque::new(),
            tenu: false,
            traits: 0,
            zoom: Bascule::new(ZOOM_S),
            projecteur: Bascule::new(PROJECTEUR_S),
            centre: None,
            vu_a: f64::NEG_INFINITY,
            journal: Vec::new(),
            zoom_auto: false,
            dernier_clic: f64::NEG_INFINITY,
            clavier: Clavier::default(),
        }
    }

    pub fn recevoir_touche(&mut self, k: &Touche) {
        self.clavier.recevoir(k, self.options.touches);
    }

    /// Maj et Alt Gr tenus : de quoi lire le caractere d'une touche.
    pub fn modificateurs(&self) -> (bool, bool) {
        (self.clavier.maj, self.clavier.altgr)
    }

    pub fn recevoir(&mut self, e: Evenement) {
        self.souris = (e.x, e.y);
        let garder = match e.geste {
            Geste::Bouge => self.journal.last().is_none_or(|d| d.geste != Geste::Bouge || e.t - d.t >= JOURNAL_PAS_S),
            _ => true,
        };
        if garder {
            self.journal.push(e);
        }
        match e.geste {
            Geste::Appui(b) => {
                if self.options.onde && b != Bouton::Milieu {
                    self.ondes.push_back((e.t, e.x, e.y, if b == Bouton::Gauche { JAUNE } else { BLEU }));
                }
                if b == Bouton::Gauche {
                    self.tenu = true;
                    self.traits += 1;
                    self.pointer(e);
                    if self.options.zoom_auto {
                        self.dernier_clic = e.t;
                        if !self.zoom.allume {
                            self.zoom.basculer(e.t);
                            self.zoom_auto = true;
                        }
                    }
                }
            }
            Geste::Relache(Bouton::Gauche) => self.tenu = false,
            Geste::Relache(_) => {}
            Geste::Bouge => {
                if self.tenu {
                    self.pointer(e);
                }
            }
        }
    }

    fn pointer(&mut self, e: Evenement) {
        if self.options.trainee {
            self.points.push_back(Point { t: e.t, x: e.x, y: e.y, trait_: self.traits });
        }
    }

    pub fn basculer_zoom(&mut self, t: f64) -> bool {
        self.zoom_auto = false;
        self.zoom.basculer(t);
        self.zoom.allume
    }

    pub fn basculer_projecteur(&mut self, t: f64) -> bool {
        self.projecteur.basculer(t);
        self.projecteur.allume
    }

    /// La scene a l'instant `t`. Avance le suivi de la vue.
    pub fn scene(&mut self, t: f64) -> Scene {
        let e = self.options.echelle;
        if self.zoom_auto && self.zoom.allume && !self.tenu && t - self.dernier_clic > ZOOM_AUTO_S {
            self.zoom.basculer(t);
            self.zoom_auto = false;
        }
        while self.ondes.front().is_some_and(|o| t - o.0 > ONDE_S) {
            self.ondes.pop_front();
        }
        while self.points.front().is_some_and(|p| t - p.t > TRAINEE_S) {
            self.points.pop_front();
        }
        let ondes = self
            .ondes
            .iter()
            .filter(|o| t >= o.0)
            .map(|&(t0, x, y, couleur)| {
                let p = (t - t0) / ONDE_S;
                Onde { x, y, rayon: (ONDE_DE + (ONDE_A - ONDE_DE) * adoucir(p)) * e, epaisseur: ONDE_EPAISSEUR * e, opacite: 1.0 - p as f32, couleur }
            })
            .collect();
        // 0.24 : chaque glisse a l'epaisseur du pinceau d'encre de Chine.
        let mut segments = Vec::new();
        let points: Vec<&Point> = self.points.iter().collect();
        for glisse in points.chunk_by(|a, b| a.trait_ == b.trait_) {
            let largeurs = pinceau(&glisse.iter().map(|p| (p.x, p.y, p.t)).collect::<Vec<_>>(), TRAINEE_EPAISSEUR * e);
            for (i, paire) in glisse.windows(2).enumerate() {
                let (a, b) = (paire[0], paire[1]);
                segments.push(Segment {
                    de: (a.x, a.y),
                    a: (b.x, b.y),
                    epaisseur: (largeurs[i] + largeurs[i + 1]) / 2.0,
                    opacite: (1.0 - (t - a.t) / TRAINEE_S).clamp(0.0, 1.0) as f32,
                    couleur: JAUNE,
                });
            }
        }
        let force = self.zoom.valeur(t);
        let centre = match self.centre {
            Some(c) if force > 0.0 => {
                let k = 1.0 - (-(t - self.vu_a).max(0.0) / SUIVI_S).exp() as f32;
                (c.0 + (self.souris.0 - c.0) * k, c.1 + (self.souris.1 - c.1) * k)
            }
            // Vue entiere : le prochain zoom part de la souris.
            _ => self.souris,
        };
        self.centre = Some(centre);
        self.vu_a = t;
        let ombre = self.projecteur.valeur(t) * PROJECTEUR_OMBRE;
        let projecteur = (ombre > 0.0).then_some(Projecteur { x: self.souris.0, y: self.souris.1, rayon: PROJECTEUR_RAYON * e, opacite: ombre });
        let touches = self.clavier.touches(t, e);
        Scene { zoom: (centre.0, centre.1, 1.0 + (ZOOM - 1.0) * force), ondes, segments, projecteur, curseur: None, traits: Vec::new(), touches }
    }

    pub fn allumes(&self) -> (bool, bool) {
        (self.zoom.allume, self.projecteur.allume)
    }

    /// Les evenements gardes pour le journal, dans l'ordre.
    pub fn journal(&self) -> &[Evenement] {
        &self.journal
    }
}

/// Le journal de la souris (`<nom>.souris.json`), en millisecondes de la video :
/// `actif` rend le temps de video d'un temps de l'ecoute, `None` pendant une pause
/// ou avant la premiere image.
pub fn journal(evenements: &[Evenement], zone: (u32, u32), actif: impl Fn(f64) -> Option<f64>) -> serde_json::Value {
    let lignes: Vec<serde_json::Value> = evenements
        .iter()
        .filter_map(|e| {
            let ms = (actif(e.t)? * 1000.0).round() as i64;
            let (geste, bouton) = match e.geste {
                Geste::Bouge => ("bouge", None),
                Geste::Appui(b) => ("appui", Some(b)),
                Geste::Relache(b) => ("relache", Some(b)),
            };
            Some(serde_json::json!({ "ms": ms, "geste": geste, "bouton": bouton, "x": e.x.round() as i32, "y": e.y.round() as i32 }))
        })
        .collect();
    serde_json::json!({ "version": 1, "zone": { "largeur": zone.0, "hauteur": zone.1 }, "evenements": lignes })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(onde: bool, trainee: bool) -> Options {
        Options { onde, trainee, echelle: 1.0, zoom_auto: false, touches: ModeTouches::Raccourcis }
    }

    fn animateur(onde: bool, trainee: bool) -> Animateur {
        Animateur::new(options(onde, trainee))
    }

    fn touche(t: f64, vk: u32, appui: bool, nom: &str, caractere: Option<&str>) -> Touche {
        Touche { t, vk, appui, nom: (!nom.is_empty()).then(|| nom.to_string()), caractere: caractere.map(str::to_string) }
    }

    fn frapper(a: &mut Animateur, t: f64, vk: u32, nom: &str, caractere: Option<&str>) {
        a.recevoir_touche(&touche(t, vk, true, nom, caractere));
        a.recevoir_touche(&touche(t, vk, false, nom, caractere));
    }

    #[test]
    fn un_raccourci_s_affiche_puis_s_efface() {
        let mut a = animateur(false, false);
        a.recevoir_touche(&touche(1.0, 0xA2, true, "", None));
        a.recevoir_touche(&touche(1.0, 0xA0, true, "", None));
        frapper(&mut a, 1.1, 0x53, "S", Some("S"));
        let t = a.scene(1.2).touches.expect("montre");
        assert_eq!((t.texte.as_str(), t.opacite, t.taille), ("Ctrl + Maj + S", 1.0, 22.0));
        assert!(a.scene(2.75).touches.expect("en fondu").opacite < 0.6);
        assert!(a.scene(3.0).touches.is_none(), "effacee 1,8 s apres");
    }

    #[test]
    fn la_frappe_seule_ne_s_affiche_pas_en_raccourcis() {
        let mut a = animateur(false, false);
        frapper(&mut a, 0.0, 0x41, "A", Some("a"));
        a.recevoir_touche(&touche(0.1, 0xA0, true, "", None));
        frapper(&mut a, 0.2, 0x42, "B", Some("B"));
        assert!(a.scene(0.3).touches.is_none());
        // F5 seule est un raccourci.
        frapper(&mut a, 0.4, 0x74, "F5", None);
        assert_eq!(a.scene(0.5).touches.expect("F5").texte, "Maj + F5");
    }

    #[test]
    fn alt_gr_n_est_pas_un_raccourci() {
        let mut a = animateur(false, false);
        // Alt Gr : Windows envoie un faux Ctrl gauche puis Alt de droite.
        a.recevoir_touche(&touche(0.0, 0xA2, true, "", None));
        a.recevoir_touche(&touche(0.0, 0xA5, true, "", None));
        frapper(&mut a, 0.1, 0x30, "0", Some("@"));
        assert!(a.scene(0.2).touches.is_none());
    }

    #[test]
    fn toute_la_frappe_avec_retour_arriere() {
        let mut a = Animateur::new(Options { touches: ModeTouches::Toutes, ..options(false, false) });
        for (i, (vk, nom, c)) in [(0x42, "B", "b"), (0x4F, "O", "o"), (0x4E, "N", "n"), (0x08, "Retour", ""), (0x4E, "N", "n"), (0x20, "Espace", " "), (0x0D, "Entrée", "")].into_iter().enumerate() {
            frapper(&mut a, i as f64 * 0.1, vk, nom, (!c.is_empty()).then_some(c));
        }
        assert_eq!(a.scene(0.7).touches.expect("frappe").texte, "bon  Entrée ");
        // Un raccourci remplace la frappe, et la frappe suivante repart de zero.
        a.recevoir_touche(&touche(0.8, 0xA2, true, "", None));
        frapper(&mut a, 0.8, 0x43, "C", Some("c"));
        a.recevoir_touche(&touche(0.8, 0xA2, false, "", None));
        assert_eq!(a.scene(0.85).touches.expect("raccourci").texte, "Ctrl + C");
        frapper(&mut a, 0.9, 0x58, "X", Some("x"));
        assert_eq!(a.scene(0.95).touches.expect("frappe").texte, "x");
    }

    #[test]
    fn aucune_touche_quand_c_est_eteint() {
        let mut a = Animateur::new(Options { touches: ModeTouches::Aucune, ..options(false, false) });
        a.recevoir_touche(&touche(0.0, 0xA2, true, "", None));
        frapper(&mut a, 0.1, 0x53, "S", Some("s"));
        assert!(a.scene(0.2).touches.is_none());
    }

    #[test]
    fn zoom_auto_au_clic_puis_retour_apres_le_dernier_clic() {
        let mut a = Animateur::new(Options { zoom_auto: true, ..options(false, false) });
        a.recevoir(ev(1.0, 100.0, 80.0, Geste::Appui(Bouton::Gauche)));
        a.recevoir(ev(1.1, 100.0, 80.0, Geste::Relache(Bouton::Gauche)));
        assert!((a.scene(1.4).zoom.2 - 2.0).abs() < 1e-6, "zoome au clic");
        a.recevoir(ev(3.0, 120.0, 80.0, Geste::Appui(Bouton::Gauche)));
        a.recevoir(ev(3.1, 120.0, 80.0, Geste::Relache(Bouton::Gauche)));
        assert!((a.scene(5.0).zoom.2 - 2.0).abs() < 1e-6, "un clic de plus le garde");
        a.scene(5.6);
        assert!((a.scene(6.0).zoom.2 - 1.0).abs() < 1e-6, "reparti 2,5 s apres le dernier clic");
        // Un zoom pris a la main ne repart pas seul.
        a.basculer_zoom(7.0);
        a.recevoir(ev(7.1, 0.0, 0.0, Geste::Appui(Bouton::Gauche)));
        a.recevoir(ev(7.2, 0.0, 0.0, Geste::Relache(Bouton::Gauche)));
        assert!((a.scene(20.0).zoom.2 - 2.0).abs() < 1e-6, "le zoom a la main reste");
    }

    #[test]
    fn sans_zoom_auto_le_clic_ne_zoome_pas() {
        let mut a = animateur(false, false);
        a.recevoir(ev(1.0, 100.0, 80.0, Geste::Appui(Bouton::Gauche)));
        assert!((a.scene(1.5).zoom.2 - 1.0).abs() < 1e-6);
    }

    fn ev(t: f64, x: f32, y: f32, geste: Geste) -> Evenement {
        Evenement { t, x, y, geste }
    }

    #[test]
    fn onde_qui_s_ouvre_et_s_efface_en_une_demi_seconde() {
        let mut a = animateur(true, false);
        a.recevoir(ev(1.0, 100.0, 50.0, Geste::Appui(Bouton::Gauche)));
        let o = a.scene(1.0).ondes;
        assert_eq!(o.len(), 1);
        assert_eq!((o[0].x, o[0].y, o[0].rayon, o[0].opacite, o[0].couleur), (100.0, 50.0, 8.0, 1.0, JAUNE));
        let o = a.scene(1.25).ondes;
        assert!(o[0].rayon > 30.0 && o[0].rayon < 36.0 && (o[0].opacite - 0.5).abs() < 1e-6, "{o:?}");
        assert!(a.scene(1.51).ondes.is_empty());
    }

    #[test]
    fn onde_bleue_au_clic_droit_aucune_au_milieu_ni_sans_l_option() {
        let mut a = animateur(true, false);
        a.recevoir(ev(0.0, 1.0, 1.0, Geste::Appui(Bouton::Droit)));
        a.recevoir(ev(0.0, 1.0, 1.0, Geste::Appui(Bouton::Milieu)));
        let o = a.scene(0.1).ondes;
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].couleur, BLEU);
        let mut sans = animateur(false, false);
        sans.recevoir(ev(0.0, 1.0, 1.0, Geste::Appui(Bouton::Gauche)));
        assert!(sans.scene(0.1).ondes.is_empty());
    }

    #[test]
    fn les_tailles_suivent_l_echelle_de_l_ecran() {
        let mut a = Animateur::new(Options { echelle: 2.0, ..options(true, false) });
        a.recevoir(ev(0.0, 1.0, 1.0, Geste::Appui(Bouton::Gauche)));
        let o = a.scene(0.0).ondes;
        assert_eq!((o[0].rayon, o[0].epaisseur), (16.0, 6.0));
    }

    #[test]
    fn trainee_seulement_bouton_gauche_tenu() {
        let mut a = animateur(false, true);
        a.recevoir(ev(0.0, 0.0, 0.0, Geste::Bouge));
        a.recevoir(ev(0.1, 10.0, 0.0, Geste::Bouge));
        assert!(a.scene(0.1).segments.is_empty(), "rien sans clic tenu");
        a.recevoir(ev(0.2, 20.0, 0.0, Geste::Appui(Bouton::Gauche)));
        a.recevoir(ev(0.3, 30.0, 0.0, Geste::Bouge));
        a.recevoir(ev(0.4, 40.0, 0.0, Geste::Bouge));
        a.recevoir(ev(0.5, 40.0, 0.0, Geste::Relache(Bouton::Gauche)));
        a.recevoir(ev(0.6, 90.0, 0.0, Geste::Bouge));
        let s = a.scene(0.6).segments;
        assert_eq!(s.iter().map(|s| (s.de.0, s.a.0)).collect::<Vec<_>>(), vec![(20.0, 30.0), (30.0, 40.0)]);
        assert!(s[0].opacite < s[1].opacite, "la queue s'efface la premiere");
        // Le clic droit tenu ne trace rien.
        a.recevoir(ev(0.7, 0.0, 0.0, Geste::Appui(Bouton::Droit)));
        a.recevoir(ev(0.75, 50.0, 0.0, Geste::Bouge));
        let s = a.scene(0.75).segments;
        assert_eq!(s.iter().map(|s| (s.de.0, s.a.0)).collect::<Vec<_>>(), vec![(20.0, 30.0), (30.0, 40.0)], "rien de neuf au clic droit");
        assert!(a.scene(1.01).segments.is_empty(), "effacee 0,6 s apres");
    }

    #[test]
    fn la_trainee_a_la_forme_du_pinceau() {
        let mut a = animateur(false, true);
        a.recevoir(ev(0.0, 0.0, 0.0, Geste::Appui(Bouton::Gauche)));
        for i in 1..40 {
            a.recevoir(ev(i as f64 * 0.01, i as f32 * 3.0, 0.0, Geste::Bouge));
        }
        let s = a.scene(0.4).segments;
        let (bout, milieu) = (s[0].epaisseur, s[s.len() / 2].epaisseur);
        assert!(bout < milieu * 0.6, "effilee au depart : {bout} contre {milieu}");
        assert!(milieu > 6.0, "lente, elle s'epaissit : {milieu}");
    }

    #[test]
    fn deux_glisses_ne_sont_pas_reliees() {
        let mut a = animateur(false, true);
        for (t, x) in [(0.0, 0.0), (0.05, 10.0)] {
            a.recevoir(ev(t, x, 0.0, if t == 0.0 { Geste::Appui(Bouton::Gauche) } else { Geste::Bouge }));
        }
        a.recevoir(ev(0.1, 10.0, 0.0, Geste::Relache(Bouton::Gauche)));
        a.recevoir(ev(0.15, 100.0, 0.0, Geste::Appui(Bouton::Gauche)));
        a.recevoir(ev(0.2, 110.0, 0.0, Geste::Bouge));
        let s = a.scene(0.2).segments;
        assert_eq!(s.iter().map(|s| (s.de.0, s.a.0)).collect::<Vec<_>>(), vec![(0.0, 10.0), (100.0, 110.0)]);
    }

    #[test]
    fn zoom_qui_s_ouvre_suit_la_souris_et_revient() {
        let mut a = animateur(false, false);
        a.recevoir(ev(0.0, 100.0, 80.0, Geste::Bouge));
        assert_eq!(a.scene(0.0).zoom, (100.0, 80.0, 1.0));
        assert!(a.basculer_zoom(1.0));
        let z = a.scene(1.0).zoom;
        assert_eq!(z, (100.0, 80.0, 1.0));
        assert!((a.scene(1.35).zoom.2 - 2.0).abs() < 1e-6);
        // La souris part : la vue la suit sans sauter.
        a.recevoir(ev(1.4, 300.0, 80.0, Geste::Bouge));
        let x1 = a.scene(1.45).zoom.0;
        assert!(x1 > 100.0 && x1 < 200.0, "suivi doux : {x1}");
        let x2 = a.scene(3.0).zoom.0;
        assert!((x2 - 300.0).abs() < 1.0, "rattrapee : {x2}");
        assert!(!a.basculer_zoom(3.0));
        assert!((a.scene(3.35).zoom.2 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn zoom_rebascule_en_pleine_transition_sans_sauter() {
        let mut a = animateur(false, false);
        a.basculer_zoom(0.0);
        let mi = a.scene(0.1).zoom.2;
        a.basculer_zoom(0.1);
        assert!((a.scene(0.1).zoom.2 - mi).abs() < 1e-6);
    }

    #[test]
    fn projecteur_autour_de_la_souris() {
        let mut a = animateur(false, false);
        a.recevoir(ev(0.0, 50.0, 60.0, Geste::Bouge));
        assert!(a.scene(0.0).projecteur.is_none());
        a.basculer_projecteur(0.0);
        let p = a.scene(0.2).projecteur.expect("allume");
        assert_eq!((p.x, p.y, p.rayon), (50.0, 60.0, 160.0));
        assert!((p.opacite - 0.6).abs() < 1e-6);
        a.basculer_projecteur(1.0);
        assert!(a.scene(1.2).projecteur.is_none());
    }

    #[test]
    fn journal_en_temps_de_video_sans_les_pauses() {
        let mut a = animateur(false, false);
        for t in [0.0, 0.01, 0.02, 0.04] {
            a.recevoir(ev(t, t as f32 * 100.0, 0.0, Geste::Bouge));
        }
        a.recevoir(ev(0.041, 4.0, 0.0, Geste::Appui(Bouton::Gauche)));
        a.recevoir(ev(2.0, 4.0, 0.0, Geste::Relache(Bouton::Gauche)));
        a.recevoir(ev(3.0, 4.0, 0.0, Geste::Relache(Bouton::Droit)));
        // Un mouvement par 1/30 s : 0,01 et 0,02 tombent.
        assert_eq!(a.journal().iter().map(|e| e.t).collect::<Vec<_>>(), vec![0.0, 0.04, 0.041, 2.0, 3.0]);
        // Pause de 1 a 2,5 s, premiere image a 0,02.
        let actif = |t: f64| match t {
            t if t < 0.02 => None,
            t if t < 1.0 => Some(t - 0.02),
            t if t < 2.5 => None,
            t => Some(t - 0.02 - 1.5),
        };
        let j = journal(a.journal(), (640, 360), actif);
        let ms: Vec<i64> = j["evenements"].as_array().expect("liste").iter().map(|e| e["ms"].as_i64().expect("ms")).collect();
        assert_eq!(ms, vec![20, 21, 1480]);
        assert_eq!(j["evenements"][1]["geste"], "appui");
        assert_eq!(j["evenements"][1]["bouton"], "gauche");
        assert_eq!(j["zone"]["largeur"], 640);
    }
}
