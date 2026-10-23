//! Java Cryptography Architecture algorithm names (`MessageDigest`,
//! `Cipher`, `Mac`, `Signature`, `KeyGenerator` transformations), shared by
//! the Java and Kotlin rule tables.

/// The weak algorithm or mode a JCA transformation names (`"MD5"`,
/// `"DES/CBC/PKCS5Padding"`, `"AES/ECB/NoPadding"`). An HMAC (`"HmacSHA1"`)
/// does not rely on its hash's collision resistance.
pub(crate) fn weak_jca_algorithm(transformation: &str) -> Option<&'static str> {
    let upper = transformation.to_ascii_uppercase();
    let mut parts = upper.split('/');
    let algorithm = parts.next().unwrap_or_default();
    let mode = parts.next();
    match algorithm {
        "MD2" | "MD4" | "MD5" | "MD5WITHRSA" => Some("MD5"),
        "SHA1" | "SHA-1" | "SHA" | "SHA1WITHRSA" | "SHA1WITHDSA" => Some("SHA-1"),
        "DES" | "DESEDE" | "TRIPLEDES" => Some("DES"),
        "RC2" | "RC4" | "ARCFOUR" => Some("RC4"),
        "BLOWFISH" => Some("Blowfish"),
        // `Cipher.getInstance("AES")` is AES/ECB/PKCS5Padding.
        _ if mode == Some("ECB") || (algorithm == "AES" && mode.is_none()) => Some("ECB mode"),
        _ => None,
    }
}
