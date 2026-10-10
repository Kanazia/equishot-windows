//! Raccourci systeme de capture, en deux cases :
//! principal et secondaire, chacune au clavier ou a la souris (ADR-003).
//!
//! Le raccourci clavier s'enregistre aupres de Windows a chaud, jamais dans le
//! `Builder` du greffon : une combinaison deja prise par une autre application
//! y ferait echouer le demarrage d'EquiShot. Ici, elle laisse l'etat
//! `indisponible`, que la pastille et les reglages montrent. Une case a la
//! souris passe par le crochet de `souris.rs`.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Modifiers, Shortcut};

use super::souris;

pub const DEFAUT: &str = "Ctrl+Alt+S";
/// L'instance de developpement tourne a cote de la production : elle ne lui
/// dispute pas son raccourci, sans quoi une frappe au clavier partirait dans un
/// parcours, ou la production trouverait le sien deja pris.
pub const DEFAUT_DEV: &str = "Ctrl+Alt+Shift+S";
/// Secondaire de la production. L'instance dev n'en a pas : elle ne dispute pas
/// le geste a la production ouverte a cote.
pub const DEFAUT_SOURIS: &str = "Alt+Bouton5";

/// Troisieme case : reprendre la derniere zone (ADR-009). R pour « reprendre ».
pub const DEFAUT_DERNIERE: &str = "Ctrl+Alt+R";
/// Quatrieme case : le texte d'une zone (ADR-010). T pour « texte ».
pub const DEFAUT_TEXTE: &str = "Ctrl+Alt+T";
/// Cinquieme case : filmer, ou arreter la video en cours (ADR-016). F pour
/// « filmer » : Ctrl+Alt+V est le collage special d'Office.
pub const DEFAUT_VIDEO: &str = "Ctrl+Alt+F";
/// Sixieme case : l'ecran entier (0.21). P pour « plein ecran » :
/// E est une touche d'AltGr (€) sur un clavier AZERTY.
pub const DEFAUT_ECRAN: &str = "Ctrl+Alt+P";

pub const PRINCIPAL: usize = 0;
pub const SECONDAIRE: usize = 1;
/// Reprend la derniere zone au lieu d'ouvrir la selection.
pub const DERNIERE: usize = 2;
/// Trace une zone et copie son texte, pas l'image (ADR-010).
pub const TEXTE: usize = 3;
/// Ouvre la selection en mode video, ou arrete la video en cours (ADR-016).
pub const VIDEO: usize = 4;
/// Prend l'ecran entier, sans selection (0.21).
pub const ECRAN: usize = 5;
pub const NB_CASES: usize = 6;
pub const CASES: [usize; NB_CASES] = [PRINCIPAL, SECONDAIRE, DERNIERE, TEXTE, VIDEO, ECRAN];
/// Le nom de chaque case, cote page et cote commande.
pub const NOMS: [&str; NB_CASES] = ["principal", "secondaire", "derniere", "texte", "video", "ecran"];

pub fn defaut(dev: bool) -> &'static str {
    if dev {
        DEFAUT_DEV
    } else {
        DEFAUT
    }
}

/// Ce qui declenche une capture, tel qu'il est persiste.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Declencheur {
    Clavier { combinaison: String },
    Souris { combinaison: String },
    Aucun,
}

impl Declencheur {
    fn type_et_combinaison(&self) -> (&'static str, String) {
        match self {
            Declencheur::Clavier { combinaison } => ("clavier", combinaison.clone()),
            Declencheur::Souris { combinaison } => ("souris", combinaison.clone()),
            Declencheur::Aucun => ("aucun", String::new()),
        }
    }
}

