//! Les quelques appels Win32 qu'EquiShot fait lui-meme : lire le premier plan,
//! et le rendre a la fenetre qui l'avait. La croix du curseur vient seule a
//! l'ouverture de la selection (mesure 1, `docs/TESTS.md`).

/// Poignee de la fenetre au premier plan, 0 s'il n'y en a pas.
pub fn premier_plan() -> isize {
    #[cfg(windows)]
    {
        // SAFETY: appel sans argument, qui rend une poignee ou nul.
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow() as isize }
    }
    #[cfg(not(windows))]
    0
}

/// Rend le premier plan a `poignee`. Possible depuis EquiShot tant que l'une de
/// ses fenetres l'a (la selection, juste avant de se fermer).
pub fn rendre_premier_plan(poignee: isize) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{IsWindow, SetForegroundWindow};
        if poignee == 0 {
            return;
        }
        // SAFETY: une poignee peut etre perimee ; IsWindow le dit sans risque.
        unsafe {
            if IsWindow(poignee as _) != 0 {
                SetForegroundWindow(poignee as _);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = poignee;
}

/// Pose le curseur en `(x, y)`, pixels physiques du bureau : les fleches de la
/// selection (ADR-008). Le mouvement qui en nait passe a la page comme celui de la souris.
pub fn poser_curseur(x: i32, y: i32) {
    #[cfg(windows)]
    // SAFETY: appel sans pointeur ; Windows borne lui-meme au bureau.
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetCursorPos(x, y);
    }
    #[cfg(not(windows))]
    let _ = (x, y);
}

/// Le curseur, pixels physiques du bureau : la barre video traversante ne recoit
/// plus d'evenements de souris, Rust juge le survol a sa place (ADR-017).
pub fn curseur() -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        let mut p = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
        // SAFETY: pointeur vers un POINT local valide pendant l'appel.
        let ok = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) };
        (ok != 0).then_some((p.x, p.y))
    }
    #[cfg(not(windows))]
    None
}

/// `crans` crans de molette dans `sens`, sous le curseur : la capture defilante
/// (ADR-015, ADR-019) fait defiler la fenetre sous sa zone, comme ShareX.
/// Positif vers le haut pour WHEEL, vers la droite pour HWHEEL (doc de MOUSEINPUT).
pub fn molette(sens: crate::captures::defilante::Sens, crans: i32) {
    #[cfg(windows)]
    {
        use crate::captures::defilante::Sens;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_WHEEL, MOUSEINPUT};
        let (quantite, drapeau) = match sens {
            Sens::Bas => (-120 * crans, MOUSEEVENTF_WHEEL),
            Sens::Haut => (120 * crans, MOUSEEVENTF_WHEEL),
            Sens::Droite => (120 * crans, MOUSEEVENTF_HWHEEL),
            Sens::Gauche => (-120 * crans, MOUSEEVENTF_HWHEEL),
        };
        let entree = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: quantite as u32, dwFlags: drapeau, time: 0, dwExtraInfo: 0 },
            },
        };
        // SAFETY: une entree valide, sa taille exacte.
        unsafe {
            SendInput(1, &entree, std::mem::size_of::<INPUT>() as i32);
        }
    }
    #[cfg(not(windows))]
    let _ = (sens, crans);
}

/// Montre ou cache une fenetre sans l'activer. Le `show()` de tao fait `SW_SHOW`,
/// qui donne le premier plan a la fenetre des qu'EquiShot en a le droit (juste
/// apres le raccourci) : le `WS_EX_NOACTIVATE` d'une fenetre `focusable(false)`
/// n'arrete que le clic (parcours du 4 octobre 2026, `docs/TESTS.md`).
/// ponytail: tao ne voit pas ce changement de visibilite ; une fenetre montree ici
/// se montre et se cache toujours ici, jamais par `show()` ni `hide()`.
///
/// Montree, elle repasse aussi en tete de la couche « toujours devant » : toutes
/// les fenetres montrees ici sont `always_on_top`, et `SW_SHOWNOACTIVATE` ne touche
/// pas a l'ordre. La pile des vignettes de la production en etait sortie (sous la
/// galerie et 70 fenetres ordinaires, 2026-10-10, cause non trouvee) et chaque
/// capture la remontrait derriere tout. Mesure du meme jour : `HWND_TOPMOST` sur
/// une fenetre deja visible rend vrai et ne fait rien ; cachee puis montree, si.
/// Le cycle ne se fait donc que si une fenetre ordinaire est passee devant elle.
pub fn montrer_sans_activer<R: tauri::Runtime>(fenetre: &tauri::WebviewWindow<R>, visible: bool) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            IsWindowVisible, SetWindowPos, ShowWindowAsync, HWND_TOPMOST, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
            SW_SHOWNOACTIVATE,
        };
        let Ok(poignee) = fenetre.hwnd() else { return };
        let poignee = poignee.0 as _;
        // SAFETY: poignee d'une fenetre de ce processus ; les appels se postent a son fil, dans l'ordre, sans l'attendre.
        unsafe {
            if !visible {
                ShowWindowAsync(poignee, SW_HIDE);
                return;
            }
            if IsWindowVisible(poignee) != 0 && sous_une_ordinaire(poignee) {
                ShowWindowAsync(poignee, SW_HIDE);
            }
            ShowWindowAsync(poignee, SW_SHOWNOACTIVATE);
            SetWindowPos(poignee, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS);
        }
    }
    #[cfg(not(windows))]
    let _ = if visible { fenetre.show() } else { fenetre.hide() };
}

