// Pas de console en release : EquiShot vit dans sa pastille et la zone de notification.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    equishot_lib::run()
}
