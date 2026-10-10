//! Declencheur a la souris : lecture d'une combinaison (`Alt+Bouton5`) et
//! decision du crochet `WH_MOUSE_LL`, en fonctions pures.

/// Bits des modificateurs, dans l'ordre de l'ecriture canonique.
pub const CTRL: u8 = 1;
pub const ALT: u8 = 2;
pub const MAJ: u8 = 4;
/// Win tenue : jamais dans une combinaison, elle empeche donc toute correspondance.
pub const WIN: u8 = 8;

/// Boutons permis : 3 (clic de molette), 4 et 5 (boutons lateraux).
const BOUTONS: [(u8, &str); 3] = [(4, "Bouton4"), (5, "Bouton5"), (3, "Molette")];

/// Une combinaison souris, tassee sur 16 bits pour un atomique : modificateurs
/// en haut, bouton en bas. 0 veut dire « rien ».
pub fn tasser(modificateurs: u8, bouton: u8) -> u16 {
    (u16::from(modificateurs) << 8) | u16::from(bouton)
}

/// Lit une combinaison souris et rend `(modificateurs, bouton, ecriture canonique)`.
pub fn canonique(saisie: &str) -> Result<(u8, u8, String), String> {
    let mut modificateurs = 0u8;
    let mut bouton = None;
    for morceau in saisie.trim().split('+').map(str::trim) {
        match morceau.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modificateurs |= CTRL,
            "alt" => modificateurs |= ALT,
            "shift" | "maj" => modificateurs |= MAJ,
            "super" | "win" | "meta" => return Err("La touche Win ne se combine pas à la souris : relâchée seule après le clic, elle ouvrirait le menu Démarrer.".to_string()),
            autre => match BOUTONS.iter().find(|(_, nom)| nom.eq_ignore_ascii_case(autre)) {
                Some(&(numero, _)) if bouton.is_none() => bouton = Some(numero),
                _ => return Err(format!("« {saisie} » n'est pas une combinaison souris reconnue : bouton 4, bouton 5 ou clic de molette.")),
            },
        }
    }
    let Some(bouton) = bouton else {
        return Err(format!("« {saisie} » n'a pas de bouton : bouton 4, bouton 5 ou clic de molette."));
    };
    if modificateurs & (CTRL | ALT) == 0 {
        return Err("Il faut tenir Ctrl ou Alt avec le bouton : seul, il garde son rôle dans toutes les applications.".to_string());
    }
    let mut morceaux: Vec<&str> = [(CTRL, "Ctrl"), (ALT, "Alt"), (MAJ, "Shift")]
        .iter()
        .filter(|(bit, _)| modificateurs & bit != 0)
        .map(|&(_, nom)| nom)
        .collect();
    let nom = BOUTONS.iter().find(|(numero, _)| *numero == bouton).map(|&(_, nom)| nom).unwrap_or("Bouton4");
    morceaux.push(nom);
    Ok((modificateurs, bouton, morceaux.join("+")))
}

/// Ce que le crochet fait du message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Rendu a la chaine des crochets : l'application le recoit.
    Passer,
    /// Avale sans rien ouvrir : le relachement d'un appui avale.
    Avaler,
    /// Avale, et declenche l'action de la case.
    Declencher(usize),
}

const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;
const WM_XBUTTONDOWN: u32 = 0x020B;
const WM_XBUTTONUP: u32 = 0x020C;

/// Le bouton d'un message souris, et s'il s'agit d'un appui. `mouse_data` est
/// le champ de `MSLLHOOKSTRUCT` : son mot haut dit quel bouton lateral.
pub fn bouton_du_message(message: u32, mouse_data: u32) -> Option<(u8, bool)> {
    let lateral = match mouse_data >> 16 {
        1 => Some(4),
        2 => Some(5),
        _ => None,
    };
    match message {
        WM_MBUTTONDOWN => Some((3, true)),
        WM_MBUTTONUP => Some((3, false)),
        WM_XBUTTONDOWN => lateral.map(|b| (b, true)),
        WM_XBUTTONUP => lateral.map(|b| (b, false)),
        _ => None,
    }
}

