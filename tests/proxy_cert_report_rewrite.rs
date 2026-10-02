//! Real MITM forwarding must reject an origin signed by an untrusted CA.
//! Certificate report transformation remains covered in proxy::rewrite unit tests.

mod common;

use std::sync::Arc;
use std::time::Duration;

use akagi::config::{HttpCaptureConfig, Platform, ProxyConfig};
use akagi::logger::Session;
use akagi::proxy::start_proxy;
use hudsucker::rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair, SanType,
    SerialNumber,
};
use hudsucker::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use hudsucker::rustls::ServerConfig;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Notify};
use tokio_rustls::TlsAcceptor;

const TIMEOUT: Duration = Duration::from_secs(15);

/// The origin's own CA and leaf — the values that must end up in the
/// beacon. Deliberately unlike anything Akagi would mint: a distinct
/// issuer DN, a wildcard subject, and a serial of its own.
const ORIGIN_CA_CN: &str = "Example Root TLS CA";
const ORIGIN_SUBJECT: &str = "*.example.test";

const ORIGIN_SERIAL: u64 = 0x07FEC9E77B8C0D52;

/// A TLS origin serving a certificate chain of its own, so the proxy's
/// verifier has something genuine to record. Answers any request with a
/// zero-length 200, exactly as the real beacon endpoint does.
async fn tls_origin(host: &str) -> u16 {
    let ca_key = KeyPair::generate().unwrap();
    let mut ca_params = CertificateParams::default();
    let mut ca_dn = DistinguishedName::new();
    ca_dn.push(DnType::CountryName, "US");
    ca_dn.push(DnType::OrganizationName, "Example Inc");
    ca_dn.push(DnType::CommonName, ORIGIN_CA_CN);
    ca_params.distinguished_name = ca_dn;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    let issuer = Issuer::new(ca_params, ca_key);

    let leaf_key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, ORIGIN_SUBJECT);
    params.distinguished_name = dn;
    params.serial_number = Some(SerialNumber::from(ORIGIN_SERIAL));
    params
        .subject_alt_names
        .push(SanType::DnsName(host.try_into().unwrap()));
    let leaf = params.signed_by(&leaf_key, &issuer).unwrap();

    let certs = vec![CertificateDer::from(leaf.der().to_vec())];
    let key = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .expect("origin server config");
    let acceptor = TlsAcceptor::from(Arc::new(cfg));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind origin");
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else {
                return;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(sock).await else {
                    return;
                };
                let mut buf = [0u8; 4096];
                while let Ok(n) = tls.read(&mut buf).await {
                    if n == 0 {
                        return;
                    }
                    if buf[..n].windows(4).any(|w| w == b"\r\n\r\n")
                        && tls
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                            .await
                            .is_err()
                    {
                        return;
                    }
                }
            });
        }
    });
    port
}

async fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn wait_until_listening(port: u16) {
    let deadline = tokio::time::Instant::now() + TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("proxy never bound 127.0.0.1:{port}");
}

#[tokio::test(flavor = "multi_thread")]
async fn untrusted_origin_is_rejected_without_downgrading_tls() {
    let tmp = TempDir::new().expect("tempdir");
    let session =
        Arc::new(Session::init(&tmp.path().join("logs"), "info", "info", &[]).expect("session"));
    let inspector = session.dir().join("inspector.jsonl");

    // Must be a name that actually resolves: the proxy dials the origin
    // by name, and if resolution fails there is no handshake, no observed
    // certificate, and nothing to substitute. `localhost` also keeps SNI
    // a `DnsName`, which is the production path.
    let host = "localhost";
    let origin_port = tls_origin(host).await;
    let proxy_port = free_port().await;

    let (stop, stop_rx) = oneshot::channel::<()>();
    let proxy = tokio::spawn(start_proxy(
        ProxyConfig {
            enabled: true,
            addr: format!("127.0.0.1:{proxy_port}"),
            ca_dir: tmp.path().join("ca"),
            rewrite_certificate_report: true,
            // This test exercises the rewrite path, which only runs when the
            // beacon is actually forwarded — so blocking must be off here.
            block_telemetry: false,
        },
        HttpCaptureConfig::default(),
        Platform::Majsoul,
        session,
        None,
        None,
        Arc::new(Notify::new()),
        None,
        async move {
            stop_rx.await.unwrap_or_default();
        },
    ));
    wait_until_listening(proxy_port).await;

    // Two requests over one proxy connection. The first is an ordinary
    // HTTPS call whose only job is to make the proxy dial the origin, so
    // its certificate lands in the store — the same thing the game's
    // gateway probes do before it reports on them.
    let mut stream = TcpStream::connect(("127.0.0.1", proxy_port))
        .await
        .expect("reach proxy");
    let warmup = format!(
        "GET https://{host}:{origin_port}/api/clientgate/routes HTTP/1.1\r\nHost: {host}\r\n\r\n"
    );
    stream.write_all(warmup.as_bytes()).await.unwrap();
    let mut buf = [0u8; 1024];
    let n = tokio::time::timeout(TIMEOUT, stream.read(&mut buf))
        .await
        .expect("proxy answered the warm-up")
        .expect("read");
    let status = String::from_utf8_lossy(&buf[..n]);
    assert!(
        status.starts_with("HTTP/1.1 502"),
        "untrusted origin must fail: {status}"
    );
    let _ = stop.send(());
    let _ = tokio::time::timeout(TIMEOUT, proxy).await;
    let body = std::fs::read_to_string(inspector).unwrap_or_default();
    assert!(!body.contains("DEADBEEF"));
}
