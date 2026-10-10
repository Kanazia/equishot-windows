//! Relit un MP4 par Media Foundation et rend son releve en JSON (ADR-016) :
//! durees, trous, bips a 880 Hz, eclairs du centre. Sert au parcours de bout en
//! bout, sans lecteur a l'ecran : `cargo run --example lire-video -- <mp4>`.
//! Avec des demandes `t:x,y` apres le chemin, rend plutot la couleur de chaque
//! point a chaque instant, `[[r,g,b] | null, ...]` (la bulle de camera, 0.18).

use equishot_lib::video::lecture;

fn main() {
    let Some(chemin) = std::env::args().nth(1) else {
        eprintln!("usage : lire-video <fichier.mp4> [t:x,y ...]");
        std::process::exit(2);
    };
    let chemin = std::path::Path::new(&chemin);
    let demandes: Vec<_> = std::env::args().skip(2).filter_map(|a| lecture::lire_demande(&a)).collect();
    let json = if demandes.is_empty() {
        lecture::relever(chemin).map(|releve| serde_json::to_string(&releve).unwrap_or_default())
    } else {
        lecture::couleurs(chemin, &demandes).map(|c| serde_json::to_string(&c).unwrap_or_default())
    };
    match json {
        Ok(json) => println!("{json}"),
        Err(motif) => {
            eprintln!("{motif}");
            std::process::exit(1);
        }
    }
}
