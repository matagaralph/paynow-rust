use crate::{Error, Fields, Result, TransactionStatus, form, payment::parse_cents};

const INNBUCKS_DEEP_LINK: &str = "com.innbucks.customer://purchase?paymentToken=";

/// A web transaction Paynow accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebResponse {
    /// Where to send the customer to pay.
    pub redirect_url: String,
    pub poll_url: String,
    pub hash: String,
    pub fields: Fields,
}

impl WebResponse {
    pub(crate) fn new(fields: Fields) -> Result<Self> {
        expect_ok(&fields)?;
        Ok(Self {
            redirect_url: fields.require("browserurl")?.to_owned(),
            poll_url: fields.require("pollurl")?.to_owned(),
            hash: fields.text("hash"),
            fields,
        })
    }
}

/// A mobile transaction Paynow accepted; the customer now approves it on their phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileResponse {
    pub poll_url: String,
    /// USSD instructions for the customer, sent for some methods.
    pub instructions: Option<String>,
    pub innbucks: Option<InnBucks>,
    pub hash: String,
    pub fields: Fields,
}

impl MobileResponse {
    pub(crate) fn new(fields: Fields) -> Result<Self> {
        expect_ok(&fields)?;
        let innbucks = fields
            .get("authorizationcode")
            .filter(|code| !code.is_empty())
            .map(|code| {
                let mut deep_link_url = INNBUCKS_DEEP_LINK.to_owned();
                form::escape_into(&mut deep_link_url, code);
                InnBucks {
                    authorisation_code: code.to_owned(),
                    deep_link_url,
                    expires_at: fields.get("authorizationexpires").map(str::to_owned),
                }
            });

        Ok(Self {
            poll_url: fields.require("pollurl")?.to_owned(),
            instructions: fields.get("instructions").map(str::to_owned),
            innbucks,
            hash: fields.text("hash"),
            fields,
        })
    }
}

/// What the customer needs to approve an InnBucks payment. Show the code as text or as a QR
/// code, or offer the deep link on a phone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InnBucks {
    pub authorisation_code: String,
    /// Opens the InnBucks app with the code filled in. InnBucks is unavailable in Paynow's test
    /// mode, so check this on a device against a live integration.
    pub deep_link_url: String,
    pub expires_at: Option<String>,
}

/// The state of a transaction, from polling, tracing or an update posted to the result URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusResponse {
    pub status: TransactionStatus,
    /// In cents.
    pub amount: u64,
    pub reference: String,
    pub paynow_reference: String,
    pub poll_url: String,
    pub hash: String,
    pub fields: Fields,
}

impl StatusResponse {
    pub(crate) fn new(fields: Fields) -> Result<Self> {
        Ok(Self {
            status: fields.require("status")?.into(),
            amount: parse_cents(fields.require("amount")?).ok_or(Error::InvalidField("amount"))?,
            reference: fields.require("reference")?.to_owned(),
            paynow_reference: fields.require("paynowreference")?.to_owned(),
            poll_url: fields.require("pollurl")?.to_owned(),
            hash: fields.text("hash"),
            fields,
        })
    }

    pub fn is_paid(&self) -> bool {
        self.status.is_paid()
    }
}

/// The outcome of looking a transaction up by its merchant trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trace {
    Found(StatusResponse),
    /// Paynow has no transaction with this trace, so it is safe to initiate it again.
    NotFound,
}

fn expect_ok(fields: &Fields) -> Result<()> {
    if fields.status_is("ok") {
        Ok(())
    } else {
        Err(Error::UnexpectedStatus(fields.text("status")))
    }
}
