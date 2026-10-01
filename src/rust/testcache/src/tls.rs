//! A TLS handshake with a remote cache, as buck2's client makes it

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::server::dial;

/// Connects to `host_port` and completes a TLS handshake within `timeout`,
/// offering HTTP/2 as gRPC does, and verifying the server's certificate
/// for its host name against the system's roots
pub(crate) fn handshake(host_port: &str, timeout: Duration) -> Result<(), String> {
    handshake_with(host_port, timeout, system_roots())
}

/// [`handshake`] with the roots as a seam
pub(crate) fn handshake_with(
    host_port: &str,
    timeout: Duration,
    roots: RootCertStore,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let host = split_host(host_port);
    let server_name = ServerName::try_from(host.to_string()).map_err(|e| format!("{host}: {e}"))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let mut conn =
        ClientConnection::new(Arc::new(config), server_name).map_err(|e| e.to_string())?;

    let mut stream = dial(host_port, timeout).map_err(|e| e.to_string())?;
    while conn.is_handshaking() {
        // The timeout bounds the connection and the handshake as a whole
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("i/o timeout".into());
        }
        stream
            .set_read_timeout(Some(left))
            .and_then(|()| stream.set_write_timeout(Some(left)))
            .map_err(|e| e.to_string())?;
        match conn.complete_io(&mut stream) {
            Ok((0, 0)) if conn.is_handshaking() => return Err("EOF".into()),
            Ok(_) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(())
}

/// The system's trusted roots
fn system_roots() -> RootCertStore {
    let mut roots = RootCertStore::empty();
    roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
    roots
}

/// The host of `host:port`, as `net.SplitHostPort` splits it (an IPv6
/// address in brackets), or the whole when there is no port
fn split_host(host_port: &str) -> &str {
    if let Some(rest) = host_port.strip_prefix('[')
        && let Some((host, _)) = rest.split_once(']')
    {
        return host;
    }
    host_port
        .rsplit_once(':')
        .map_or(host_port, |(host, _)| host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls::{ServerConfig, ServerConnection};
    use std::net::TcpListener;

    const CERT: &str = include_str!("../testdata/untrusted-cert.pem");
    const KEY: &str = include_str!("../testdata/untrusted-key.pem");

    /// A TLS server on a loopback port, serving the self-signed
    /// certificate of testdata/
    fn tls_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let cert = CertificateDer::from_pem_slice(CERT.as_bytes()).unwrap();
        let key = PrivateKeyDer::from_pem_slice(KEY.as_bytes()).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .unwrap();
        config.alpn_protocols = vec![b"h2".to_vec()];
        let config = Arc::new(config);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let config = config.clone();
                std::thread::spawn(move || {
                    let mut conn = ServerConnection::new(config).unwrap();
                    while conn.is_handshaking() {
                        if conn.complete_io(&mut stream).is_err() {
                            return;
                        }
                    }
                    let _ = conn.complete_io(&mut stream);
                });
            }
        });
        address
    }

    #[test]
    fn an_untrusted_certificate_fails_the_handshake() {
        let address = tls_server();
        let err = handshake(&address, Duration::from_secs(2)).unwrap_err();
        assert!(err.contains("certificate"), "{err}");
    }

    #[test]
    fn a_trusted_certificate_completes_it() {
        let address = tls_server();
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from_pem_slice(CERT.as_bytes()).unwrap())
            .unwrap();
        handshake_with(&address, Duration::from_secs(2), roots).unwrap();
    }

    #[test]
    fn hosts() {
        for (host_port, host) in [
            ("cache.example.com:443", "cache.example.com"),
            ("127.0.0.1:1", "127.0.0.1"),
            ("[::1]:443", "::1"),
            ("nohost", "nohost"),
        ] {
            assert_eq!(split_host(host_port), host);
        }
    }
}
