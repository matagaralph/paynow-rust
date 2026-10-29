use std::io;

/// Posts a form-encoded body and returns the response body.
///
/// Implemented for closures, which makes it easy to plug in another HTTP client or a mock.
pub trait Transport {
    fn post(&self, url: &str, body: &str) -> io::Result<String>;
}

impl<F> Transport for F
where
    F: Fn(&str, &str) -> io::Result<String>,
{
    fn post(&self, url: &str, body: &str) -> io::Result<String> {
        self(url, body)
    }
}

#[cfg(feature = "ureq")]
pub use self::ureq::Ureq;

#[cfg(feature = "ureq")]
mod ureq {
    use std::{io, time::Duration};

    use ureq::{
        Agent,
        tls::{RootCerts, TlsConfig, TlsProvider},
    };

    use super::Transport;
    use crate::Fields;

    const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    /// The default transport, backed by a [`ureq::Agent`] that reuses connections.
    ///
    /// Requests time out after 30 seconds unless built with [`Ureq::with_timeout`].
    #[derive(Debug, Clone)]
    pub struct Ureq(Agent);

    impl Ureq {
        pub fn with_timeout(timeout: Duration) -> Self {
            Self::configured(timeout, true)
        }

        fn configured(timeout: Duration, https_only: bool) -> Self {
            Agent::config_builder()
                .https_only(https_only)
                // Paynow only offers TLS 1.2 with CBC cipher suites, which rustls refuses.
                .tls_config(
                    TlsConfig::builder()
                        .provider(TlsProvider::NativeTls)
                        .root_certs(RootCerts::PlatformVerifier)
                        .build(),
                )
                .http_status_as_error(false)
                .timeout_global(Some(timeout))
                .build()
                .new_agent()
                .into()
        }
    }

    impl Default for Ureq {
        fn default() -> Self {
            Self::with_timeout(DEFAULT_TIMEOUT)
        }
    }

    impl From<Agent> for Ureq {
        fn from(agent: Agent) -> Self {
            Self(agent)
        }
    }

    impl Transport for Ureq {
        fn post(&self, url: &str, body: &str) -> io::Result<String> {
            let mut response = self
                .0
                .post(url)
                .content_type("application/x-www-form-urlencoded")
                .send(body)
                .map_err(ureq::Error::into_io)?;
            let status = response.status();
            let body = response
                .body_mut()
                .read_to_string()
                .map_err(ureq::Error::into_io)?;

            // Paynow answers errors with 200, so any other status that lacks a Paynow reply
            // came from something in between, such as a proxy or load balancer.
            let is_paynow_reply = || Fields::parse(&body).is_ok_and(|f| f.get("status").is_some());
            if status.is_success() || is_paynow_reply() {
                Ok(body)
            } else {
                Err(io::Error::other(format!("Paynow returned HTTP {status}")))
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            thread,
        };

        use super::*;

        /// Serves one request on a local port, replying with `reply` after `delay`.
        fn serve(reply: String, delay: Duration) -> String {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/", listener.local_addr().unwrap());
            thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buf = [0; 1024];
                while !request.ends_with(b"\r\n\r\nid=1") {
                    let n = stream.read(&mut buf).unwrap();
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&buf[..n]);
                }
                thread::sleep(delay);
                let _ = stream.write_all(reply.as_bytes());
            });
            url
        }

        fn post(reply: String, delay: Duration, timeout: Duration) -> io::Result<String> {
            let url = serve(reply, delay);
            Ureq::configured(timeout, false).post(&url, "id=1")
        }

        fn response(status: &str, body: &str) -> String {
            format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        }

        #[test]
        fn returns_the_body_on_success() {
            let body = post(
                response("200 OK", "status=Ok&pollurl=p"),
                Duration::ZERO,
                DEFAULT_TIMEOUT,
            )
            .unwrap();
            assert_eq!(body, "status=Ok&pollurl=p");
        }

        #[test]
        fn keeps_paynow_replies_sent_with_an_error_status() {
            let body = post(
                response("500 Internal Server Error", "status=Error&error=Oops"),
                Duration::ZERO,
                DEFAULT_TIMEOUT,
            )
            .unwrap();
            assert_eq!(body, "status=Error&error=Oops");
        }

        #[test]
        fn reports_http_failures_from_elsewhere() {
            let err = post(
                response("502 Bad Gateway", "<html>Bad gateway</html>"),
                Duration::ZERO,
                DEFAULT_TIMEOUT,
            )
            .unwrap_err();
            assert!(err.to_string().contains("502"), "{err}");
        }

        #[test]
        fn times_out() {
            let result = post(
                response("200 OK", "status=Ok"),
                Duration::from_secs(2),
                Duration::from_millis(200),
            );
            assert!(result.is_err());
        }

        #[test]
        fn refuses_plain_http_by_default() {
            assert!(Ureq::default().post("http://127.0.0.1:9/", "").is_err());
        }
    }
}
