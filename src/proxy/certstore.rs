//! The genuine server certificates Akagi saw on the upstream leg.
//!
//! When Akagi intercepts TLS it terminates the client's connection with a
//! certificate of its own — but it also makes the real connection, and on
//! that leg it is handed the origin's real certificate. `upstream.rs`
//! validates the chain and hostname before recording it in this store.
//!
//! The one consumer today is [`crate::proxy::rewrite`]. See that module
//! for why a client's certificate report is worth correcting.
//!
//! ## Field formatting
//!
//! The stored fields are rendered the way **.NET's `X509Certificate2`**
//! renders them, because that is the shape the Unity clients report and
//! anything else would stand out more than the original problem:
//!
//! - `issuer` / `subject` — RDNs in **reverse** DER order, `", "`-joined,
//!   using .NET's short names (note `S`, not `ST`, for
//!   stateOrProvinceName). Verified against a real capture: a DER order of
//!   C, O, OU, CN prints as `CN=…, OU=…, O=…, C=…`.
//! - `thumbprint` — uppercase hex SHA-1 of the whole DER.
//! - `serial_number` — uppercase hex of the serial integer's bytes.
//! - `not_before` / `not_after` — `M/d/yyyy h:mm:ss tt` in **local time**,
//!   which is what the en-US default `ToString()` produces.
//!
//! Two known imprecisions, neither observed in practice:
//! values containing `,` `+` `"` or `=` are not quoted the way .NET would
//! quote them, and a serial whose DER encoding carries a leading `0x00`
//! sign byte is reproduced with that byte included.

use anyhow::{anyhow, Result};
use chrono::{Local, TimeZone, Utc};
use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::sync::Mutex;
use x509_parser::der_parser::oid::Oid;
use x509_parser::prelude::*;

/// What a .NET client would report about one certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedCert {
    pub issuer: String,
    pub subject: String,
    pub version: u32,
    pub oid_value: String,
    pub oid_friendly_name: String,
    pub thumbprint: String,
    pub serial_number: String,
    pub not_before: String,
    pub not_after: String,
}

/// .NET's short names for the attribute types that appear in a
/// distinguished name. Anything else falls back to its dotted OID, which
/// is also what .NET does.
fn attribute_short_name(oid: &Oid<'_>) -> String {
    let dotted = oid.to_id_string();
    match dotted.as_str() {
        "2.5.4.3" => "CN",
        "2.5.4.4" => "SN",
        "2.5.4.5" => "SERIALNUMBER",
        "2.5.4.6" => "C",
        "2.5.4.7" => "L",
        // .NET prints stateOrProvinceName as `S`, not the `ST` most other
        // tooling uses.
        "2.5.4.8" => "S",
        "2.5.4.9" => "STREET",
        "2.5.4.10" => "O",
        "2.5.4.11" => "OU",
        "2.5.4.12" => "T",
        "2.5.4.42" => "G",
        "0.9.2342.19200300.100.1.25" => "DC",
        "1.2.840.113549.1.9.1" => "E",
        _ => return dotted,
    }
    .to_string()
}

/// Render a DN the way .NET does: reversed, `", "`-joined.
fn distinguished_name(name: &X509Name<'_>) -> String {
    let mut rdns: Vec<String> =
        name.iter_rdn()
            .map(|rdn| {
                // A multi-valued RDN keeps its own order and joins with `+`.
                rdn.iter()
                    .map(|attr| {
                        let value = attr.as_str().map(str::to_string).unwrap_or_else(|_| {
                            String::from_utf8_lossy(attr.as_slice()).into_owned()
                        });
                        format!("{}={}", attribute_short_name(attr.attr_type()), value)
                    })
                    .collect::<Vec<_>>()
                    .join("+")
            })
            .collect();
    rdns.reverse();
    rdns.join(", ")
}

