//! Names for devices behind a receiver whose USB IDs several brands share. The battery
//! reply doesn't say which device it is, so all that tells them apart is the receiver's product string: "AJAZZ 2.4G
//! 8K" on the AJAZZ AJ179 Pro, "2.4G Wireless Mouse" on the Attack Shark R3, both on 3151:402D (issue #22).

use crate::loc::{t, tf};

/// Words that say how a device is connected or what kind it is, not whose it is.
const PLAIN_WORDS: &[&str] = &[
    "2.4G", "2.4GHz", "1K", "2K", "4K", "8K", "USB", "HID", "Wireless", "Receiver", "Dongle", "Mouse", "Keyboard",
    "Device", "Gaming", "Optical", "Nano", "NK", "Nearlink", "HS", "3-mode", "Compx", "dongle-L", "dongle-H",
];

/// What the product string says besides such words: "AJAZZ 2.4G 8K" -> "AJAZZ", "2.4G Wireless Mouse" -> "".
pub fn brand(product: Option<&str>) -> String {
    product
        .unwrap_or_default()
        .split_whitespace()
        .filter(|w| !PLAIN_WORDS.iter().any(|p| p.eq_ignore_ascii_case(w)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The name of the mouse behind a receiver with this product string. One word is taken for a brand and shown as
/// "AJAZZ mouse", more are shown as they are ("MCHOSE G7 Pro"). A string that names nobody gives `fallback`, or just
/// "Mouse".
pub fn mouse(product: Option<&str>, fallback: Option<&str>) -> String {
    let brand = brand(product);
    if brand.is_empty() {
        return fallback.map(str::to_string).unwrap_or_else(|| t("name.mouse"));
    }
    if brand.contains(' ') {
        brand
    } else {
        tf("name.brandMouse", &[&brand])
    }
}

/// The same for a keyboard: "AJAZZ keyboard", "Neo Ergo", or `fallback`.
pub fn keyboard(product: Option<&str>, fallback: Option<&str>) -> String {
    let brand = brand(product);
    if brand.is_empty() {
        return fallback.map(str::to_string).unwrap_or_else(|| t("name.keyboard"));
    }
    if brand.contains(' ') {
        brand
    } else {
        tf("name.brandKeyboard", &[&brand])
    }
}

#[cfg(test)]
mod tests {
    //! "AJAZZ 2.4G 8K" is the AJAZZ AJ179 Pro's receiver 3151:402D (issue #22), "2.4G Wireless Mouse" the Attack
    //! Shark R3's with the same IDs, "AJAZZ 2.4G" the AJ139 V2 MC's A8A5:2255 (issue #20).
    use super::*;

    #[test]
    fn brand_is_what_is_left_without_the_plain_words() {
        assert_eq!(brand(Some("AJAZZ 2.4G 8K")), "AJAZZ");
        assert_eq!(brand(Some("AJAZZ 2.4G")), "AJAZZ");
        assert_eq!(brand(Some("2.4G Wireless Mouse")), "");
        assert_eq!(brand(Some("2.4G Wireless Receiver")), "");
        assert_eq!(brand(Some("  usb   DONGLE ")), "");
        assert_eq!(brand(Some("2.4G Wireless Gaming Mouse")), "");
        assert_eq!(brand(Some("USB Gaming Keyboard")), "");
        assert_eq!(brand(Some("MCHOSE G7 Pro")), "MCHOSE G7 Pro");
        assert_eq!(brand(Some("MonsGeek 2.4G Wireless Keyboard")), "MonsGeek");
        assert_eq!(brand(Some("")), "");
        assert_eq!(brand(None), "");
        // What receivers of ATK and CompX call themselves
        assert_eq!(brand(Some("Wireless mouse 8k NANO dongle-L")), "");
        assert_eq!(brand(Some("Compx 2.4G Wireless 8K")), "");
        assert_eq!(brand(Some("Nearlink Mouse Dongle")), "");
    }

    #[test]
    fn one_word_is_a_brand() {
        assert_eq!(mouse(Some("AJAZZ 2.4G 8K"), Some("Attack Shark R3")), "AJAZZ mouse");
    }

    #[test]
    fn several_words_are_the_name() {
        assert_eq!(mouse(Some("MCHOSE G7 Pro"), None), "MCHOSE G7 Pro");
    }

    #[test]
    fn a_string_that_names_nobody_gives_the_fallback() {
        assert_eq!(mouse(Some("2.4G Wireless Mouse"), Some("Attack Shark R3")), "Attack Shark R3");
        assert_eq!(mouse(None, Some("Attack Shark R3")), "Attack Shark R3");
    }

    #[test]
    fn without_a_fallback_it_is_just_a_mouse() {
        assert_eq!(mouse(Some("2.4G Wireless Receiver"), None), "Mouse");
    }
}
