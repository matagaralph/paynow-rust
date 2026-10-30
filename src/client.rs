#[cfg(feature = "ureq")]
use crate::Ureq;
use crate::{
    Error, Fields, MobileResponse, Payment, PaymentMethod, Result, StatusResponse, Trace,
    Transport, WebResponse, form, hash,
    payment::{format_cents, is_valid_email},
};

const WEB_URL: &str = "https://www.paynow.co.zw/interface/initiatetransaction";
const MOBILE_URL: &str = "https://www.paynow.co.zw/interface/remotetransaction";
const TRACE_URL: &str = "https://www.paynow.co.zw/interface/trace";

/// Talks to Paynow on behalf of a single merchant integration.
///
/// With a [`Transport`], methods such as [`Client::send`] do the whole round trip. Without one,
/// build each request with a `*_request` method, send it with any HTTP client, sync or async,
/// and check the reply with the matching `parse_*` method.
#[derive(Clone)]
pub struct Client<T = ()> {
    id: String,
    key: String,
    result_url: String,
    return_url: String,
    transport: T,
}

#[cfg(feature = "ureq")]
impl Client<Ureq> {
    pub fn new(integration_id: impl Into<String>, integration_key: &str) -> Self {
        Self::with_transport(integration_id, integration_key, Ureq::default())
    }
}

impl Client {
    /// A client that never does networking itself, for use with your own HTTP client.
    pub fn without_transport(integration_id: impl Into<String>, integration_key: &str) -> Self {
        Self::with_transport(integration_id, integration_key, ())
    }
}

/// A request for Paynow, to be sent with your own HTTP client.
///
/// POST `body` to `url` with the content type [`Request::CONTENT_TYPE`]. Paynow answers its own
/// errors with HTTP 200, so treat any other status, like a timeout or a dropped connection, as a
/// failed request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub body: String,
}

impl Request {
    pub const CONTENT_TYPE: &'static str = "application/x-www-form-urlencoded";
}

impl<T> Client<T> {
    pub fn with_transport(
        integration_id: impl Into<String>,
        integration_key: &str,
        transport: T,
    ) -> Self {
        Self {
            id: integration_id.into(),
            // Paynow lowercases the key before hashing.
            key: integration_key.to_lowercase(),
            result_url: String::new(),
            return_url: String::new(),
            transport,
        }
    }

    /// Where Paynow posts transaction status updates.
    #[must_use]
    pub fn with_result_url(mut self, url: impl Into<String>) -> Self {
        self.result_url = url.into();
        self
    }

    /// Where the customer is sent after paying.
    #[must_use]
    pub fn with_return_url(mut self, url: impl Into<String>) -> Self {
        self.return_url = url.into();
        self
    }

    pub fn payment(&self, reference: impl Into<String>, auth_email: impl Into<String>) -> Payment {
        Payment::new(reference, auth_email)
    }

    /// Verifies a status update Paynow posted to the result URL.
    ///
    /// Pass the request body exactly as received, since the hash covers the field order. Paynow
    /// recommends confirming important updates with [`Client::poll_transaction`].
    pub fn process_status_update(&self, body: &str) -> Result<StatusResponse> {
        self.verified(body).and_then(StatusResponse::new)
    }

    /// Builds the request that starts a web transaction. The reply goes to
    /// [`Client::parse_web_response`].
    pub fn web_request(&self, payment: &Payment) -> Result<Request> {
        payment.validate()?;
        Ok(Request {
            url: WEB_URL.to_owned(),
            body: self.payment_body(payment, &[]),
        })
    }

    /// Builds the request that starts an express checkout mobile money transaction, charging
    /// `phone` directly. The reply goes to [`Client::parse_mobile_response`].
    ///
    /// Set [`Payment::merchant_trace`] first, so that if the request fails or times out the
    /// transaction can be recovered with [`Client::trace_request`] rather than retried blindly,
    /// which could charge the customer twice.
    pub fn mobile_request(
        &self,
        payment: &Payment,
        phone: &str,
        method: PaymentMethod,
    ) -> Result<Request> {
        payment.validate()?;
        if !is_valid_email(&payment.auth_email) {
            return Err(Error::InvalidEmail);
        }

        let mut extra = vec![("phone", phone), ("method", method.as_str())];
        if let Some(trace) = &payment.merchant_trace {
            extra.push(("merchanttrace", trace));
        }
        Ok(Request {
            url: MOBILE_URL.to_owned(),
            body: self.payment_body(payment, &extra),
        })
    }

