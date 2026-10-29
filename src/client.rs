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
#[derive(Clone)]
pub struct Client<T> {
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

    fn verified(&self, body: &str) -> Result<Fields> {
        let fields = Fields::parse(body)?;
        if fields.status_is("error") {
            return Err(Error::Api(fields.text("error")));
        }
        fields.verify(&self.key)?;
        Ok(fields)
    }

    fn request(&self, payment: &Payment, extra: &[(&str, &str)]) -> String {
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
        payment.validate()?;
        self.post(WEB_URL, &self.request(payment, &[]))
            .and_then(WebResponse::new)
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
        payment.validate()?;
        if !is_valid_email(&payment.auth_email) {
            return Err(Error::InvalidEmail);
        }

        let mut extra = vec![("phone", phone), ("method", method.as_str())];
        if let Some(trace) = &payment.merchant_trace {
            extra.push(("merchanttrace", trace));
        }
        self.post(MOBILE_URL, &self.request(payment, &extra))
            .and_then(MobileResponse::new)
    }

    pub fn poll_transaction(&self, poll_url: &str) -> Result<StatusResponse> {
        self.post(poll_url, "").and_then(StatusResponse::new)
    }

    /// Looks a transaction up by the merchant trace it was initiated with.
    ///
    /// [`Error::Api`] here does not prove the transaction is missing; only [`Trace::NotFound`]
    /// does.
    pub fn trace(&self, merchant_trace: &str) -> Result<Trace> {
        let body = self.signed(&[
            ("id", &self.id),
            ("merchanttrace", merchant_trace),
            ("status", "Message"),
        ]);
        let fields = self.post(TRACE_URL, &body)?;
        if fields.status_is("notfound") {
            Ok(Trace::NotFound)
        } else {
            StatusResponse::new(fields).map(Trace::Found)
        }
    }

    fn post(&self, url: &str, body: &str) -> Result<Fields> {
        let response = self.transport.post(url, body)?;
        self.verified(&response)
    }
}