/// Le defaut d'une case pour l'instance.
pub fn defaut_case(case: usize, dev: bool) -> Declencheur {
    match (case, dev) {
        (PRINCIPAL, _) => Declencheur::Clavier { combinaison: defaut(dev).to_string() },
        (_, true) => Declencheur::Aucun,
        (DERNIERE, false) => Declencheur::Clavier { combinaison: DEFAUT_DERNIERE.to_string() },
        (TEXTE, false) => Declencheur::Clavier { combinaison: DEFAUT_TEXTE.to_string() },
        (VIDEO, false) => Declencheur::Clavier { combinaison: DEFAUT_VIDEO.to_string() },
        (ECRAN, false) => Declencheur::Clavier { combinaison: DEFAUT_ECRAN.to_string() },
        _ => Declencheur::Souris { combinaison: DEFAUT_SOURIS.to_string() },
    }
}

/// Verifie un declencheur et le rend en ecriture canonique. « Aucun » n'est
/// permis qu'hors du principal : le principal ouvre toujours quelque chose.
pub fn valider(declencheur: Declencheur, case: usize) -> Result<Declencheur, String> {
    match declencheur {
        Declencheur::Clavier { combinaison } => canonique(&combinaison).map(|(_, lisible)| Declencheur::Clavier { combinaison: lisible }),
        Declencheur::Souris { combinaison } => souris::canonique(&combinaison).map(|(_, _, lisible)| Declencheur::Souris { combinaison: lisible }),
        Declencheur::Aucun if case != PRINCIPAL => Ok(Declencheur::Aucun),
        Declencheur::Aucun => Err("Le raccourci principal ne peut pas être retiré.".to_string()),
    }
}

/// Vrai si `nouveau` reprend la combinaison de l'autre case. « Aucun » n'est jamais un doublon.
pub fn doublon(nouveau: &Declencheur, autre: &Declencheur) -> bool {
    *nouveau != Declencheur::Aucun && nouveau == autre
}

/// La premiere autre case que `nouveau`, pose a la case `i`, reprendrait.
pub fn doublon_de(nouveau: &Declencheur, cases: &[Declencheur; NB_CASES], i: usize) -> Option<usize> {
    CASES.into_iter().find(|&autre| autre != i && doublon(nouveau, &cases[autre]))
}

/// Une case lue en base. Vide, illisible ou invalide : le defaut de la case.
pub fn lire_case(brut: &str, case: usize, dev: bool) -> Declencheur {
    serde_json::from_str::<Declencheur>(brut).ok().and_then(|d| valider(d, case).ok()).unwrap_or_else(|| defaut_case(case, dev))
}

/// Lit une combinaison frappee ou reglee, et rend son ecriture canonique
/// (`Ctrl+Alt+S`). Un raccourci systeme avale la frappe pour toutes les
/// applications : il lui faut deux modificateurs au moins, dont Ctrl, Alt ou Win.
pub fn canonique(saisie: &str) -> Result<(Shortcut, String), String> {
    let raccourci = Shortcut::from_str(saisie.trim()).map_err(|_| format!("« {saisie} » n'est pas une combinaison reconnue."))?;
    // Avec un seul modificateur, c'est un raccourci de toutes les applications :
    // Ctrl+V, Ctrl+Entree, Ctrl+Tab, Alt+F4, Alt+Espace, les lettres de menu,
    // les codes Alt du pave numerique. Aucun n'est a prendre pour la capture.
    let principal = raccourci.mods.intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER);
    if !principal || raccourci.mods.bits().count_ones() < 2 {
        return Err("Il faut au moins deux touches parmi Ctrl, Alt, Maj et Win, dont Ctrl, Alt ou Win, puis une touche : avec une seule, le raccourci prendrait une frappe courante à toutes les applications.".to_string());
    }
    let touche = raccourci.key.to_string();
    let lettre = touche.strip_prefix("Key").filter(|reste| reste.len() == 1);
    let chiffre = touche.starts_with("Digit");
    // Ctrl+Alt, c'est AltGr : sur un clavier francais, les chiffres, E et la
    // ponctuation y portent l'arobase, le diese, l'euro, les accolades.
    let ponctuation = [
        "Minus", "Equal", "BracketLeft", "BracketRight", "Backslash", "Semicolon", "Quote", "Backquote", "Comma", "Period",
        "Slash", "IntlBackslash",
    ];
    if raccourci.mods == (Modifiers::CONTROL | Modifiers::ALT) && (chiffre || lettre == Some("E") || ponctuation.contains(&touche.as_str())) {
        return Err("Ctrl+Alt avec cette touche, c'est AltGr : elle sert à taper un caractère (@, #, €). Ajoutez Maj ou choisissez une autre touche.".to_string());
    }
    let mut morceaux = Vec::new();
    // `Super`, pas `Win` : le greffon ne relit pas `Win`, et cette ecriture est
    // celle qui est persistee. L'ecran l'affiche « Win ».
    for (drapeau, nom) in [(Modifiers::CONTROL, "Ctrl"), (Modifiers::ALT, "Alt"), (Modifiers::SHIFT, "Shift"), (Modifiers::SUPER, "Super")] {
        if raccourci.mods.contains(drapeau) {
            morceaux.push(nom.to_string());
        }
    }
    let courte = ["Key", "Digit"].iter().find_map(|p| touche.strip_prefix(p).filter(|reste| reste.len() == 1));
    morceaux.push(courte.unwrap_or(&touche).to_string());
    Ok((raccourci, morceaux.join("+")))
}

