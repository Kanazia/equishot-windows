//! Capture defilante (ADR-015) : la zone relue pendant que la page defile, et
//! chaque vue recollee sous les precedentes. Le recollage est une fonction pure,
//! testee sur une page synthetique dont chaque ligne est connue.

use image::RgbaImage;

/// Ce qu'une nouvelle vue a donne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pas {
    /// La page a defile de `d` lignes, recollees.
    Decale(u32),
    /// Rien n'a bouge : la fin de la page, ou une page qui ne defile pas.
    Immobile,
    /// Aucun decalage ne recolle la vue : elle est sautee.
    Introuvable,
}

/// La bande ecartee de chaque cote d'une ligne avant de la comparer : la barre de
/// defilement y vit. Celle de ShareX, avec un plancher plus bas pour les zones etroites.
pub fn bande_laterale(largeur: u32) -> u32 {
    (largeur / 20).max(16).min(largeur / 3)
}

/// L'empreinte de chaque ligne de la vue, sans ses bandes laterales (FNV-1a sur
/// les octets : rapide meme dans l'exe de debug).
pub fn empreintes(vue: &RgbaImage) -> Vec<u64> {
    let (largeur, hauteur) = vue.dimensions();
    let bande = bande_laterale(largeur) as usize * 4;
    let ligne = largeur as usize * 4;
    let octets = vue.as_raw();
    (0..hauteur as usize)
        .map(|y| {
            let debut = y * ligne;
            octets[debut + bande..debut + ligne - bande]
                .iter()
                .fold(0xcbf2_9ce4_8422_2325u64, |h, &o| (h ^ o as u64).wrapping_mul(0x100_0000_01b3))
        })
        .collect()
}

/// Le decalage `d` tel que la ligne `r` de `n` soit la ligne `r + d` de `p`, en
/// dehors de l'en-tete et du pied fixes ; et la hauteur `b` de ce pied. Plusieurs
/// decalages possibles (zone unie, motif repete) : le plus proche de `prefere`,
/// le plus petit sans preference.
pub fn decalage(p: &[u64], n: &[u64], prefere: Option<u32>) -> Result<(u32, u32), Pas> {
    let h = p.len().min(n.len());
    let t = (0..h).take_while(|&y| p[y] == n[y]).count();
    if t == h {
        return Err(Pas::Immobile);
    }
    let b = (0..h).rev().take_while(|&y| p[y] == n[y]).count();
    let bande = h - t - b;
    let recouvrement = (bande / 8).max(8);
    // ponytail: recherche en O(bande²) au pire, qui s'arrete au premier ecart ; une vue fait au plus la hauteur de l'ecran.
    let valides = (1..bande.saturating_sub(recouvrement) + 1).filter(|&d| (t..h - b - d).all(|r| n[r] == p[r + d]));
    let choisi = match prefere {
        Some(q) => valides.min_by_key(|&d| (d as i64 - q as i64).abs()),
        None => valides.min(),
    };
    choisi.map(|d| (d as u32, b as u32)).ok_or(Pas::Introuvable)
}

/// Le sens du defilement (ADR-019). Le recollage travaille toujours « vers le
/// bas » : chaque vue y est ramenee, l'image recollee en revient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sens {
    Bas,
    Haut,
    Droite,
    Gauche,
}

/// Les quatre sens, dans l'ordre ou la detection les essaie.
pub const TOUS: [Sens; 4] = [Sens::Bas, Sens::Haut, Sens::Droite, Sens::Gauche];

fn retournee(a: &RgbaImage) -> RgbaImage {
    let h = a.height();
    RgbaImage::from_fn(a.width(), h, |x, y| *a.get_pixel(x, h - 1 - y))
}

fn transposee(a: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(a.height(), a.width(), |x, y| *a.get_pixel(y, x))
}

/// La vue ramenee dans le repere « vers le bas » : retournee vers le haut,
/// transposee vers la droite, les deux vers la gauche.
pub fn vers_bas(vue: &RgbaImage, sens: Sens) -> RgbaImage {
    match sens {
        Sens::Bas => vue.clone(),
        Sens::Haut => retournee(vue),
        Sens::Droite => transposee(vue),
        Sens::Gauche => retournee(&transposee(vue)),
    }
}

