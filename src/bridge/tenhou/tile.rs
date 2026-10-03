//! Tenhou tile encoding ↔ mjai tile string conversion.
//!
//! Tenhou represents tiles as integer indices `0..=135`. Each of the 34 tile
//! types occupies 4 consecutive indices (the four physical copies). `index / 4`
//! gives the tile type, `index % 4` is the variant within that type.
//!
//! Tile type layout (`index / 4`):
//! - `0..=8`   — 1m..9m
//! - `9..=17`  — 1p..9p
//! - `18..=26` — 1s..9s
//! - `27..=33` — E, S, W, N, P, F, C
//!
//! Red 5s (赤ドラ) live at exactly indices **16, 52, 88** (the 0-th physical
//! copy of 5m / 5p / 5s respectively) and serialize as `5mr`, `5pr`, `5sr` in
//! mjai. Mirrors the original Akagi Python Tenhou tile converter
//! (`tenhou/utils/converter.py`).

const TILES: [&str; 34] = [
    "1m", "2m", "3m", "4m", "5m", "6m", "7m", "8m", "9m", "1p", "2p", "3p", "4p", "5p", "6p", "7p",
    "8p", "9p", "1s", "2s", "3s", "4s", "5s", "6s", "7s", "8s", "9s", "E", "S", "W", "N", "P", "F",
    "C",
];

/// Convert a single Tenhou tile index to its mjai string representation.
///
/// Indices outside `0..=135` are treated as `'?'` (defensive — Tenhou never
/// emits out-of-range tiles, but malformed traffic shouldn't panic).
pub fn tenhou_to_mjai_one(index: u32) -> String {
    let t = (index / 4) as usize;
    if t >= TILES.len() {
        return "?".to_string();
    }
    let label = TILES[t];
    if matches!(index, 16 | 52 | 88) {
        format!("{label}r")
    } else {
        label.to_string()
    }
}

/// Vector form of [`tenhou_to_mjai_one`].
pub fn tenhou_to_mjai(indices: &[u32]) -> Vec<String> {
    indices.iter().map(|&i| tenhou_to_mjai_one(i)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn man_pin_sou_basics() {
        assert_eq!(tenhou_to_mjai_one(0), "1m");
        assert_eq!(tenhou_to_mjai_one(35), "9m");
        assert_eq!(tenhou_to_mjai_one(36), "1p");
        assert_eq!(tenhou_to_mjai_one(71), "9p");
        assert_eq!(tenhou_to_mjai_one(72), "1s");
        assert_eq!(tenhou_to_mjai_one(107), "9s");
    }

    #[test]
    fn red_doras() {
        assert_eq!(tenhou_to_mjai_one(16), "5mr");
        assert_eq!(tenhou_to_mjai_one(52), "5pr");
        assert_eq!(tenhou_to_mjai_one(88), "5sr");
        // Other 5s remain non-red.
        assert_eq!(tenhou_to_mjai_one(17), "5m");
        assert_eq!(tenhou_to_mjai_one(53), "5p");
        assert_eq!(tenhou_to_mjai_one(89), "5s");
    }

    #[test]
    fn honors() {
        assert_eq!(tenhou_to_mjai_one(108), "E");
        assert_eq!(tenhou_to_mjai_one(112), "S");
        assert_eq!(tenhou_to_mjai_one(116), "W");
        assert_eq!(tenhou_to_mjai_one(120), "N");
        assert_eq!(tenhou_to_mjai_one(124), "P");
        assert_eq!(tenhou_to_mjai_one(128), "F");
        assert_eq!(tenhou_to_mjai_one(132), "C");
    }

    #[test]
    fn out_of_range_is_unknown() {
        assert_eq!(tenhou_to_mjai_one(200), "?");
    }

    #[test]
    fn vector_form() {
        let v = tenhou_to_mjai(&[0, 16, 108]);
        assert_eq!(v, vec!["1m", "5mr", "E"]);
    }
}