/// Emis vers toutes les fenetres quand le raccourci change : la pastille suit.
pub const EVENEMENT_RACCOURCI: &str = "captures://raccourci";

/// Ce que la page lit d'une case : son declencheur, celui par defaut, et si
/// Windows l'a accepte.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EtatCase {
    pub r#type: &'static str,
    pub combinaison: String,
    pub defaut_type: &'static str,
    pub defaut: String,
    /// `enregistre` ou `indisponible`.
    pub etat: &'static str,
    pub motif: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EtatRaccourci {
    pub principal: EtatCase,
    pub secondaire: EtatCase,
    pub derniere: EtatCase,
    pub texte: EtatCase,
    pub video: EtatCase,
    pub ecran: EtatCase,
}

/// Une case en place. `actif` est ce que Windows tient pour une case clavier a
/// cet instant : `None` quand la combinaison est prise ailleurs, ou suspendue.
struct Case {
    declencheur: Declencheur,
    actif: Option<Shortcut>,
    motif: Option<String>,
}

impl Default for Case {
    fn default() -> Self {
        Case { declencheur: Declencheur::Aucun, actif: None, motif: None }
    }
}

#[derive(Default)]
pub struct Raccourci {
    cases: [Case; NB_CASES],
    suspendu: bool,
    dev: bool,
}

fn poser<R: tauri::Runtime>(app: &tauri::AppHandle<R>, combinaison: &str) -> Result<Shortcut, String> {
    let (raccourci, lisible) = canonique(combinaison)?;
    app.global_shortcut().register(raccourci).map_err(|e| {
        // Le greffon aplatit l'erreur en texte. Son seul cas courant est la
        // combinaison deja prise ; tout autre refus est rendu tel que Windows le dit.
        let touches = lisible.replace("Shift", "Maj").replace("Super", "Win").replace('+', " + ");
        let detail = e.to_string();
        if detail.contains("already registered") {
            format!("Windows refuse {touches} : cette combinaison est déjà prise.")
        } else {
            format!("Windows refuse {touches} : {detail}")
        }
    })?;
    Ok(raccourci)
}