/// Format a certificate timestamp the way an en-US .NET client does.
///
/// Local time, not UTC: the client prints `DateTime` values that the
/// framework has already converted. Akagi runs on the same machine as the
/// client, so "local" means the same thing to both.
fn dotnet_datetime(unix_seconds: i64) -> String {
    let Some(utc) = Utc.timestamp_opt(unix_seconds, 0).single() else {
        return String::new();
    };
    // `%-m`/`%-d`/`%-I` drop the leading zero, matching `M/d/yyyy h:mm:ss tt`.
    Local
        .from_utc_datetime(&utc.naive_utc())
        .format("%-m/%-d/%Y %-I:%M:%S %p")
        .to_string()
}

/// Public-key algorithm, as .NET's `PublicKey.Oid` reports it.
fn key_algorithm(oid: &Oid<'_>) -> (String, String) {
    let dotted = oid.to_id_string();
    let friendly = match dotted.as_str() {
        "1.2.840.113549.1.1.1" => "RSA",
        "1.2.840.10045.2.1" => "ECC",
        "1.2.840.10040.4.1" => "DSA",
        _ => "",
    };
    (dotted, friendly.to_string())
}

impl ObservedCert {
    /// Parse a DER certificate into the fields a client would report.
    pub fn from_der(der: &[u8]) -> Result<Self> {
        let (_, cert) =
            X509Certificate::from_der(der).map_err(|e| anyhow!("parse certificate: {e}"))?;

        let (oid_value, oid_friendly_name) = key_algorithm(&cert.public_key().algorithm.algorithm);

        let mut hasher = Sha1::new();
        hasher.update(der);
        let thumbprint = hex_upper(&hasher.finalize());

        Ok(Self {
            issuer: distinguished_name(cert.issuer()),
            subject: distinguished_name(cert.subject()),
            // `X509Version` is zero-based on the wire: v3 is stored as 2.
            version: cert.version().0 + 1,
            oid_value,
            oid_friendly_name,
            thumbprint,
            serial_number: hex_upper(cert.raw_serial()),
            not_before: dotnet_datetime(cert.validity().not_before.timestamp()),
            not_after: dotnet_datetime(cert.validity().not_after.timestamp()),
        })
    }
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Certificates observed on the upstream leg, keyed by the host we asked
/// for (the SNI, which is the URI host).
#[derive(Debug, Default)]
pub struct CertStore {
    certs: Mutex<HashMap<String, ObservedCert>>,
}

impl CertStore {
    /// Record what an origin served. Called from the TLS verifier, so it
    /// must never fail the handshake: an unparseable certificate is
    /// dropped and the connection proceeds.
    pub fn record(&self, host: &str, der: &[u8]) {
        match ObservedCert::from_der(der) {
            Ok(cert) => {
                self.certs
                    .lock()
                    .expect("cert store poisoned")
                    .insert(host.to_ascii_lowercase(), cert);
            }
            Err(e) => tracing::debug!("could not parse upstream certificate for {host}: {e:#}"),
        }
    }

    pub fn get(&self, host: &str) -> Option<ObservedCert> {
        self.certs
            .lock()
            .expect("cert store poisoned")
            .get(&host.to_ascii_lowercase())
            .cloned()
    }

    pub fn len(&self) -> usize {
        self.certs.lock().expect("cert store poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::time::{Duration, OffsetDateTime};
    use hudsucker::rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SerialNumber};

    /// Build a self-signed certificate with known fields to parse back.
    fn sample_der(serial: u64) -> Vec<u8> {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        let mut dn = DistinguishedName::new();
        // Pushed in DER order C, O, OU, CN — the order a real CA uses, and
        // the one whose reversal we need to reproduce.
        dn.push(DnType::CountryName, "US");
        dn.push(DnType::OrganizationName, "Example Inc");
        dn.push(DnType::OrganizationalUnitName, "www.example.com");
        dn.push(DnType::CommonName, "Example TLS CA");
        params.distinguished_name = dn;
        params.serial_number = Some(SerialNumber::from(serial));
        params.not_before = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        params.not_after = params.not_before + Duration::days(365);
        params.self_signed(&key).unwrap().der().to_vec()
    }

    /// The field that matters most: a DN must come back reversed, exactly
    /// as .NET renders it. A real capture showed DER order C, O, OU, CN
    /// printing as `CN=…, OU=…, O=…, C=…`.
    #[test]
    fn distinguished_names_are_reversed_like_dotnet() {
        let cert = ObservedCert::from_der(&sample_der(1)).unwrap();
        assert_eq!(
            cert.subject,
            "CN=Example TLS CA, OU=www.example.com, O=Example Inc, C=US"
        );
        // Self-signed, so the issuer is the same name.
        assert_eq!(cert.issuer, cert.subject);
    }

    #[test]
    fn thumbprint_is_uppercase_sha1_of_the_der() {
        let der = sample_der(2);
        let cert = ObservedCert::from_der(&der).unwrap();
        let mut h = Sha1::new();
        h.update(&der);
        let expected: String = h.finalize().iter().map(|b| format!("{b:02X}")).collect();
        assert_eq!(cert.thumbprint, expected);
        assert_eq!(cert.thumbprint.len(), 40);
        assert!(cert
            .thumbprint
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase()));
    }