/// L'inverse exact de `vers_bas`.
pub fn depuis_bas(image: &RgbaImage, sens: Sens) -> RgbaImage {
    match sens {
        Sens::Bas => image.clone(),
        Sens::Haut => retournee(image),
        Sens::Droite => transposee(image),
        Sens::Gauche => transposee(&retournee(image)),
    }
}

/// L'image recollee, et la derniere vue qui y est entree. Tout y est dans le
/// repere « vers le bas » de `sens`.
pub struct Recollage {
    sens: Sens,
    largeur: u32,
    /// Lignes RGBA du resultat, bout a bout.
    pixels: Vec<u8>,
    precedente: Vec<u64>,
    dernier: Option<u32>,
}

impl Recollage {
    pub fn nouveau(premiere: &RgbaImage, sens: Sens) -> Self {
        let premiere = vers_bas(premiere, sens);
        Recollage { sens, largeur: premiere.width(), precedente: empreintes(&premiere), pixels: premiere.into_raw(), dernier: None }
    }

    /// La taille de l'image sur l'axe du defilement.
    pub fn longueur(&self) -> u32 {
        (self.pixels.len() / (self.largeur as usize * 4)) as u32
    }

    /// Recolle `vue` (de la taille de la premiere) : le pied de la vue precedente
    /// est retire, puis le bas de `vue` ajoute ; le resultat finit donc toujours
    /// sur le pied de la derniere vue. Rien ne change hors d'un `Decale`.
    pub fn ajouter(&mut self, vue: &RgbaImage) -> Pas {
        let vue = &vers_bas(vue, self.sens);
        let nouvelles = empreintes(vue);
        let (d, b) = match decalage(&self.precedente, &nouvelles, self.dernier) {
            Ok(trouve) => trouve,
            Err(pas) => return pas,
        };
        let ligne = self.largeur as usize * 4;
        let h = vue.height() as usize;
        self.pixels.truncate(self.pixels.len() - b as usize * ligne);
        self.pixels.extend_from_slice(&vue.as_raw()[(h - b as usize - d as usize) * ligne..]);
        self.precedente = nouvelles;
        self.dernier = Some(d);
        Pas::Decale(d)
    }

    /// L'image recollee, dans le repere de l'ecran.
    pub fn image(self) -> RgbaImage {
        let longueur = self.longueur();
        depuis_bas(&RgbaImage::from_raw(self.largeur, longueur, self.pixels).expect("des lignes entieres"), self.sens)
    }
}

/// Le mode manuel (ADR-019) : le sens n'est pas connu d'avance. Les quatre
/// recollages partent de la meme premiere vue ; le premier qui recolle est
/// garde, les autres sont jetes.
pub struct Detection {
    candidats: Vec<(Sens, Recollage)>,
    /// La derniere vue lue, recollee ou non.
    lue: Vec<u8>,
}

impl Detection {
    pub fn nouvelle(premiere: &RgbaImage) -> Self {
        Detection { candidats: TOUS.iter().map(|&s| (s, Recollage::nouveau(premiere, s))).collect(), lue: premiere.as_raw().clone() }
    }

    /// Le sens garde, `None` tant que rien n'a recolle.
    #[cfg(test)]
    pub fn sens(&self) -> Option<Sens> {
        (self.candidats.len() == 1).then(|| self.candidats[0].0)
    }

    /// Un recollage qui ne rend pas `Decale` ne change pas : les essayer tous
    /// ne coute aucune copie. Une vue n'entre que relue identique : la zone est lue
    /// pendant que la page se repeint, et une vue dechiree (le haut deja defile, le
    /// bas pas encore peint) recollerait un bas perime. En attendant : `Immobile`.
    pub fn ajouter(&mut self, vue: &RgbaImage) -> Pas {
        if vue.as_raw() != &self.lue {
            self.lue = vue.as_raw().clone();
            return Pas::Immobile;
        }
        if self.candidats.len() == 1 {
            return self.candidats[0].1.ajouter(vue);
        }
        let mut immobiles = true;
        for i in 0..self.candidats.len() {
            match self.candidats[i].1.ajouter(vue) {
                Pas::Decale(d) => {
                    let garde = self.candidats.swap_remove(i);
                    self.candidats = vec![garde];
                    return Pas::Decale(d);
                }
                Pas::Introuvable => immobiles = false,
                Pas::Immobile => {}
            }
        }
        if immobiles { Pas::Immobile } else { Pas::Introuvable }
    }

    pub fn longueur(&self) -> u32 {
        self.candidats[0].1.longueur()
    }