impl Raccourci {
    fn etat_case(&self, i: usize) -> EtatCase {
        let case = &self.cases[i];
        let (r#type, combinaison) = case.declencheur.type_et_combinaison();
        let (defaut_type, defaut) = defaut_case(i, self.dev).type_et_combinaison();
        let (etat, motif) = match &case.declencheur {
            Declencheur::Clavier { .. } if case.actif.is_none() && !self.suspendu => ("indisponible", case.motif.clone()),
            Declencheur::Souris { .. } if souris::refuse() => ("indisponible", Some("Windows refuse le crochet de la souris : ce raccourci ne marche pas.".to_string())),
            _ => ("enregistre", None),
        };
        EtatCase { r#type, combinaison, defaut_type, defaut, etat, motif }
    }

    pub fn etat(&self) -> EtatRaccourci {
        EtatRaccourci { principal: self.etat_case(PRINCIPAL), secondaire: self.etat_case(SECONDAIRE), derniere: self.etat_case(DERNIERE), texte: self.etat_case(TEXTE), video: self.etat_case(VIDEO), ecran: self.etat_case(ECRAN) }
    }

    /// La case clavier dont `raccourci` est la combinaison, telle que Windows la tient.
    pub fn case_de(&self, raccourci: &Shortcut) -> Option<usize> {
        self.cases.iter().position(|c| c.actif.as_ref().is_some_and(|actif| actif.id() == raccourci.id()))
    }

    /// Le declencheur d'une case, tel qu'il est en place.
    pub fn declencheur(&self, i: usize) -> Declencheur {
        self.cases[i].declencheur.clone()
    }

    /// Au demarrage : les six cases du reglage, posees si Windows veut.
    pub fn demarrer<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>, bruts: [&str; NB_CASES], dev: bool) {
        self.dev = dev;
        for i in CASES {
            self.cases[i].declencheur = lire_case(bruts[i], i, dev);
            // Une base ecrite a la main peut porter deux fois la meme combinaison :
            // la case la plus haute la cede.
            if CASES[..i].iter().any(|&avant| doublon(&self.cases[i].declencheur, &self.cases[avant].declencheur)) {
                self.cases[i].declencheur = Declencheur::Aucun;
            }
            self.poser_case(app, i);
        }
    }

    fn poser_case<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>, i: usize) {
        let case = &mut self.cases[i];
        case.motif = None;
        match &case.declencheur {
            Declencheur::Clavier { combinaison } => {
                souris::regler(i, 0);
                match poser(app, combinaison) {
                    Ok(raccourci) => case.actif = Some(raccourci),
                    Err(motif) => {
                        case.actif = None;
                        case.motif = Some(motif);
                    }
                }
            }
            Declencheur::Souris { combinaison } => {
                let combinaison = souris::canonique(combinaison).map(|(m, b, _)| souris::tasser(m, b)).unwrap_or(0);
                souris::regler(i, combinaison);
            }
            Declencheur::Aucun => souris::regler(i, 0),
        }
    }