    /// Builds the request that checks a transaction's status. The reply goes to
    /// [`Client::parse_status_response`].
    pub fn poll_request(&self, poll_url: &str) -> Request {
        Request {
            url: poll_url.to_owned(),
            body: String::new(),
        }
    }

    /// Builds the request that looks a transaction up by the merchant trace it was initiated
    /// with. The reply goes to [`Client::parse_trace_response`].
    pub fn trace_request(&self, merchant_trace: &str) -> Request {
        Request {
            url: TRACE_URL.to_owned(),
            body: self.signed(&[
                ("id", &self.id),
                ("merchanttrace", merchant_trace),
                ("status", "Message"),
            ]),
        }
    }

    /// Checks the reply to a [`Client::web_request`].
    pub fn parse_web_response(&self, body: &str) -> Result<WebResponse> {
        self.verified(body).and_then(WebResponse::new)
    }

    /// Checks the reply to a [`Client::mobile_request`].
    pub fn parse_mobile_response(&self, body: &str) -> Result<MobileResponse> {
        self.verified(body).and_then(MobileResponse::new)
    }

    /// Checks the reply to a [`Client::poll_request`].
    pub fn parse_status_response(&self, body: &str) -> Result<StatusResponse> {
        self.verified(body).and_then(StatusResponse::new)
    }

    /// Checks the reply to a [`Client::trace_request`].
    ///
    /// [`Error::Api`] here does not prove the transaction is missing; only [`Trace::NotFound`]
    /// does.
    pub fn parse_trace_response(&self, body: &str) -> Result<Trace> {
        let fields = self.verified(body)?;
        if fields.status_is("notfound") {
            Ok(Trace::NotFound)
        } else {
            StatusResponse::new(fields).map(Trace::Found)
        }
    }

    fn verified(&self, body: &str) -> Result<Fields> {
        let fields = Fields::parse(body)?;
        if fields.status_is("error") {
            return Err(Error::Api(fields.text("error")));
        }
        fields.verify(&self.key)?;
        Ok(fields)
    }

    fn payment_body(&self, payment: &Payment, extra: &[(&str, &str)]) -> String {
        let amount = format_cents(payment.total());
        let info = payment.info();

        let mut fields = Vec::with_capacity(11);
        fields.extend_from_slice(&[
            ("resulturl", self.result_url.as_str()),
            ("returnurl", &self.return_url),
            ("reference", &payment.reference),
            ("amount", &amount),
            ("id", &self.id),
            ("additionalinfo", &info),
            ("authemail", &payment.auth_email),
        ]);
        fields.extend_from_slice(extra);
        fields.push(("status", "Message"));
        self.signed(&fields)
    }

    fn signed(&self, fields: &[(&str, &str)]) -> String {
        let hash = hash::sign(fields.iter().map(|&(_, value)| value), &self.key);
        let mut body = form::encode(fields);
        body.push_str("&hash=");
        body.push_str(&hash);
        body
    }
}

impl<T: Transport> Client<T> {
    /// Starts a web transaction; send the customer to [`WebResponse::redirect_url`] to pay.
    pub fn send(&self, payment: &Payment) -> Result<WebResponse> {
        let reply = self.post(&self.web_request(payment)?)?;
        self.parse_web_response(&reply)
    }

    /// Starts an express checkout mobile money transaction, charging `phone` directly.
    ///
    /// Set [`Payment::merchant_trace`] first, so that if this fails with
    /// [`Error::Transport`] the transaction can be recovered with [`Client::trace`] rather
    /// than retried blindly, which could charge the customer twice.
    pub fn send_mobile(
        &self,
        payment: &Payment,
        phone: &str,
        method: PaymentMethod,
    ) -> Result<MobileResponse> {
        let reply = self.post(&self.mobile_request(payment, phone, method)?)?;
        self.parse_mobile_response(&reply)
    }

    pub fn poll_transaction(&self, poll_url: &str) -> Result<StatusResponse> {
        let reply = self.post(&self.poll_request(poll_url))?;
        self.parse_status_response(&reply)
    }

    /// Looks a transaction up by the merchant trace it was initiated with.
    ///
    /// [`Error::Api`] here does not prove the transaction is missing; only [`Trace::NotFound`]
    /// does.
    pub fn trace(&self, merchant_trace: &str) -> Result<Trace> {
        let reply = self.post(&self.trace_request(merchant_trace))?;
        self.parse_trace_response(&reply)
    }

    fn post(&self, request: &Request) -> Result<String> {
        Ok(self.transport.post(&request.url, &request.body)?)
    }
}