    #[test]
    fn serial_is_uppercase_hex() {
        let cert = ObservedCert::from_der(&sample_der(0x47265EB4E9BADFF6)).unwrap();
        assert_eq!(cert.serial_number, "47265EB4E9BADFF6");
    }

    #[test]
    fn version_is_one_based_like_dotnet() {
        // On the wire v3 is encoded as 2; clients report 3.
        assert_eq!(ObservedCert::from_der(&sample_der(3)).unwrap().version, 3);
    }

    #[test]
    fn key_algorithm_is_reported_as_the_client_would() {
        // rcgen's default key is ECDSA P-256.
        let cert = ObservedCert::from_der(&sample_der(4)).unwrap();
        assert_eq!(cert.oid_value, "1.2.840.10045.2.1");
        assert_eq!(cert.oid_friendly_name, "ECC");
        // The other half of the pair, which real gateway certificates use.
        let (oid, friendly) = key_algorithm(&oid_registry::OID_PKCS1_RSAENCRYPTION);
        assert_eq!(oid, "1.2.840.113549.1.1.1");
        assert_eq!(friendly, "RSA");
    }

    /// `M/d/yyyy h:mm:ss tt` — no leading zeros on month, day or hour,
    /// and an uppercase AM/PM. Asserted against a fixed timezone so the
    /// test says the same thing on every machine.
    #[test]
    fn datetime_matches_the_dotnet_short_format() {
        // 2026-07-31T22:38:07Z
        let s = dotnet_datetime(1_785_537_487);
        let re_shaped = s
            .split(['/', ' ', ':'])
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>();
        assert_eq!(
            re_shaped.len(),
            7,
            "expected M/d/yyyy h:mm:ss tt, got {s:?}"
        );
        assert!(
            re_shaped[6] == "AM" || re_shaped[6] == "PM",
            "expected an AM/PM marker, got {s:?}"
        );
        // No leading zero on the month or the day.
        assert!(!re_shaped[0].starts_with('0'), "{s:?}");
        assert!(!re_shaped[1].starts_with('0'), "{s:?}");
        assert_eq!(re_shaped[2].len(), 4, "four-digit year: {s:?}");
        // Minutes and seconds keep theirs.
        assert_eq!(re_shaped[4].len(), 2, "{s:?}");
        assert_eq!(re_shaped[5].len(), 2, "{s:?}");
    }

    #[test]
    fn store_is_case_insensitive_on_host() {
        let store = CertStore::default();
        store.record("Route-6.Maj-Soul.com", &sample_der(5));
        assert!(store.get("route-6.maj-soul.com").is_some());
        assert!(store.get("ROUTE-6.MAJ-SOUL.COM").is_some());
        assert!(store.get("route-5.maj-soul.com").is_none());
    }

    /// The verifier calls this on the TLS handshake path, so garbage must
    /// be dropped rather than propagated.
    #[test]
    fn unparseable_certificate_is_ignored() {
        let store = CertStore::default();
        store.record("example.com", b"not a certificate");
        assert!(store.is_empty());
    }
}