/// Vrai si une fenetre visible hors de la couche « toujours devant » est au-dessus
/// de `poignee` : elle est sortie de cette couche, quel que soit son drapeau
/// `WS_EX_TOPMOST` (pose sur la pile de la production qui en etait sortie).
#[cfg(windows)]
fn sous_une_ordinaire(poignee: windows_sys::Win32::Foundation::HWND) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindow, GetWindowLongPtrW, IsWindowVisible, GWL_EXSTYLE, GW_HWNDPREV, WS_EX_TOPMOST};
    // SAFETY: appels de lecture ; une poignee perimee rend nul ou zero, sans risque.
    unsafe {
        let mut dessus = GetWindow(poignee, GW_HWNDPREV);
        while !dessus.is_null() {
            if IsWindowVisible(dessus) != 0 && GetWindowLongPtrW(dessus, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST == 0 {
                return true;
            }
            dessus = GetWindow(dessus, GW_HWNDPREV);
        }
    }
    false
}

/// Alt relache seul sur une fenetre d'EquiShot y active le menu systeme
/// (`WM_SYSCOMMAND`, `SC_KEYMENU` sans lettre), et la fenetre se fige. Le
/// raccourci a la souris laisse Alt se repeter sur la selection qu'il ouvre :
/// elle passait en mode menu et ne repondait plus (parcours du 3 octobre 2026,
/// `docs/TESTS.md`). La fenetre ignore ce menu-la ; Alt + une lettre n'est pas touche.
pub fn sans_menu_alt<R: tauri::Runtime>(fenetre: &tauri::WebviewWindow<R>) {
    #[cfg(windows)]
    {
        let Ok(poignee) = fenetre.hwnd() else { return };
        let poignee = poignee.0 as isize;
        // Le sous-classement se pose depuis le fil qui possede la fenetre.
        let _ = fenetre.run_on_main_thread(move || {
            // SAFETY: poignee d'une fenetre de ce processus, sur son fil ; la procedure est statique.
            unsafe { windows_sys::Win32::UI::Shell::SetWindowSubclass(poignee as _, Some(sans_menu), 1, 0) };
        });
    }
    #[cfg(not(windows))]
    let _ = fenetre;
}

/// Barre d'enregistrement : non activable (`true`), puis traversante ou non au survol.
/// Pas par `set_focusable` ni `set_ignore_cursor_events` : leur `apply_diff` (tao 0.35.3,
/// `window_state.rs:325-424`) fait `SW_HIDE` sur une fenetre que tao croit cachee et
/// `SW_SHOW`, qui l'active, sur une visible. On pose ici les memes styles que tao.
pub fn non_activable<R: tauri::Runtime>(fenetre: &tauri::WebviewWindow<R>) {
    #[cfg(windows)]
    styles_etendus(fenetre, windows_sys::Win32::UI::WindowsAndMessaging::WS_EX_NOACTIVATE, 0);
    #[cfg(not(windows))]
    let _ = fenetre.set_focusable(false);
}

pub fn traversante<R: tauri::Runtime>(fenetre: &tauri::WebviewWindow<R>, oui: bool) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{WS_EX_LAYERED, WS_EX_TRANSPARENT};
        let styles = WS_EX_TRANSPARENT | WS_EX_LAYERED;
        if oui {
            styles_etendus(fenetre, styles, 0);
        } else {
            styles_etendus(fenetre, 0, styles);
        }
    }
    #[cfg(not(windows))]
    let _ = fenetre.set_ignore_cursor_events(oui);
}

#[cfg(windows)]
fn styles_etendus<R: tauri::Runtime>(fenetre: &tauri::WebviewWindow<R>, poser: u32, retirer: u32) {
    let Ok(poignee) = fenetre.hwnd() else { return };
    let poignee = poignee.0 as isize;
    let _ = fenetre.run_on_main_thread(move || {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        };
        // SAFETY: poignee d'une fenetre de ce processus, sur son fil.
        unsafe {
            let avant = GetWindowLongPtrW(poignee as _, GWL_EXSTYLE);
            SetWindowLongPtrW(poignee as _, GWL_EXSTYLE, (avant & !(retirer as isize)) | poser as isize);
            SetWindowPos(poignee as _, std::ptr::null_mut(), 0, 0, 0, 0, SWP_NOZORDER | SWP_NOMOVE | SWP_NOSIZE | SWP_FRAMECHANGED | SWP_NOACTIVATE);
        }
    });
}