/// Decide du sort d'un appui ou d'un relachement. `avales` porte un bit par
/// bouton dont l'appui a ete avale : son relachement l'est aussi, quels que
/// soient les modificateurs tenus a ce moment, et meme si le reglage a change
/// entre-temps. Un appui passe laisse passer son relachement.
pub fn decider(bouton: u8, appui: bool, tenus: u8, reglees: [u16; super::raccourci::NB_CASES], suspendu: bool, avales: &mut u8) -> Decision {
    let bit = 1u8 << bouton;
    if !appui {
        if *avales & bit != 0 {
            *avales &= !bit;
            return Decision::Avaler;
        }
        return Decision::Passer;
    }
    // Un nouvel appui efface ce qui restait du precedent : un relachement rate
    // (crochet retire par Windows entre-temps) n'avale pas celui de cet appui-ci.
    *avales &= !bit;
    let combinaison = tasser(tenus, bouton);
    if let Some(case) = reglees.iter().position(|&r| r != 0 && r == combinaison).filter(|_| !suspendu) {
        *avales |= bit;
        return Decision::Declencher(case);
    }
    Decision::Passer
}

// ---------------------------------------------------------------------------
// Le crochet vivant
// ---------------------------------------------------------------------------
//
// Un fil a lui, avec sa boucle de messages : Windows appelle la procedure en
// envoyant un message a ce fil (doc de `LowLevelMouseProc`). La procedure voit
// chaque mouvement de souris de la session : elle ne lit que des atomiques,
// n'alloue rien, ne prend aucun verrou, et poste a son propre fil ce qui prend
// du temps (masque, ouverture de la selection).

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU16, AtomicU32, AtomicU64, AtomicU8, Ordering};

/// Les combinaisons des trois cases (principal, secondaire, derniere zone), 0
/// si la case n'est pas a la souris.
static REGLEES: [AtomicU16; super::raccourci::NB_CASES] = [const { AtomicU16::new(0) }; super::raccourci::NB_CASES];
/// L'enregistreur des reglages ecoute : rien ne declenche.
static SUSPENDU: AtomicBool = AtomicBool::new(false);
/// Un bit par bouton dont l'appui a ete avale. Ecrit par la seule procedure.
static AVALES: AtomicU8 = AtomicU8::new(0);
/// Derniere position vue par la procedure, x en haut, y en bas.
static DERNIER: AtomicU64 = AtomicU64::new(0);
/// Duree maximale d'un appel de la procedure, en nanosecondes (sonde dev).
static DUREE_MAX_NS: AtomicU64 = AtomicU64::new(0);
/// Fil du crochet, 0 tant qu'il n'est pas lance.
static FIL: AtomicU32 = AtomicU32::new(0);
/// Poignee du crochet tel que le fil le croit pose, 0 sinon. Ecrite par le seul fil.
static CROCHET: AtomicIsize = AtomicIsize::new(0);
/// Windows a refuse de poser le crochet.
static REFUSE: AtomicBool = AtomicBool::new(false);
/// Crochets reposes par le chien de garde depuis le lancement.
static REPOSES: AtomicU32 = AtomicU32::new(0);

const WM_DECLENCHER: u32 = 0x8000 + 1; // WM_APP + 1
const WM_METTRE_A_JOUR: u32 = 0x8000 + 2;
const WM_RETIRER_SANS_DIRE: u32 = 0x8000 + 3;
/// Toutes les 2 s, le chien de garde compare le curseur a la derniere position vue.
const GARDE_MS: u32 = 2000;
/// Touche sans effet, frappee a l'appui avale : sans elle, Alt relache seul fait
/// entrer l'application au premier plan dans son menu (mesure 2, `docs/TESTS.md`).
const VK_MASQUE: u16 = 0xE8;

fn tasser_point(x: i32, y: i32) -> u64 {
    (u64::from(x as u32) << 32) | u64::from(y as u32)
}

/// Regle la combinaison d'une case (0 : pas a la souris) et pose ou retire le crochet.
pub fn regler(case: usize, combinaison: u16) {
    REGLEES[case].store(combinaison, Ordering::SeqCst);
    poster(WM_METTRE_A_JOUR);
}

/// Pendant l'ecoute de l'enregistreur, aucune combinaison ne declenche.
pub fn suspendre(suspendu: bool) {
    SUSPENDU.store(suspendu, Ordering::SeqCst);
}

/// Windows a refuse le crochet (au dernier essai).
pub fn refuse() -> bool {
    REFUSE.load(Ordering::SeqCst)
}

fn poster(message: u32) {
    poster_case(message, 0);
}

/// `case` part dans `wParam` : la boucle sait quelle case a declenche.
fn poster_case(message: u32, case: usize) {
    let fil = FIL.load(Ordering::SeqCst);
    #[cfg(windows)]
    if fil != 0 {
        // SAFETY: message sans pointeur a un fil de ce processus ; un fil qui
        // n'existe plus fait seulement echouer l'appel.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(fil, message, case, 0) };
    }
    #[cfg(not(windows))]
    let _ = (fil, message, case);
}

