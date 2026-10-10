//! Le GIF d'une video (ADR-016, 0.16) : les 30 premieres secondes au plus, 10
//! images par seconde, 800 px de large au plus, une palette par image. Pour
//! les messageries et les tickets, ou un MP4 ne se lit pas toujours en ligne.

use std::path::Path;

use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};

pub const IPS: u32 = 10;
pub const DUREE_MAX_S: f64 = 30.0;
pub const LARGEUR_MAX: u32 = 800;

/// La taille du GIF : celle de la video, reduite a 800 px de large au plus.
pub fn taille(largeur: u32, hauteur: u32) -> (u32, u32) {
    if largeur <= LARGEUR_MAX {
        return (largeur, hauteur);
    }
    (LARGEUR_MAX, ((hauteur as f64 * LARGEUR_MAX as f64 / largeur as f64).round() as u32).max(1))
}

/// Ecrit les images en GIF qui boucle, 1/10 s chacune. Rend leur nombre.
pub fn ecrire(images: impl IntoIterator<Item = RgbaImage>, chemin: &Path) -> Result<u32, String> {
    let erreur = |e: image::ImageError| format!("Écriture du GIF {} impossible : {e}", chemin.display());
    let fichier = std::fs::File::create(chemin).map_err(|e| format!("Création de {} impossible : {e}", chemin.display()))?;
    // Vitesse 10 : la quantification la plus legere qui garde un texte lisible.
    let mut encodeur = GifEncoder::new_with_speed(std::io::BufWriter::new(fichier), 10);
    encodeur.set_repeat(Repeat::Infinite).map_err(erreur)?;
    let mut n = 0;
    for image in images {
        encodeur.encode_frame(Frame::from_parts(image, 0, 0, Delay::from_numer_denom_ms(1000 / IPS, 1))).map_err(erreur)?;
        n += 1;
    }
    Ok(n)
}

/// Le GIF de la video `mp4`, ecrit en `gif`. Bloquant, sur un fil a priorite
/// basse : la machine ne doit pas geler pendant la quantification.
pub fn depuis_video(mp4: &Path, gif: &Path) -> Result<u32, String> {
    let _basse = PrioriteBasse::poser();
    let mut images = Vec::new();
    super::lecture::images(mp4, 1.0 / IPS as f64, DUREE_MAX_S, |image| {
        let (l, h) = taille(image.width(), image.height());
        images.push(if (l, h) == image.dimensions() { image } else { image::imageops::resize(&image, l, h, image::imageops::FilterType::Triangle) });
    })?;
    if images.is_empty() {
        return Err("Aucune image dans cette vidéo.".to_string());
    }
    let ecrit = ecrire(images, gif);
    if ecrit.is_err() {
        let _ = std::fs::remove_file(gif);
    }
    ecrit
}

/// Le fil courant passe sous la priorite normale, et y revient a la fin.
struct PrioriteBasse;

impl PrioriteBasse {
    fn poser() -> Self {
        #[cfg(windows)]
        // SAFETY: le pseudo-handle du fil courant, toujours valide.
        unsafe {
            use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL);
        }
        PrioriteBasse
    }
}

impl Drop for PrioriteBasse {
    fn drop(&mut self) {
        #[cfg(windows)]
        // SAFETY: idem.
        unsafe {
            use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_NORMAL};
            SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_NORMAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::AnimationDecoder;

    #[test]
    fn gif_taille_bornee_a_800() {
        assert_eq!(taille(640, 360), (640, 360));
        assert_eq!(taille(800, 600), (800, 600));
        assert_eq!(taille(3840, 1080), (800, 225));
        assert_eq!(taille(1601, 1), (800, 1));
    }

    #[test]
    fn gif_ecrit_relu_image_par_image() {
        let chemin = std::env::temp_dir().join(format!("equishot-gif-test-{}.gif", std::process::id()));
        let images = (0..5u8).map(|i| RgbaImage::from_pixel(40, 20, image::Rgba([i * 50, 0, 0, 255])));
        assert_eq!(ecrire(images, &chemin).expect("ecrit"), 5);
        let lu = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(std::fs::File::open(&chemin).expect("ouvert"))).expect("decodeur");
        let images: Vec<_> = lu.into_frames().collect::<Result<_, _>>().expect("relu");
        assert_eq!(images.len(), 5);
        assert_eq!(images[0].buffer().dimensions(), (40, 20));
        assert_eq!(images[0].delay().numer_denom_ms(), (100, 1));
        assert!(images[4].buffer().get_pixel(10, 10)[0] >= 190, "la derniere image est la plus rouge");
        let _ = std::fs::remove_file(&chemin);
    }
}