    pub fn image(mut self) -> RgbaImage {
        self.candidats.swap_remove(0).1.image()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: u32 = 200;
    /// Colonnes de la barre de defilement simulee, a droite.
    const BARRE: u32 = 10;

    /// Une page dont chaque ligne porte son numero ; `blanches` est une bande
    /// blanche (debut, longueur).
    fn page(hauteur: u32, blanches: Option<(u32, u32)>) -> RgbaImage {
        RgbaImage::from_fn(L, hauteur, |x, y| {
            if blanches.is_some_and(|(debut, n)| y >= debut && y < debut + n) {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([(y & 255) as u8, (y >> 8) as u8, (x * 7 % 251) as u8, 255])
            }
        })
    }

    /// La vue d'une fenetre de `h` lignes ouverte sur `page` a `defile` lignes du
    /// haut, avec un en-tete et un pied fixes et une barre qui bouge.
    fn vue(page: &RgbaImage, defile: u32, h: u32, entete: u32, pied: u32) -> RgbaImage {
        RgbaImage::from_fn(L, h, |x, y| {
            if x >= L - BARRE {
                image::Rgba([90, (defile % 256) as u8, 90, 255])
            } else if y < entete {
                image::Rgba([10, 200, 30, 255])
            } else if y >= h - pied {
                image::Rgba([200, 30, 10 + (x % 7) as u8, 255])
            } else {
                *page.get_pixel(x, defile + y - entete)
            }
        })
    }

    /// Recolle les vues prises aux defilements donnes ; rend l'image et les pas.
    fn recoller(page: &RgbaImage, defilements: &[u32], h: u32, entete: u32, pied: u32) -> (RgbaImage, Vec<Pas>) {
        let mut r = Recollage::nouveau(&vue(page, defilements[0], h, entete, pied), Sens::Bas);
        let pas = defilements[1..].iter().map(|&d| r.ajouter(&vue(page, d, h, entete, pied))).collect();
        (r.image(), pas)
    }

    /// L'image attendue : l'en-tete, la page jusqu'au bas de la derniere vue, le
    /// pied ; comparee hors de la barre.
    fn attendue(page: &RgbaImage, dernier: u32, h: u32, entete: u32, pied: u32) -> RgbaImage {
        let contenu = h - entete - pied;
        RgbaImage::from_fn(L, entete + dernier + contenu + pied, |x, y| {
            if y < entete {
                image::Rgba([10, 200, 30, 255])
            } else if y >= entete + dernier + contenu {
                image::Rgba([200, 30, 10 + (x % 7) as u8, 255])
            } else {
                *page.get_pixel(x, y - entete)
            }
        })
    }

    fn identiques_hors_barre(a: &RgbaImage, b: &RgbaImage) -> Result<(), String> {
        if a.dimensions() != b.dimensions() {
            return Err(format!("{:?} contre {:?}", a.dimensions(), b.dimensions()));
        }
        for y in 0..a.height() {
            for x in 0..L - BARRE {
                if a.get_pixel(x, y) != b.get_pixel(x, y) {
                    return Err(format!("premier ecart en {x},{y} : {:?} contre {:?}", a.get_pixel(x, y), b.get_pixel(x, y)));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn defilante_bande_laterale_plancher_et_plafond() {
        assert_eq!(bande_laterale(200), 16);
        assert_eq!(bande_laterale(1000), 50);
        assert_eq!(bande_laterale(30), 10);
    }

    #[test]
    fn defilante_page_sans_element_fixe_recollee_exactement() {
        let p = page(1500, None);
        let (image, pas) = recoller(&p, &[0, 150, 300, 450, 600], 400, 0, 0);
        assert_eq!(pas, vec![Pas::Decale(150); 4]);
        identiques_hors_barre(&image, &attendue(&p, 600, 400, 0, 0)).unwrap();
    }

    #[test]
    fn defilante_en_tete_et_pied_fixes_une_seule_fois() {
        let p = page(1500, None);
        let (image, pas) = recoller(&p, &[0, 120, 240, 360], 400, 40, 30);
        assert_eq!(pas, vec![Pas::Decale(120); 3]);
        identiques_hors_barre(&image, &attendue(&p, 360, 400, 40, 30)).unwrap();
    }

    #[test]
    fn defilante_bande_blanche_et_pas_variables() {
        let p = page(1500, Some((500, 120)));
        let (image, pas) = recoller(&p, &[0, 100, 137, 347, 447, 547, 600], 400, 40, 30);
        assert_eq!(pas, vec![Pas::Decale(100), Pas::Decale(37), Pas::Decale(210), Pas::Decale(100), Pas::Decale(100), Pas::Decale(53)]);
        identiques_hors_barre(&image, &attendue(&p, 600, 400, 40, 30)).unwrap();
    }

    #[test]
    fn defilante_vue_immobile_est_la_fin() {
        let p = page(1500, None);
        let (image, pas) = recoller(&p, &[0, 150, 150], 400, 40, 30);
        assert_eq!(pas, vec![Pas::Decale(150), Pas::Immobile]);
        identiques_hors_barre(&image, &attendue(&p, 150, 400, 40, 30)).unwrap();
    }

    #[test]
    fn defilante_vue_etrangere_sautee_puis_la_suite_recollee() {
        let p = page(1500, None);
        let autre = RgbaImage::from_fn(L, 400, |x, y| image::Rgba([(x * 3 % 256) as u8, (y * 5 % 256) as u8, 77, 255]));
        let mut r = Recollage::nouveau(&vue(&p, 0, 400, 0, 0), Sens::Bas);
        assert_eq!(r.ajouter(&autre), Pas::Introuvable);
        assert_eq!(r.longueur(), 400);
        assert_eq!(r.ajouter(&vue(&p, 150, 400, 0, 0)), Pas::Decale(150));
        identiques_hors_barre(&r.image(), &attendue(&p, 150, 400, 0, 0)).unwrap();
    }

    #[test]
    fn defilante_recouvrement_trop_court_refuse() {
        let p = page(1500, None);
        let mut r = Recollage::nouveau(&vue(&p, 0, 400, 0, 0), Sens::Bas);
        // 395 lignes de defilement : 5 lignes communes, sous le minimum de 50.
        assert_eq!(r.ajouter(&vue(&p, 395, 400, 0, 0)), Pas::Introuvable);
    }

    #[test]
    fn defilante_contenu_periodique_le_pas_le_plus_proche_du_precedent() {
        // Une page qui se repete toutes les 50 lignes : 20, 70, 120 et 170 recollent.
        let p: Vec<u64> = (0..400).map(|y| y % 50).collect();
        let n: Vec<u64> = (0..400).map(|y| (y + 120) % 50).collect();
        assert_eq!(decalage(&p, &n, Some(118)), Ok((120, 0)));
        assert_eq!(decalage(&p, &n, None), Ok((20, 0)));
    }

    /// Une image non carree dont chaque pixel dit sa place.
    fn reperee(l: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(l, h, |x, y| image::Rgba([x as u8, y as u8, (x * 3 + y * 7) as u8, 255]))
    }

    #[test]
    fn defilante_repere_aller_retour_non_carre() {
        let a = reperee(37, 23);
        for sens in [Sens::Bas, Sens::Haut, Sens::Droite, Sens::Gauche] {
            let b = vers_bas(&a, sens);
            let attendu = if matches!(sens, Sens::Droite | Sens::Gauche) { (23, 37) } else { (37, 23) };
            assert_eq!(b.dimensions(), attendu, "{sens:?}");
            assert_eq!(depuis_bas(&b, sens), a, "{sens:?}");
        }
    }

    #[test]
    fn defilante_repere_haut_retourne_droite_transpose() {
        let a = reperee(37, 23);
        assert_eq!(vers_bas(&a, Sens::Haut).get_pixel(5, 0), a.get_pixel(5, 22));
        assert_eq!(vers_bas(&a, Sens::Droite).get_pixel(4, 9), a.get_pixel(9, 4));
        assert_eq!(vers_bas(&a, Sens::Gauche).get_pixel(4, 0), a.get_pixel(36, 4));
    }

    /// La vue de l'ecran pour `sens` : la vue « vers le bas » de `vue`, ramenee a l'ecran.
    fn vue_dans(sens: Sens, page: &RgbaImage, defile: u32, h: u32, entete: u32, pied: u32) -> RgbaImage {
        depuis_bas(&vue(page, defile, h, entete, pied), sens)
    }

    #[test]
    fn defilante_haut_droite_gauche_recolles_exactement() {
        let p = page(1500, Some((500, 120)));
        for sens in [Sens::Haut, Sens::Droite, Sens::Gauche] {
            let mut r = Recollage::nouveau(&vue_dans(sens, &p, 0, 400, 40, 30), sens);
            for d in [100, 137, 347, 447] {
                assert!(matches!(r.ajouter(&vue_dans(sens, &p, d, 400, 40, 30)), Pas::Decale(_)), "{sens:?} {d}");
            }
            assert_eq!(r.longueur(), 40 + 447 + 330 + 30, "{sens:?}");
            let image = vers_bas(&r.image(), sens);
            identiques_hors_barre(&image, &attendue(&p, 447, 400, 40, 30)).unwrap();
        }
    }

    #[test]
    fn defilante_detection_chaque_sens() {
        let p = page(1500, None);
        for sens in [Sens::Bas, Sens::Haut, Sens::Droite, Sens::Gauche] {
            let mut d = Detection::nouvelle(&vue_dans(sens, &p, 0, 400, 40, 30));
            assert_eq!(relue(&mut d, &vue_dans(sens, &p, 0, 400, 40, 30)), Pas::Immobile);
            assert_eq!(d.sens(), None);
            assert_eq!(relue(&mut d, &vue_dans(sens, &p, 120, 400, 40, 30)), Pas::Decale(120), "{sens:?}");
            assert_eq!(d.sens(), Some(sens));
        }
    }

    #[test]
    fn defilante_detection_droite_pas_prise_pour_bas() {
        let p = page(1500, None);
        let mut d = Detection::nouvelle(&vue_dans(Sens::Droite, &p, 0, 400, 0, 0));
        assert_eq!(relue(&mut d, &vue_dans(Sens::Droite, &p, 90, 400, 0, 0)), Pas::Decale(90));
        assert_eq!(d.sens(), Some(Sens::Droite));
    }

    #[test]
    fn defilante_detection_recul_puis_reprise() {
        let p = page(1500, None);
        let mut d = Detection::nouvelle(&vue(&p, 0, 400, 0, 0));
        assert_eq!(relue(&mut d, &vue(&p, 150, 400, 0, 0)), Pas::Decale(150));
        // Retour en arriere : introuvable dans le sens garde, rien n'est ajoute.
        assert_eq!(relue(&mut d, &vue(&p, 60, 400, 0, 0)), Pas::Introuvable);
        assert_eq!(d.sens(), Some(Sens::Bas));
        assert_eq!(d.longueur(), 550);
        // Saut trop grand puis retour pres de la derniere vue recollee : la suite reprend.
        assert_eq!(relue(&mut d, &vue(&p, 900, 400, 0, 0)), Pas::Introuvable);
        assert_eq!(relue(&mut d, &vue(&p, 300, 400, 0, 0)), Pas::Decale(150));
        identiques_hors_barre(&d.image(), &attendue(&p, 300, 400, 0, 0)).unwrap();
    }

    /// La vue lue, puis relue si elle n'a pas ete prise : la boucle relit la zone
    /// toutes les 150 ms, une vue posee est lue au moins deux fois.
    fn relue(d: &mut Detection, v: &RgbaImage) -> Pas {
        match d.ajouter(v) {
            Pas::Immobile => d.ajouter(v),
            pas => pas,
        }
    }

    /// Une vue lue pendant le repeint : le haut a deja defile de `apres - avant`
    /// lignes, le bas montre encore l'ancien contenu (ScrollWindow puis WM_PAINT).
    fn dechiree(page: &RgbaImage, avant: u32, apres: u32, h: u32) -> RgbaImage {
        let (vieille, neuve) = (vue(page, avant, h, 0, 0), vue(page, apres, h, 0, 0));
        let coupure = h - (apres - avant);
        RgbaImage::from_fn(L, h, |x, y| *if y < coupure { &neuve } else { &vieille }.get_pixel(x, y))
    }

    #[test]
    fn defilante_detection_vue_dechiree_pas_recollee() {
        // Parcours complet du 8 octobre 2026 : 120 lignes en double a la main.
        let p = page(1500, None);
        let mut d = Detection::nouvelle(&vue(&p, 0, 400, 0, 0));
        assert_eq!(relue(&mut d, &vue(&p, 150, 400, 0, 0)), Pas::Decale(150));
        // Lue une fois pendant le repeint, puis la page repeinte, relue.
        d.ajouter(&dechiree(&p, 150, 270, 400));
        relue(&mut d, &vue(&p, 270, 400, 0, 0));
        identiques_hors_barre(&d.image(), &attendue(&p, 270, 400, 0, 0)).unwrap();
    }
}
