use anyhow::{Context, Result};
use http::uri::Authority;
use hudsucker::{
    certificate_authority::CertificateAuthority,
    rcgen::{
        string::Ia5String, BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa,
        Issuer, KeyPair, KeyUsagePurpose, SanType,
    },
    rustls::{
        self,
        crypto::CryptoProvider,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
        ServerConfig,
    },
};
use rand::{rng, Rng};
use std::{
    collections::HashMap,
    net::IpAddr,
    path::Path,
    sync::{Arc, Mutex},
};
use time::{Duration, OffsetDateTime};
use tracing::info;

/// Leaf-cert validity window. Mirrors hudsucker's defaults.
const LEAF_TTL_SECS: i64 = 365 * 24 * 60 * 60;
const NOT_BEFORE_OFFSET_SECS: i64 = 60;

const BASENAME: &str = "akagi-ca";
const CERT_PEM_EXTS: &[&str] = &["cer", "crt", "pem"];
const CERT_DER_EXT: &str = "der";
const KEY_PEM_EXT: &str = "key";
const KEY_DER_EXT: &str = "key.der";

pub fn load_or_generate(dir: &Path) -> Result<IpAwareAuthority> {
    crate::util::private_fs::directory(dir)
        .with_context(|| format!("Failed to create CA dir {}", dir.display()))?;

    let cert_pem_path = dir.join(format!("{BASENAME}.cer"));
    let key_pem_path = dir.join(format!("{BASENAME}.{KEY_PEM_EXT}"));

    let cert_exists = std::fs::symlink_metadata(&cert_pem_path).is_ok();
    let key_exists = std::fs::symlink_metadata(&key_pem_path).is_ok();
    if cert_exists != key_exists {
        anyhow::bail!("incomplete existing CA; refusing automatic replacement");
    }
    if !cert_exists && std::fs::read_dir(dir)?.next().is_some() {
        anyhow::bail!("nonempty CA directory without complete CA; refusing automatic replacement");
    }
    let (cert_pem, key_pem) = if cert_exists && key_exists {
        info!("Loading CA from {}", dir.display());
        let cert_pem = crate::util::private_fs::read_and_protect(&cert_pem_path)
            .context("Failed to read CA cert")?;
        let key_pem = crate::util::private_fs::read_and_protect(&key_pem_path)
            .context("Failed to read CA key")?;
        let key_pair_for_der = KeyPair::from_pem(&key_pem).context("Failed to parse CA key")?;
        write_extra_pem_formats(dir, &cert_pem)?;
        write_extra_key_der(dir, &key_pair_for_der.serialize_der())?;
        (cert_pem, key_pem)
    } else {
        info!("Generating new CA at {}", dir.display());
        let (cert_pem, key_pem, cert_der, key_der) = generate_ca()?;
        crate::util::private_fs::write(&cert_pem_path, &cert_pem)
            .context("Failed to write CA cert")?;
        crate::util::private_fs::write(&key_pem_path, &key_pem)
            .context("Failed to write CA key")?;
        write_extra_pem_formats(dir, &cert_pem)?;
        write_cert_der(dir, &cert_der)?;
        write_extra_key_der(dir, &key_der)?;
        (cert_pem, key_pem)
    };

    let key_pair = KeyPair::from_pem(&key_pem).context("Failed to parse CA key")?;
    let issuer =
        Issuer::from_ca_cert_pem(&cert_pem, key_pair).context("Failed to parse CA certificate")?;

    Ok(IpAwareAuthority::new(
        issuer,
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
}

fn write_extra_pem_formats(dir: &Path, cert_pem: &str) -> Result<()> {
    for ext in CERT_PEM_EXTS {
        let p = dir.join(format!("{BASENAME}.{ext}"));
        if !p.exists() {
            crate::util::private_fs::write(&p, cert_pem)
                .with_context(|| format!("Failed to write CA cert {}", p.display()))?;
        }
    }
    Ok(())
}

fn write_cert_der(dir: &Path, cert_der: &[u8]) -> Result<()> {
    let p = dir.join(format!("{BASENAME}.{CERT_DER_EXT}"));
    if !p.exists() {
        crate::util::private_fs::write(&p, cert_der)
            .with_context(|| format!("Failed to write CA cert {}", p.display()))?;
    }
    Ok(())
}

fn write_extra_key_der(dir: &Path, key_der: &[u8]) -> Result<()> {
    let p = dir.join(format!("{BASENAME}.{KEY_DER_EXT}"));
    if p.exists() {
        // Opening validates ownership/type, refuses links, and tightens permissions.
        let _ = crate::util::private_fs::append(&p)?;
    } else {
        crate::util::private_fs::write(&p, key_der).context("Failed to write private CA key")?;
    }
    Ok(())
}

fn generate_ca() -> Result<(String, String, Vec<u8>, Vec<u8>)> {
    let key_pair = KeyPair::generate().context("Failed to generate CA key pair")?;

    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Akagi Proxy CA");
    dn.push(DnType::OrganizationName, "Akagi");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
    ];

    let cert = params
        .self_signed(&key_pair)
        .context("Failed to self-sign CA certificate")?;

    let cert_pem = cert.pem();
    let cert_der = cert.der().to_vec();
    let key_pem = key_pair.serialize_pem();
    let key_der = key_pair.serialize_der();
    Ok((cert_pem, key_pem, cert_der, key_der))
}

/// Build the parameters for a per-host leaf certificate.
///
/// The crucial difference from hudsucker's built-in `RcgenAuthority`: when the
/// host is an IP literal we emit an `iPAddress` SAN, not a `DnsName`. A DNS SAN
/// holding an IP string is rejected by RFC-6125-compliant TLS clients, so
/// MITMing a server reached by raw IP (Riichi City's game server connects to
/// `<ip>:443`) would otherwise fail the handshake.
fn leaf_params(host: &str) -> CertificateParams {
    let mut params = CertificateParams::default();
    params.serial_number = Some(rng().random::<u64>().into());

    let not_before = OffsetDateTime::now_utc() - Duration::seconds(NOT_BEFORE_OFFSET_SECS);
    params.not_before = not_before;
    params.not_after = not_before + Duration::seconds(LEAF_TTL_SECS);

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, host);
    params.distinguished_name = dn;

    let san = match host.parse::<IpAddr>() {
        Ok(ip) => SanType::IpAddress(ip),
        Err(_) => {
            SanType::DnsName(Ia5String::try_from(host).expect("host is not a valid IA5 string"))
        }
    };
    params.subject_alt_names.push(san);
    params
}