/// Chromium (Brave, Chrome) cesse de dessiner une fenetre qu'il croit couverte
/// (`IsWindowVisibleAndFullyOpaque`, `ui/gfx/win/hwnd_util.cc`) : la selection,
/// plein ecran et transparente, noircissait une video. Il ignore les fenetres
/// `WS_EX_TOOLWINDOW`. tao reecrit tout le style etendu a chaque changement
/// (`show()` compris) : le style est rajoute a chaque `WM_STYLECHANGING`.
pub fn hors_occlusion<R: tauri::Runtime>(fenetre: &tauri::WebviewWindow<R>) {
    #[cfg(windows)]
    {
        let Ok(poignee) = fenetre.hwnd() else { return };
        let poignee = poignee.0 as isize;
        let _ = fenetre.run_on_main_thread(move || {
            use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE};
            // SAFETY: poignee d'une fenetre de ce processus, sur son fil ; la procedure est statique.
            unsafe {
                windows_sys::Win32::UI::Shell::SetWindowSubclass(poignee as _, Some(outil), 2, 0);
                // Repasse par `WM_STYLECHANGING`, donc par `outil`.
                SetWindowLongPtrW(poignee as _, GWL_EXSTYLE, GetWindowLongPtrW(poignee as _, GWL_EXSTYLE));
            }
        });
    }
    #[cfg(not(windows))]
    let _ = fenetre;
}

/// La selection va se fermer. A la destruction, WebView2 detruit sa fenetre
/// enfant ; si la selection est encore active, Windows lui rend le focus, et
/// wry traite ce `WM_SETFOCUS` avec une WebView deja liberee : pointeur nul,
/// l'exe tombe (`wry` 0.55.1, `webview2/mod.rs:1256`, parcours du 4 octobre
/// 2026). Marquee ici, la selection avale ce message. A appeler avant `close()`
/// et `destroy()` : les deux passent, dans l'ordre, par le fil principal.
pub fn avant_fermeture<R: tauri::Runtime>(fenetre: &tauri::WebviewWindow<R>) {
    #[cfg(windows)]
    {
        let Ok(poignee) = fenetre.hwnd() else { return };
        let poignee = poignee.0 as isize;
        let _ = fenetre.run_on_main_thread(move || {
            // SAFETY: meme fenetre, meme procedure, meme identifiant : seule la donnee change.
            unsafe { windows_sys::Win32::UI::Shell::SetWindowSubclass(poignee as _, Some(outil), 2, FERMETURE) };
        });
    }
    #[cfg(not(windows))]
    let _ = fenetre;
}

/// Donnee du sous-classement `outil` : la fenetre se ferme.
#[cfg(windows)]
const FERMETURE: usize = 1;

#[cfg(windows)]
unsafe extern "system" fn outil(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    donnee: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GWL_EXSTYLE, STYLESTRUCT, WM_NCDESTROY, WM_SETFOCUS, WM_STYLECHANGING, WS_EX_TOOLWINDOW};
    if message == WM_SETFOCUS && donnee == FERMETURE {
        return 0;
    }
    if message == WM_STYLECHANGING && wparam as i32 == GWL_EXSTYLE {
        // SAFETY: pour WM_STYLECHANGING, lparam pointe un STYLESTRUCT modifiable.
        unsafe { (*(lparam as *mut STYLESTRUCT)).styleNew |= WS_EX_TOOLWINDOW };
    }
    if message == WM_NCDESTROY {
        // SAFETY: meme fenetre, meme procedure, meme identifiant qu'a la pose.
        unsafe { windows_sys::Win32::UI::Shell::RemoveWindowSubclass(hwnd, Some(outil), 2) };
    }
    // SAFETY: transmission telle quelle a la procedure suivante.
    unsafe { windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(windows)]
unsafe extern "system" fn sans_menu(
    hwnd: windows_sys::Win32::Foundation::HWND,
    message: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _donnee: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SC_KEYMENU, WM_NCDESTROY, WM_SYSCOMMAND};
    if message == WM_SYSCOMMAND && (wparam as u32 & 0xFFF0) == SC_KEYMENU && lparam == 0 {
        return 0;
    }
    if message == WM_NCDESTROY {
        // La doc de SetWindowSubclass demande de retirer le sous-classement avant la destruction.
        // SAFETY: meme fenetre, meme procedure, meme identifiant qu'a la pose.
        unsafe { windows_sys::Win32::UI::Shell::RemoveWindowSubclass(hwnd, Some(sans_menu), 1) };
    }
    // SAFETY: transmission telle quelle a la procedure suivante.
    unsafe { windows_sys::Win32::UI::Shell::DefSubclassProc(hwnd, message, wparam, lparam) }
}