    fn retirer_clavier<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>, i: usize) {
        if let Some(ancien) = self.cases[i].actif.take() {
            // Un echec ici ferait dire a tort, au prochain enregistrement, qu'une
            // autre application tient la combinaison : il part a la console.
            if let Err(e) = app.global_shortcut().unregister(ancien) {
                eprintln!("raccourci de capture non rendu à Windows : {e}");
            }
        }
    }

    /// L'enregistreur des reglages ecoute : aucune case ne doit ouvrir une
    /// capture pendant qu'on refait une combinaison. `false` les remet.
    pub fn suspendre<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>, suspendu: bool) {
        if suspendu {
            for i in CASES {
                self.retirer_clavier(app, i);
            }
            souris::suspendre(true);
            self.suspendu = true;
        } else if self.suspendu {
            self.suspendu = false;
            souris::suspendre(false);
            for i in CASES {
                if matches!(self.cases[i].declencheur, Declencheur::Clavier { .. }) {
                    self.poser_case(app, i);
                }
            }
        }
    }

    /// Change le declencheur d'une case : l'ancien est rendu, le nouveau pose.
    /// Refuse la combinaison d'une autre case. Si Windows refuse une combinaison
    /// clavier, l'ancien declencheur est remis et l'erreur rendue. Rend le JSON a persister.
    pub fn regler<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>, i: usize, saisie: Declencheur) -> Result<String, String> {
        // L'enregistreur a suspendu les cases : quoi qu'il arrive, elles reviennent.
        self.suspendre(app, false);
        let nouveau = valider(saisie, i)?;
        let cases = CASES.map(|c| self.cases[c].declencheur.clone());
        if let Some(autre) = doublon_de(&nouveau, &cases, i) {
            let nom = ["principal", "secondaire", "de la dernière zone", "du texte d'une zone", "de la vidéo", "de l'écran entier"][autre];
            return Err(format!("Cette combinaison est déjà celle du raccourci {nom}."));
        }
        let ancien = std::mem::replace(&mut self.cases[i].declencheur, nouveau.clone());
        self.retirer_clavier(app, i);
        self.poser_case(app, i);
        if let Some(motif) = self.cases[i].motif.take() {
            self.retirer_clavier(app, i);
            self.cases[i].declencheur = ancien;
            self.poser_case(app, i);
            return Err(motif);
        }
        serde_json::to_string(&nouveau).map_err(|e| format!("Raccourci impossible à écrire : {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La combinaison clavier du principal, comme la lisait la version a une case.
    fn combinaison_reglee(brut: &str, dev: bool) -> String {
        match lire_case(brut, PRINCIPAL, dev) {
            Declencheur::Clavier { combinaison } => combinaison,
            _ => defaut(dev).to_string(),
        }
    }

    #[test]
    fn captures_raccourci_ecriture_canonique() {
        assert_eq!(canonique("ctrl+alt+s").expect("lu").1, "Ctrl+Alt+S");
        assert_eq!(canonique("Alt+Control+KeyS").expect("lu").1, "Ctrl+Alt+S", "l'ordre et le nom de touche du clavier sont repris");
        assert_eq!(canonique("Ctrl+Alt+F9").expect("lu").1, "Ctrl+Alt+F9");
        assert_eq!(canonique("Shift+Super+Digit4").expect("lu").1, "Shift+Super+4");
        assert_eq!(canonique("Ctrl+Shift+ArrowUp").expect("lu").1, "Ctrl+Shift+ArrowUp");
        assert_eq!(canonique("Ctrl+Shift+KeyS").expect("lu").1, "Ctrl+Shift+S");
        assert_eq!(canonique("Alt+Shift+F5").expect("lu").1, "Alt+Shift+F5");
        assert_eq!(canonique("Ctrl+Alt+Shift+Digit0").expect("lu").1, "Ctrl+Alt+Shift+0", "avec Maj, ce n'est plus AltGr");
        // L'ecriture canonique se relit elle-meme : c'est elle qui est persistee.
        for defaut in [DEFAUT, DEFAUT_DEV, "Shift+Super+4"] {
            assert_eq!(canonique(defaut).expect("relu").1, defaut);
        }
    }

    #[test]
    fn captures_raccourci_sans_vrai_modificateur_refuse() {
        assert!(canonique("S").is_err(), "une touche seule");
        assert!(canonique("Shift+S").is_err(), "Maj seule avalerait la majuscule");
        assert!(canonique("Ctrl+Alt").is_err(), "des modificateurs sans touche");
        assert!(canonique("Ctrl+Alt+Truc").is_err());
        assert!(canonique("").is_err());
    }

    #[test]
    fn captures_raccourci_ne_prend_ni_coller_ni_l_arobase() {
        // Relecture du 2 octobre 2026 : un Ctrl+V frappe pendant l'ecoute devenait
        // le raccourci systeme, et coller ouvrait la capture partout. Avec un
        // seul modificateur, tout est a une application ou a Windows.
        for prise in [
            "Ctrl+KeyV", "Ctrl+KeyC", "Ctrl+Digit1", "Ctrl+Enter", "Ctrl+Tab", "Ctrl+Insert", "Ctrl+ArrowLeft", "Ctrl+F4", "Ctrl+Numpad0",
            "Alt+F4", "Alt+Space", "Alt+KeyS", "Alt+Numpad2", "Super+KeyE",
        ] {
            assert!(canonique(prise).is_err(), "{prise} est a toutes les applications");
        }
        // Ctrl+Alt est AltGr : l'arobase, le diese, l'euro, les accolades.
        for prise in ["Ctrl+Alt+Digit0", "Ctrl+Alt+Digit3", "Ctrl+Alt+KeyE", "Ctrl+Alt+BracketLeft", "Ctrl+Alt+Equal"] {
            assert!(canonique(prise).is_err(), "{prise} sert a taper un caractere");
        }
        // Les deux defauts et les combinaisons du parcours restent permis.
        for permise in [DEFAUT, DEFAUT_DEV, "Ctrl+Alt+F9", "Ctrl+Alt+F10", "Ctrl+Alt+KeyA", "Alt+Shift+KeyS", "Super+Shift+KeyS", "Ctrl+Shift+F5"] {
            assert!(canonique(permise).is_ok(), "{permise}");
        }
    }

    #[test]
    fn captures_raccourci_reglage_lu_ou_defaut_de_l_instance() {
        assert_eq!(combinaison_reglee("", false), "Ctrl+Alt+S");
        assert_eq!(combinaison_reglee("", true), "Ctrl+Alt+Shift+S", "l'instance dev ne dispute pas le raccourci de la prod");
        assert_eq!(combinaison_reglee(r#"{"type":"clavier","combinaison":"Ctrl+Alt+F9"}"#, false), "Ctrl+Alt+F9");
        assert_eq!(combinaison_reglee("pas du json", false), "Ctrl+Alt+S");
        assert_eq!(combinaison_reglee(r#"{"type":"clavier","combinaison":"S"}"#, false), "Ctrl+Alt+S", "une valeur invalide en base retombe sur le défaut");
        assert_eq!(combinaison_reglee(r#"{"type":"souris","bouton":5}"#, false), "Ctrl+Alt+S", "un type inconnu de cette version aussi");
        let ecrit = serde_json::to_string(&Declencheur::Clavier { combinaison: "Ctrl+Alt+F9".into() }).expect("json");
        assert_eq!(ecrit, r#"{"type":"clavier","combinaison":"Ctrl+Alt+F9"}"#);
    }

    #[test]
    fn captures_raccourci_secondaire_lu_ou_defaut_de_l_instance() {
        let souris5 = Declencheur::Souris { combinaison: "Alt+Bouton5".into() };
        assert_eq!(lire_case("", SECONDAIRE, false), souris5, "la prod a Alt + bouton 5");
        assert_eq!(lire_case("", SECONDAIRE, true), Declencheur::Aucun, "l'instance dev n'a rien");
        assert_eq!(lire_case(r#"{"type":"aucun"}"#, SECONDAIRE, false), Declencheur::Aucun, "retiré, il le reste");
        assert_eq!(
            lire_case(r#"{"type":"souris","combinaison":"shift+ctrl+bouton4"}"#, SECONDAIRE, false),
            Declencheur::Souris { combinaison: "Ctrl+Shift+Bouton4".into() }
        );
        assert_eq!(lire_case(r#"{"type":"clavier","combinaison":"Ctrl+Alt+F9"}"#, SECONDAIRE, true), Declencheur::Clavier { combinaison: "Ctrl+Alt+F9".into() });
        for invalide in [r#"{"type":"souris","combinaison":"Bouton5"}"#, r#"{"type":"souris","combinaison":"Alt+Gauche"}"#, r#"{"type":"souris","combinaison":"Super+Bouton4"}"#, "{"] {
            assert_eq!(lire_case(invalide, SECONDAIRE, false), souris5, "{invalide} retombe sur le défaut");
        }
        // Le principal accepte la souris, jamais « Aucun ».
        assert_eq!(lire_case(r#"{"type":"souris","combinaison":"Alt+Bouton4"}"#, PRINCIPAL, false), Declencheur::Souris { combinaison: "Alt+Bouton4".into() });
        assert_eq!(lire_case(r#"{"type":"aucun"}"#, PRINCIPAL, true), Declencheur::Clavier { combinaison: DEFAUT_DEV.into() });
        assert!(valider(Declencheur::Aucun, PRINCIPAL).is_err());
        assert_eq!(serde_json::to_string(&Declencheur::Aucun).expect("json"), r#"{"type":"aucun"}"#);
    }

    #[test]
    fn captures_raccourci_derniere_zone_troisieme_case() {
        let r = Declencheur::Clavier { combinaison: "Ctrl+Alt+R".into() };
        assert_eq!(lire_case("", DERNIERE, false), r, "la prod a Ctrl+Alt+R");
        assert_eq!(canonique(DEFAUT_DERNIERE).expect("permis").1, DEFAUT_DERNIERE, "le défaut se relit lui-même");
        assert_eq!(lire_case("", DERNIERE, true), Declencheur::Aucun, "l'instance dev ne dispute rien à la prod");
        assert_eq!(lire_case(r#"{"type":"aucun"}"#, DERNIERE, false), Declencheur::Aucun, "retirée, elle le reste");
        assert_eq!(
            lire_case(r#"{"type":"clavier","combinaison":"ctrl+alt+shift+r"}"#, DERNIERE, true),
            Declencheur::Clavier { combinaison: "Ctrl+Alt+Shift+R".into() }
        );
    }

    #[test]
    fn captures_raccourci_texte_quatrieme_case() {
        let t = Declencheur::Clavier { combinaison: "Ctrl+Alt+T".into() };
        assert_eq!(lire_case("", TEXTE, false), t, "la prod a Ctrl+Alt+T");
        assert_eq!(canonique(DEFAUT_TEXTE).expect("permis").1, DEFAUT_TEXTE, "le défaut se relit lui-même");
        assert_eq!(lire_case("", TEXTE, true), Declencheur::Aucun, "l'instance dev ne dispute rien à la prod");
        assert_eq!(lire_case(r#"{"type":"aucun"}"#, TEXTE, false), Declencheur::Aucun, "retirée, elle le reste");
        assert_eq!(NOMS, ["principal", "secondaire", "derniere", "texte", "video", "ecran"]);
        let clavier = |c: &str| Declencheur::Clavier { combinaison: c.into() };
        let cases = [clavier("Ctrl+Alt+S"), Declencheur::Aucun, clavier("Ctrl+Alt+R"), clavier("Ctrl+Alt+T"), Declencheur::Aucun, Declencheur::Aucun];
        assert_eq!(doublon_de(&clavier("Ctrl+Alt+T"), &cases, PRINCIPAL), Some(TEXTE), "la quatrième case compte");
        assert_eq!(doublon_de(&clavier("Ctrl+Alt+R"), &cases, TEXTE), Some(DERNIERE));
        assert_eq!(doublon_de(&clavier("Ctrl+Alt+T"), &cases, TEXTE), None, "sa propre combinaison n'est pas un doublon");
    }

    #[test]
    fn captures_raccourci_video_cinquieme_case() {
        let f = Declencheur::Clavier { combinaison: "Ctrl+Alt+F".into() };
        assert_eq!(lire_case("", VIDEO, false), f, "la prod a Ctrl+Alt+F");
        assert_eq!(canonique(DEFAUT_VIDEO).expect("permis, F n'est pas une touche d'AltGr").1, DEFAUT_VIDEO);
        assert_eq!(lire_case("", VIDEO, true), Declencheur::Aucun, "l'instance dev ne dispute rien à la prod");
        let clavier = |c: &str| Declencheur::Clavier { combinaison: c.into() };
        let cases = [clavier("Ctrl+Alt+S"), Declencheur::Aucun, clavier("Ctrl+Alt+R"), clavier("Ctrl+Alt+T"), f.clone(), Declencheur::Aucun];
        assert_eq!(doublon_de(&f, &cases, PRINCIPAL), Some(VIDEO), "la cinquième case compte");
        assert_eq!(doublon_de(&clavier("Ctrl+Alt+T"), &cases, VIDEO), Some(TEXTE));
        let defauts = CASES.map(|c| defaut_case(c, false));
        assert!(CASES.iter().all(|&c| doublon_de(&defauts[c], &defauts, c).is_none()), "les défauts de la prod ne se doublent pas");
    }

    #[test]
    fn captures_raccourci_ecran_sixieme_case() {
        let p = Declencheur::Clavier { combinaison: "Ctrl+Alt+P".into() };
        assert_eq!(lire_case("", ECRAN, false), p, "la prod a Ctrl+Alt+P");
        assert_eq!(canonique(DEFAUT_ECRAN).expect("permis, P n'est pas une touche d'AltGr").1, DEFAUT_ECRAN);
        assert_eq!(lire_case("", ECRAN, true), Declencheur::Aucun, "l'instance dev ne dispute rien à la prod");
        assert_eq!(lire_case(r#"{"type":"aucun"}"#, ECRAN, false), Declencheur::Aucun, "retirée, elle le reste");
        let defauts = CASES.map(|c| defaut_case(c, false));
        assert_eq!(doublon_de(&p, &defauts, PRINCIPAL), Some(ECRAN), "la sixième case compte");
        assert!(CASES.iter().all(|&c| doublon_de(&defauts[c], &defauts, c).is_none()), "les défauts de la prod ne se doublent pas");
    }

    #[test]
    fn captures_raccourci_doublon_contre_chaque_autre_case() {
        let clavier = |c: &str| Declencheur::Clavier { combinaison: c.into() };
        let cases = [clavier("Ctrl+Alt+S"), Declencheur::Souris { combinaison: "Alt+Bouton5".into() }, clavier("Ctrl+Alt+R"), Declencheur::Aucun, Declencheur::Aucun, Declencheur::Aucun];
        assert_eq!(doublon_de(&clavier("Ctrl+Alt+S"), &cases, DERNIERE), Some(PRINCIPAL));
        assert_eq!(doublon_de(&clavier("Ctrl+Alt+R"), &cases, PRINCIPAL), Some(DERNIERE), "la troisième case compte aussi");
        assert_eq!(doublon_de(&cases[SECONDAIRE].clone(), &cases, DERNIERE), Some(SECONDAIRE));
        assert_eq!(doublon_de(&clavier("Ctrl+Alt+R"), &cases, DERNIERE), None, "sa propre combinaison n'est pas un doublon");
        assert_eq!(doublon_de(&Declencheur::Aucun, &[Declencheur::Aucun, Declencheur::Aucun, Declencheur::Aucun, Declencheur::Aucun, Declencheur::Aucun, Declencheur::Aucun], SECONDAIRE), None);
    }

    #[test]
    fn captures_raccourci_doublon_entre_cases() {
        let souris = |c: &str| valider(Declencheur::Souris { combinaison: c.into() }, SECONDAIRE).expect("valide");
        assert!(doublon(&souris("Alt+Bouton5"), &souris("alt+bouton5")), "même combinaison, écritures différentes");
        assert!(!doublon(&souris("Alt+Bouton5"), &souris("Alt+Bouton4")));
        assert!(!doublon(&Declencheur::Aucun, &Declencheur::Aucun), "deux « Aucun » ne se gênent pas");
        let clavier = Declencheur::Clavier { combinaison: "Ctrl+Alt+S".into() };
        assert!(doublon(&clavier, &clavier.clone()));
        assert!(!doublon(&clavier, &Declencheur::Souris { combinaison: "Ctrl+Alt+S".into() }), "un type différent n'est pas un doublon");
    }
}