/// A hudsucker [`CertificateAuthority`] that issues leaf certs with the correct
/// SAN type for both hostnames and IP literals (the latter is why we can't use
/// the built-in `RcgenAuthority`). Generated configs are cached per host for the
/// lifetime of the proxy.
pub struct IpAwareAuthority {
    issuer: Issuer<'static, KeyPair>,
    private_key: PrivateKeyDer<'static>,
    cache: Mutex<HashMap<String, Arc<ServerConfig>>>,
    provider: Arc<CryptoProvider>,
}

impl IpAwareAuthority {
    pub fn new(issuer: Issuer<'static, KeyPair>, provider: CryptoProvider) -> Self {
        let private_key =
            PrivateKeyDer::from(PrivatePkcs8KeyDer::from(issuer.key().serialize_der()));
        Self {
            issuer,
            private_key,
            cache: Mutex::new(HashMap::new()),
            provider: Arc::new(provider),
        }
    }

    fn gen_cert(&self, host: &str) -> CertificateDer<'static> {
        leaf_params(host)
            .signed_by(self.issuer.key(), &self.issuer)
            .expect("failed to sign leaf certificate")
            .into()
    }
}

impl CertificateAuthority for IpAwareAuthority {
    async fn gen_server_config(&self, authority: &Authority) -> Arc<ServerConfig> {
        let host = authority.host().to_string();
        if let Some(cfg) = self
            .cache
            .lock()
            .expect("CA cache poisoned")
            .get(&host)
            .cloned()
        {
            return cfg;
        }

        let certs = vec![self.gen_cert(&host)];
        let mut server_cfg = ServerConfig::builder_with_provider(Arc::clone(&self.provider))
            .with_safe_default_protocol_versions()
            .expect("failed to set protocol versions")
            .with_no_client_auth()
            .with_single_cert(certs, self.private_key.clone_key())
            .expect("failed to build ServerConfig");
        // hudsucker is built with the http2 feature, so advertise h2 too.
        server_cfg.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let server_cfg = Arc::new(server_cfg);

        self.cache
            .lock()
            .expect("CA cache poisoned")
            .insert(host, Arc::clone(&server_cfg));
        server_cfg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_host_gets_ip_san_not_dns() {
        // Regression: Riichi City's game server is reached by raw IP. A DnsName
        // SAN holding an IP would be rejected by the client, dropping the
        // gameplay WebSocket.
        let p = leaf_params("13.112.183.79");
        assert!(
            matches!(p.subject_alt_names.as_slice(), [SanType::IpAddress(_)]),
            "IP host must get an iPAddress SAN, got {:?}",
            p.subject_alt_names
        );
    }

    #[test]
    fn ipv6_host_gets_ip_san() {
        let p = leaf_params("2001:db8::1");
        assert!(matches!(
            p.subject_alt_names.as_slice(),
            [SanType::IpAddress(_)]
        ));
    }

    #[test]
    fn hostname_gets_dns_san() {
        let p = leaf_params("game.maj-soul.com");
        assert!(matches!(
            p.subject_alt_names.as_slice(),
            [SanType::DnsName(_)]
        ));
    }
}

#[cfg(all(test, unix))]
mod permission_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn new_and_existing_private_keys_are_owner_only_and_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ca");
        load_or_generate(&path).unwrap();
        for ext in [KEY_PEM_EXT, KEY_DER_EXT] {
            let p = path.join(format!("{BASENAME}.{ext}"));
            let before = std::fs::read(&p).unwrap();
            assert_eq!(
                std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
            load_or_generate(&path).unwrap();
            assert_eq!(before, std::fs::read(&p).unwrap());
            assert_eq!(
                std::fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
