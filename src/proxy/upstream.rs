//! Strict upstream TLS validation shared by HTTP and WebSocket connectors.
use super::certstore::CertStore;
use hudsucker::{
    hyper_util::client::legacy::connect::HttpConnector,
    rustls::{
        client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        crypto::aws_lc_rs,
        pki_types::{CertificateDer, ServerName, UnixTime},
        ClientConfig, DigitallySignedStruct, SignatureScheme,
    },
    tokio_tungstenite::Connector as WsConnector,
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// `ServerCertVerifier` that records only successfully validated certificates.
///
/// It is also the only place Akagi ever sees the origin's **real**
/// certificate — the client only ever sees ours — so the leaf is recorded
/// on the way past. Recording is best-effort and can never fail the
/// handshake; see [`CertStore::record`].
#[derive(Debug)]
struct RecordingVerifier {
    store: Arc<CertStore>,
    inner: Arc<dyn ServerCertVerifier>,
    pending: Mutex<HashMap<Vec<u8>, Vec<String>>>,
}

impl RecordingVerifier {
    fn finish_signature(
        &self,
        cert: &CertificateDer<'_>,
        result: Result<HandshakeSignatureValid, rustls::Error>,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        let hosts = self
            .pending
            .lock()
            .expect("verifier pending lock")
            .remove(cert.as_ref());
        let verified = result?;
        for host in hosts.into_iter().flatten() {
            self.store.record(&host, cert);
        }
        Ok(verified)
    }
}

impl ServerCertVerifier for RecordingVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, hudsucker::rustls::Error> {
        let verified = self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )?;
        // Delay publication until the peer also proves possession of the key.
        let host = match server_name {
            ServerName::DnsName(name) => Some(name.as_ref().to_owned()),
            ServerName::IpAddress(ip) => Some(std::net::IpAddr::from(*ip).to_string()),
            _ => None,
        };
        if let Some(host) = host {
            let mut pending = self.pending.lock().expect("verifier pending lock");
            if pending.len() >= 256 {
                pending.clear();
            }
            let hosts = pending.entry(end_entity.to_vec()).or_default();
            if !hosts.contains(&host) && hosts.len() < 256 {
                hosts.push(host);
            }
        }
        Ok(verified)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, hudsucker::rustls::Error> {
        self.finish_signature(cert, self.inner.verify_tls12_signature(message, cert, dss))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, hudsucker::rustls::Error> {
        self.finish_signature(cert, self.inner.verify_tls13_signature(message, cert, dss))
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// Shared rustls `ClientConfig`. aws-lc-rs as the crypto provider to match
/// hudsucker's `RcgenAuthority` (so we don't load two different providers
/// in the process). TLS 1.2 + TLS 1.3 enabled, no client cert auth.
fn client_config(store: Arc<CertStore>) -> Arc<ClientConfig> {
    let provider = aws_lc_rs::default_provider();
    let provider = Arc::new(provider);
    let roots =
        hudsucker::rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let inner = hudsucker::rustls::client::WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        provider.clone(),
    )
    .build()
    .expect("Mozilla root bundle is nonempty");
    let cfg = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("aws-lc-rs supports both TLS 1.2 and 1.3 — protocol version selection cannot fail")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(RecordingVerifier {
            store,
            inner,
            pending: Mutex::new(HashMap::new()),
        }))
        .with_no_client_auth();
    Arc::new(cfg)
}

/// HTTPS connector for the proxy's upstream HTTP/WS leg. HTTP/1 + HTTP/2
/// both enabled; ALPN is negotiated per-connection by hyper-rustls.
pub fn http_connector(store: Arc<CertStore>) -> hyper_rustls::HttpsConnector<HttpConnector> {
    let config = client_config(store);
    hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config((*config).clone())
        .https_or_http()
        .enable_http1()
        .enable_http2()
        .build()
}

/// WebSocket connector for upstream WS upgrades originating from the
/// proxy. Same `ClientConfig` as `http_connector` so the same strict verification
/// policy applies.
pub fn websocket_connector(store: Arc<CertStore>) -> WsConnector {
    WsConnector::Rustls(client_config(store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hudsucker::rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair};
    use rustls::{
        pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
        RootCertStore, ServerConfig,
    };
    use tokio::net::{TcpListener, TcpStream};
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    async fn handshake(
        trust_root: bool,
        hostname: &str,
        expired: bool,
        bad_signature: bool,
    ) -> (bool, Arc<CertStore>) {
        let key = KeyPair::generate().unwrap();
        let mut ca = CertificateParams::default();
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let root = ca.self_signed(&key).unwrap();
        let issuer = Issuer::new(ca, key);
        let leaf_key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(vec!["localhost".into()]).unwrap();
        if expired {
            params.not_before = time::OffsetDateTime::UNIX_EPOCH;
            params.not_after = time::OffsetDateTime::UNIX_EPOCH + time::Duration::days(1);
        }
        let cert = params.signed_by(&leaf_key, &issuer).unwrap();
        #[derive(Debug)]
        struct Resolver(Arc<rustls::sign::CertifiedKey>);
        impl rustls::server::ResolvesServerCert for Resolver {
            fn resolve(
                &self,
                _: rustls::server::ClientHello<'_>,
            ) -> Option<Arc<rustls::sign::CertifiedKey>> {
                Some(self.0.clone())
            }
        }
        let signer = if bad_signature {
            KeyPair::generate().unwrap()
        } else {
            leaf_key
        };
        let key = aws_lc_rs::sign::any_supported_type(&PrivateKeyDer::Pkcs8(
            PrivatePkcs8KeyDer::from(signer.serialize_der()),
        ))
        .unwrap();
        let server = ServerConfig::builder_with_provider(Arc::new(aws_lc_rs::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(Resolver(Arc::new(
                rustls::sign::CertifiedKey::new(vec![cert.der().clone()], key),
            ))));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = TlsAcceptor::from(Arc::new(server)).accept(stream).await;
        });
        let mut roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        if trust_root {
            roots.add(root.der().clone()).unwrap();
        }
        let provider = Arc::new(aws_lc_rs::default_provider());
        let inner = rustls::client::WebPkiServerVerifier::builder_with_provider(
            Arc::new(roots),
            provider.clone(),
        )
        .build()
        .unwrap();
        let store = Arc::new(CertStore::default());
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(RecordingVerifier {
                store: store.clone(),
                inner,
                pending: Mutex::new(HashMap::new()),
            }))
            .with_no_client_auth();
        let result = TlsConnector::from(Arc::new(config))
            .connect(
                ServerName::try_from(hostname.to_owned()).unwrap(),
                TcpStream::connect(addr).await.unwrap(),
            )
            .await;
        let ok = result.is_ok();
        drop(result);
        task.await.unwrap();
        (ok, store)
    }
    #[tokio::test]
    async fn valid_chain_and_signature_succeed() {
        let (ok, store) = handshake(true, "localhost", false, false).await;
        assert!(ok);
        assert!(store.get("localhost").is_some());
    }
    #[tokio::test]
    async fn untrusted_wrong_name_and_expired_fail_without_caching() {
        for (trusted, name, expired, bad_signature) in [
            (false, "localhost", false, false),
            (true, "other.example", false, false),
            (true, "localhost", true, false),
            (true, "localhost", false, true),
        ] {
            let (ok, store) = handshake(trusted, name, expired, bad_signature).await;
            assert!(!ok);
            assert!(store.get(name).is_none());
        }
    }
}
