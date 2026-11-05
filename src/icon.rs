//! Icône de barre de menus générée en code (modèle monochrome : trois barres).

use tray_icon::Icon;

const SIZE: u32 = 36;

pub fn pixels() -> Vec<u8> {
    let mut px = vec![0u8; (SIZE * SIZE * 4) as usize];
    // (x début, hauteur) pour trois barres façon jauge.
    let bars = [(5u32, 14u32), (14, 24), (23, 18)];
    for (x0, h) in bars {
        for y in (SIZE - 6 - h)..(SIZE - 6) {
            for x in x0..x0 + 8 {
                let i = ((y * SIZE + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
    }
    px
}

pub fn make() -> Option<Icon> {
    Icon::from_rgba(pixels(), SIZE, SIZE).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_is_valid() {
        assert_eq!(pixels().len(), (SIZE * SIZE * 4) as usize);
        assert!(make().is_some());
        assert!(pixels().chunks(4).any(|p| p[3] == 255));
    }
}