/// Ce que dit la sonde de l'instance dev.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sonde {
    pub pose: bool,
    pub refuse: bool,
    pub reposes: u32,
    pub duree_max_us: f64,
}

pub fn sonde() -> Sonde {
    Sonde {
        pose: CROCHET.load(Ordering::SeqCst) != 0,
        refuse: refuse(),
        reposes: REPOSES.load(Ordering::SeqCst),
        duree_max_us: DUREE_MAX_NS.load(Ordering::SeqCst) as f64 / 1000.0,
    }
}

/// Sonde dev : remet la duree maximale a zero.
pub fn oublier_duree() {
    DUREE_MAX_NS.store(0, Ordering::SeqCst);
}

/// Sonde dev : retire le crochet comme Windows le fait apres un depassement de
/// delai, sans que le fil le sache.
pub fn retirer_sans_dire() {
    poster(WM_RETIRER_SANS_DIRE);
}

/// Lance le fil du crochet, une fois. Rend la main quand le fil a sa file de
/// messages : un reglage poste juste apres n'est pas perdu.
pub fn demarrer(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        if FIL.load(Ordering::SeqCst) != 0 {
            return;
        }
        let app = app.clone();
        let (pret, attendre) = std::sync::mpsc::channel();
        let lance = std::thread::Builder::new().name("crochet-souris".into()).spawn(move || crochet::boucle(app, pret));
        if lance.is_ok() {
            let _ = attendre.recv_timeout(std::time::Duration::from_secs(2));
        } else {
            REFUSE.store(true, Ordering::SeqCst);
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
mod crochet {
    use super::*;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetCursorPos, GetMessageW, PeekMessageW, SetTimer, SetWindowsHookExW, UnhookWindowsHookEx, MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, WH_MOUSE_LL, WM_TIMER,
    };

    fn tenu(vk: u16) -> bool {
        // SAFETY: lecture de l'etat d'une touche, sans pointeur.
        unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
    }

    fn modificateurs_tenus() -> u8 {
        let mut m = 0;
        if tenu(VK_CONTROL) {
            m |= CTRL;
        }
        if tenu(VK_MENU) {
            m |= ALT;
        }
        if tenu(VK_SHIFT) {
            m |= MAJ;
        }
        if tenu(VK_LWIN) || tenu(VK_RWIN) {
            m |= WIN;
        }
        m
    }

    unsafe extern "system" fn procedure(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let debut = std::time::Instant::now();
            // SAFETY: pour WH_MOUSE_LL et code >= 0, lparam pointe un MSLLHOOKSTRUCT valide le temps de l'appel.
            let infos = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
            DERNIER.store(tasser_point(infos.pt.x, infos.pt.y), Ordering::Relaxed);
            let mut sort = Decision::Passer;
            if let Some((bouton, appui)) = bouton_du_message(wparam as u32, infos.mouseData) {
                let mut avales = AVALES.load(Ordering::Relaxed);
                let reglees = REGLEES.each_ref().map(|r| r.load(Ordering::Relaxed));
                sort = decider(bouton, appui, modificateurs_tenus(), reglees, SUSPENDU.load(Ordering::Relaxed), &mut avales);
                AVALES.store(avales, Ordering::Relaxed);
                if let Decision::Declencher(case) = sort {
                    poster_case(WM_DECLENCHER, case);
                }
            }
            DUREE_MAX_NS.fetch_max(debut.elapsed().as_nanos() as u64, Ordering::Relaxed);
            if sort != Decision::Passer {
                return 1;
            }
        }
        // SAFETY: transmission telle quelle a la chaine des crochets.
        unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
    }

    fn curseur() -> Option<u64> {
        let mut p = POINT { x: 0, y: 0 };
        // SAFETY: p est un POINT ecrit par l'appel.
        (unsafe { GetCursorPos(&mut p) } != 0).then(|| tasser_point(p.x, p.y))
    }

    fn poser() {
        // SAFETY: crochet global de bas niveau, procedure de ce module ; hmod est ce module.
        let poignee = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(procedure), GetModuleHandleW(null_mut()), 0) };
        REFUSE.store(poignee.is_null(), Ordering::SeqCst);
        if let Some(position) = curseur() {
            DERNIER.store(position, Ordering::Relaxed);
        }
        CROCHET.store(poignee as isize, Ordering::SeqCst);
    }

    fn retirer() {
        let poignee = CROCHET.swap(0, Ordering::SeqCst);
        if poignee != 0 {
            // SAFETY: poignee rendue par SetWindowsHookExW ; deja retiree par Windows, l'appel echoue sans effet.
            unsafe { UnhookWindowsHookEx(poignee as _) };
        }
    }

    fn voulu() -> bool {
        REGLEES.iter().any(|r| r.load(Ordering::SeqCst) != 0)
    }

    fn masquer() {
        let touche = |drapeaux| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_MASQUE, wScan: 0, dwFlags: drapeaux, time: 0, dwExtraInfo: 0 } },
        };
        let entrees = [touche(0), touche(KEYEVENTF_KEYUP)];
        // SAFETY: tableau de deux INPUT valides, taille exacte.
        unsafe { SendInput(entrees.len() as u32, entrees.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    }

    pub fn boucle(app: tauri::AppHandle, pret: std::sync::mpsc::Sender<()>) {
        // SAFETY: appels sans pointeur, ou pointeur vers un MSG local.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            // Cree la file de messages du fil avant que quiconque y poste.
            PeekMessageW(&mut msg, null_mut(), 0, 0, PM_NOREMOVE);
            FIL.store(GetCurrentThreadId(), Ordering::SeqCst);
            SetTimer(null_mut(), 0, GARDE_MS, None);
            if voulu() {
                poser();
            }
            let _ = pret.send(());
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                match msg.message {
                    WM_DECLENCHER => {
                        masquer();
                        crate::captures::declencher(&app, msg.wParam);
                    }
                    WM_METTRE_A_JOUR => match (voulu(), CROCHET.load(Ordering::SeqCst) != 0) {
                        (true, false) => poser(),
                        (false, true) => retirer(),
                        _ => {}
                    },
                    WM_RETIRER_SANS_DIRE => {
                        let poignee = CROCHET.load(Ordering::SeqCst);
                        if poignee != 0 {
                            UnhookWindowsHookEx(poignee as _);
                        }
                    }
                    WM_TIMER if CROCHET.load(Ordering::SeqCst) != 0 => {
                        // Le curseur a bouge sans que la procedure le voie : Windows a
                        // retire le crochet (delai depasse), ou une application a
                        // deplace le curseur par SetCursorPos (mesure 4). Reposer un
                        // crochet vivant ne coute rien.
                        // ponytail: comparaison de position seule, faux positif sur
                        // SetCursorPos ; croiser avec GetLastInputInfo si le journal s'emballe.
                        if curseur().is_some_and(|p| p != DERNIER.load(Ordering::Relaxed)) {
                            retirer();
                            poser();
                            REPOSES.fetch_add(1, Ordering::SeqCst);
                            eprintln!("capture_crochet_repose : curseur déplacé sans que la procédure le voie, crochet reposé");
                        }
                    }
                    // Windows a refuse le crochet au dernier essai : il est retente a chaque tour de garde.
                    WM_TIMER if voulu() && CROCHET.load(Ordering::SeqCst) == 0 => poser(),
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_souris_ecriture_canonique() {
        assert_eq!(canonique("alt+bouton5").expect("lu"), (ALT, 5, "Alt+Bouton5".to_string()));
        assert_eq!(canonique("Shift+Ctrl+Bouton5").expect("lu").2, "Ctrl+Shift+Bouton5", "ordre canonique");
        assert_eq!(canonique("Ctrl+Alt+Molette").expect("lu"), (CTRL | ALT, 3, "Ctrl+Alt+Molette".to_string()));
        for relue in ["Alt+Bouton4", "Ctrl+Shift+Bouton5", "Alt+Shift+Molette"] {
            assert_eq!(canonique(relue).expect("relu").2, relue, "l'écriture persistée se relit");
        }
    }

    #[test]
    fn captures_souris_combinaisons_refusees() {
        assert!(canonique("Bouton5").is_err(), "bouton seul");
        assert!(canonique("Shift+Bouton5").is_err(), "Maj seule");
        assert!(canonique("Super+Bouton5").is_err(), "Win");
        assert!(canonique("Alt+Win+Bouton4").is_err(), "Win avec Alt");
        assert!(canonique("Alt+Gauche").is_err(), "clic gauche");
        assert!(canonique("Alt+Droit").is_err(), "clic droit");
        assert!(canonique("Alt+Bouton6").is_err());
        assert!(canonique("Alt+Bouton4+Bouton5").is_err(), "deux boutons");
        assert!(canonique("Ctrl+Alt").is_err(), "pas de bouton");
        assert!(canonique("").is_err());
    }

    #[test]
    fn captures_souris_bouton_du_message() {
        assert_eq!(bouton_du_message(WM_XBUTTONDOWN, 1 << 16), Some((4, true)));
        assert_eq!(bouton_du_message(WM_XBUTTONUP, 2 << 16), Some((5, false)));
        assert_eq!(bouton_du_message(WM_MBUTTONDOWN, 0), Some((3, true)));
        assert_eq!(bouton_du_message(0x0201, 0), None, "clic gauche");
        assert_eq!(bouton_du_message(0x0204, 0), None, "clic droit");
        assert_eq!(bouton_du_message(0x0200, 0), None, "mouvement");
        assert_eq!(bouton_du_message(WM_XBUTTONDOWN, 3 << 16), None, "bouton lateral inconnu");
    }

    #[test]
    fn captures_souris_appui_et_relachement_avales_ensemble() {
        let reglees = [tasser(ALT, 4), 0, 0, 0, 0, 0];
        let mut avales = 0;
        assert_eq!(decider(4, true, ALT, reglees, false, &mut avales), Decision::Declencher(0));
        // Alt deja relache, ou Maj ajoutee : le relachement reste avale.
        assert_eq!(decider(4, false, 0, reglees, false, &mut avales), Decision::Avaler);
        assert_eq!(avales, 0);
        // Un relachement sans appui avale passe.
        assert_eq!(decider(4, false, ALT, reglees, false, &mut avales), Decision::Passer);
    }

    #[test]
    fn captures_souris_relachement_rate_n_avale_pas_le_suivant() {
        // Relecture du 3 octobre 2026 : le crochet retire par Windows rate le
        // relachement d'un appui avale ; le bit restait leve et le prochain
        // bouton 5 seul perdait son relachement.
        let reglees = [tasser(ALT, 5), 0, 0, 0, 0, 0];
        let mut avales = 0;
        assert_eq!(decider(5, true, ALT, reglees, false, &mut avales), Decision::Declencher(0));
        // ... relachement jamais vu ...
        assert_eq!(decider(5, true, 0, reglees, false, &mut avales), Decision::Passer, "bouton 5 seul passe");
        assert_eq!(decider(5, false, 0, reglees, false, &mut avales), Decision::Passer, "et son relâchement aussi");
    }

    #[test]
    fn captures_souris_modificateurs_exacts() {
        let reglees = [tasser(ALT, 4), tasser(CTRL | MAJ, 5), 0, 0, 0, 0];
        let mut avales = 0;
        for (bouton, tenus) in [(4, ALT | MAJ), (4, 0), (5, ALT), (4, ALT | WIN), (5, CTRL), (3, ALT)] {
            assert_eq!(decider(bouton, true, tenus, reglees, false, &mut avales), Decision::Passer, "bouton {bouton}, tenus {tenus}");
            assert_eq!(decider(bouton, false, tenus, reglees, false, &mut avales), Decision::Passer, "son relâchement passe aussi");
        }
        assert_eq!(decider(5, true, CTRL | MAJ, reglees, false, &mut avales), Decision::Declencher(1), "la seconde case déclenche");
    }

    #[test]
    fn captures_souris_la_troisieme_case_se_reconnait() {
        let reglees = [tasser(ALT, 4), 0, tasser(CTRL, 5), tasser(CTRL | MAJ, 4), 0, 0];
        let mut avales = 0;
        assert_eq!(decider(5, true, CTRL, reglees, false, &mut avales), Decision::Declencher(2), "la dernière zone, pas la sélection");
        assert_eq!(decider(5, false, CTRL, reglees, false, &mut avales), Decision::Avaler);
        assert_eq!(decider(4, true, ALT, reglees, false, &mut avales), Decision::Declencher(0));
        assert_eq!(decider(4, false, ALT, reglees, false, &mut avales), Decision::Avaler);
        assert_eq!(decider(4, true, CTRL | MAJ, reglees, false, &mut avales), Decision::Declencher(3), "la quatrième case, le texte d'une zone");
    }

    #[test]
    fn captures_souris_suspendu_ou_rien_de_regle() {
        let mut avales = 0;
        assert_eq!(decider(4, true, ALT, [tasser(ALT, 4), 0, 0, 0, 0, 0], true, &mut avales), Decision::Passer, "enregistreur en écoute");
        assert_eq!(decider(4, true, ALT, [0, 0, 0, 0, 0, 0], false, &mut avales), Decision::Passer, "aucune case à la souris");
        assert_eq!(decider(0, true, 0, [0, 0, 0, 0, 0, 0], false, &mut avales), Decision::Passer, "0 ne correspond jamais à rien");
        // Suspendu entre l'appui et le relachement : le relachement reste avale.
        assert_eq!(decider(4, true, ALT, [tasser(ALT, 4), 0, 0, 0, 0, 0], false, &mut avales), Decision::Declencher(0));
        assert_eq!(decider(4, false, ALT, [tasser(ALT, 4), 0, 0, 0, 0, 0], true, &mut avales), Decision::Avaler);
    }
}
