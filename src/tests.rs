use std::{cell::RefCell, io};

use crate::{Client, Error, Fields, Payment, PaymentMethod, Trace, Transport, form, hash};

const KEY: &str = "3E9C8B12-integration-key";
const POLL_URL: &str = "https://www.paynow.co.zw/interface/poll/1";

fn signed(fields: &[(&str, &str)]) -> String {
    let hash = hash::sign(fields.iter().map(|&(_, v)| v), &KEY.to_lowercase());
    let mut fields = fields.to_vec();
    fields.push(("hash", &hash));
    form::encode(&fields)
}

fn client<T: Transport>(transport: T) -> Client<T> {
    Client::with_transport("12345", KEY, transport)
        .with_result_url("https://example.com/result")
        .with_return_url("https://example.com/return")
}

fn payment() -> Payment {
    let mut payment = Payment::new("INV-1", "buyer@example.com");
    payment.add("Item", 1000);
    payment
}

fn paid_update() -> String {
    signed(&[
        ("reference", "INV-1"),
        ("amount", "10.00"),
        ("paynowreference", "PN-987"),
        ("pollurl", POLL_URL),
        ("status", "Paid"),
    ])
}

/// Replies with `response` and records the last request.
struct Mock {
    response: String,
    request: RefCell<(String, String)>,
}

impl Mock {
    fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
            request: RefCell::default(),
        }
    }

    fn sent(&self) -> (String, Fields) {
        let (url, body) = self.request.borrow().clone();
        (url, Fields::parse(&body).unwrap())
    }
}

impl Transport for &Mock {
    fn post(&self, url: &str, body: &str) -> io::Result<String> {
        *self.request.borrow_mut() = (url.to_owned(), body.to_owned());
        Ok(self.response.clone())
    }
}

fn offline(_: &str, _: &str) -> io::Result<String> {
    Err(io::Error::other("network down"))
}

fn unwrap_err<T: std::fmt::Debug>(result: crate::Result<T>) -> Error {
    result.expect_err("expected an error")
}

#[test]
fn send_signs_the_request_and_returns_the_redirect() {
    let mock = Mock::new(signed(&[
        ("status", "Ok"),
        ("browserurl", "https://www.paynow.co.zw/payment/confirm/1"),
        ("pollurl", POLL_URL),
    ]));

    let response = client(&mock).send(&payment()).unwrap();
    assert_eq!(
        response.redirect_url,
        "https://www.paynow.co.zw/payment/confirm/1"
    );
    assert_eq!(response.poll_url, POLL_URL);

    let (url, sent) = mock.sent();
    assert!(url.ends_with("/initiatetransaction"));
    assert_eq!(sent.get("id"), Some("12345"));
    assert_eq!(sent.get("amount"), Some("10.00"));
    assert_eq!(sent.get("additionalinfo"), Some("Item"));
    assert!(sent.verify(&KEY.to_lowercase()).is_ok());
}

#[test]
fn send_accepts_the_documented_capitalised_fields() {
    let body = signed(&[
        ("Status", "Ok"),
        (
            "BrowserUrl",
            "https://www.paynow.co.zw/Payment/ConfirmPayment/9510",
        ),
        ("PollUrl", POLL_URL),
    ])
    .replace("&hash=", "&Hash=");

    let response = client(&Mock::new(body)).send(&payment()).unwrap();
    assert_eq!(
        response.redirect_url,
        "https://www.paynow.co.zw/Payment/ConfirmPayment/9510"
    );
    assert_eq!(response.poll_url, POLL_URL);
}

#[test]
fn send_reports_the_documented_error() {
    let mock = Mock::new("Status=Error&Error=Invalid+amount+field");
    match client(&mock).send(&payment()) {
        Err(Error::Api(message)) => assert_eq!(message, "Invalid amount field"),
        other => panic!("expected an API error, got {other:?}"),
    }
}

#[test]
fn send_requires_the_redirect_and_poll_urls() {
    let mock = Mock::new(signed(&[("status", "Ok"), ("pollurl", POLL_URL)]));
    assert!(matches!(
        unwrap_err(client(&mock).send(&payment())),
        Error::InvalidField("browserurl")
    ));

    let mock = Mock::new(signed(&[
        ("status", "Ok"),
        ("browserurl", "b"),
        ("pollurl", ""),
    ]));
    assert!(matches!(
        unwrap_err(client(&mock).send(&payment())),
        Error::InvalidField("pollurl")
    ));
}

#[test]
fn initiation_requires_an_ok_status() {
    let mock = Mock::new(signed(&[("browserurl", "b"), ("pollurl", POLL_URL)]));
    assert!(matches!(
        unwrap_err(client(&mock).send(&payment())),
        Error::UnexpectedStatus(status) if status.is_empty()
    ));

    let mock = Mock::new(signed(&[("status", "Paid"), ("pollurl", POLL_URL)]));
    assert!(matches!(
        unwrap_err(client(&mock).send_mobile(&payment(), "0771234567", PaymentMethod::Ecocash)),
        Error::UnexpectedStatus(status) if status == "Paid"
    ));
}

#[test]
fn send_mobile_posts_the_method_phone_and_trace() {
    let mock = Mock::new(signed(&[
        ("status", "Ok"),
        ("pollurl", POLL_URL),
        ("instructions", "Dial *151#"),
    ]));
    let mut payment = payment();
    payment.merchant_trace = Some("TRACE-1".into());

    let response = client(&mock)
        .send_mobile(&payment, "0771234567", PaymentMethod::Ecocash)
        .unwrap();
    assert_eq!(response.poll_url, POLL_URL);
    assert_eq!(response.instructions.as_deref(), Some("Dial *151#"));
    assert!(response.innbucks.is_none());

    let (url, sent) = mock.sent();
    assert!(url.ends_with("/remotetransaction"));
    assert_eq!(sent.get("method"), Some("ecocash"));
    assert_eq!(sent.get("phone"), Some("0771234567"));
    assert_eq!(sent.get("merchanttrace"), Some("TRACE-1"));
    assert!(sent.verify(&KEY.to_lowercase()).is_ok());
}

#[test]
fn send_mobile_omits_an_unset_trace() {
    let mock = Mock::new(signed(&[("status", "Ok"), ("pollurl", POLL_URL)]));
    client(&mock)
        .send_mobile(&payment(), "0771234567", PaymentMethod::Ecocash)
        .unwrap();
    assert_eq!(mock.sent().1.get("merchanttrace"), None);
}

#[test]
fn send_mobile_requires_a_poll_url() {
    let mock = Mock::new(signed(&[("status", "Ok"), ("instructions", "Dial *151#")]));
    assert!(matches!(
        unwrap_err(client(&mock).send_mobile(&payment(), "0771234567", PaymentMethod::Ecocash)),
        Error::InvalidField("pollurl")
    ));
}

#[test]
fn send_mobile_surfaces_innbucks_details() {
    let mock = Mock::new(signed(&[
        ("status", "Ok"),
        ("pollurl", POLL_URL),
        ("authorizationcode", "123456789"),
        ("authorizationexpires", "2026-01-01 00:00:00"),
    ]));

    let response = client(&mock)
        .send_mobile(&payment(), "0771234567", PaymentMethod::Innbucks)
        .unwrap();
    let innbucks = response.innbucks.unwrap();
    assert_eq!(innbucks.authorisation_code, "123456789");
    assert_eq!(
        innbucks.deep_link_url,
        "com.innbucks.customer://purchase?paymentToken=123456789"
    );
    assert_eq!(innbucks.expires_at.as_deref(), Some("2026-01-01 00:00:00"));
}

#[test]
fn send_validates_before_posting() {
    let client = client(offline);

    let empty = Payment::new("INV", "buyer@example.com");
    assert!(matches!(client.send(&empty), Err(Error::EmptyCart)));

    let mut free = Payment::new("INV", "buyer@example.com");
    free.add("Free", 0);
    assert!(matches!(client.send(&free), Err(Error::ZeroTotal)));

    let mut anonymous = Payment::new("INV", "not-an-email");
    anonymous.add("Item", 1000);
    assert!(matches!(
        client.send_mobile(&anonymous, "0771234567", PaymentMethod::Ecocash),
        Err(Error::InvalidEmail)
    ));

    let mut traced = payment();
    traced.merchant_trace = Some("x".repeat(33));
    assert!(matches!(
        client.send_mobile(&traced, "0771234567", PaymentMethod::Ecocash),
        Err(Error::InvalidMerchantTrace)
    ));
}

#[test]
fn transport_errors_are_reported() {
    let client = client(offline);
    assert!(matches!(client.send(&payment()), Err(Error::Transport(_))));
    assert!(matches!(
        client.poll_transaction("poll"),
        Err(Error::Transport(_))
    ));
    assert!(matches!(client.trace("TRACE-1"), Err(Error::Transport(_))));
}

#[test]
fn tampered_responses_are_rejected() {
    let mock = Mock::new("status=Ok&pollurl=p&hash=WRONGHASH");
    assert!(matches!(
        client(&mock).send(&payment()),
        Err(Error::HashMismatch)
    ));

    let mock = Mock::new("status=Ok&pollurl=p");
    assert!(matches!(
        client(&mock).send(&payment()),
        Err(Error::MissingHash)
    ));

    let client = client(offline);
    assert!(matches!(
        client.process_status_update("status=Paid&reference=INV-1&hash=WRONG"),
        Err(Error::HashMismatch)
    ));
    let forged = paid_update().replace("amount=10.00", "amount=1.00");
    assert!(matches!(
        client.process_status_update(&forged),
        Err(Error::HashMismatch)
    ));
}

#[test]
fn poll_transaction_reports_payment() {
    let mock = Mock::new(paid_update());
    let update = client(&mock).poll_transaction(POLL_URL).unwrap();

    assert!(update.is_paid());
    assert_eq!(update.amount, 1000);
    assert_eq!(update.reference, "INV-1");
    assert_eq!(update.paynow_reference, "PN-987");
    let (url, sent) = mock.sent();
    assert_eq!(url, POLL_URL);
    assert_eq!(sent, Fields::default());
}

#[test]
fn status_updates_follow_the_documented_example() {
    let body = signed(&[
        ("reference", "ABC123"),
        ("paynowreference", "123456"),
        ("amount", "1.00"),
        ("status", "Awaiting Delivery"),
        ("pollurl", POLL_URL),
    ]);
    let update = client(offline).process_status_update(&body).unwrap();
    assert!(update.is_paid());
    assert_eq!(update.amount, 100);
    assert_eq!(update.reference, "ABC123");
}

#[test]
fn status_updates_require_their_documented_fields() {
    let client = client(offline);
    let missing_amount = signed(&[("reference", "INV-1"), ("status", "Paid")]);
    assert!(matches!(
        unwrap_err(client.process_status_update(&missing_amount)),
        Error::InvalidField("amount")
    ));

    let bad_amount = signed(&[
        ("reference", "INV-1"),
        ("amount", "ten"),
        ("status", "Paid"),
    ]);
    assert!(matches!(
        unwrap_err(client.process_status_update(&bad_amount)),
        Error::InvalidField("amount")
    ));

    let missing_reference = signed(&[("amount", "10.00"), ("status", "Paid")]);
    assert!(matches!(
        unwrap_err(client.process_status_update(&missing_reference)),
        Error::InvalidField("reference")
    ));

    let missing_paynow_reference = signed(&[
        ("reference", "INV-1"),
        ("amount", "10.00"),
        ("status", "Paid"),
        ("pollurl", POLL_URL),
    ]);
    assert!(matches!(
        unwrap_err(client.process_status_update(&missing_paynow_reference)),
        Error::InvalidField("paynowreference")
    ));
}

#[test]
fn a_found_trace_must_carry_a_poll_url() {
    let mock = Mock::new(signed(&[
        ("reference", "INV-1"),
        ("paynowreference", "PN-987"),
        ("amount", "10.00"),
        ("status", "Sent"),
        ("pollurl", ""),
    ]));
    assert!(matches!(
        unwrap_err(client(&mock).trace("TRACE-1")),
        Error::InvalidField("pollurl")
    ));
}

#[test]
fn process_status_update_reports_payment() {
    let update = client(offline)
        .process_status_update(&paid_update())
        .unwrap();
    assert!(update.is_paid());
}

#[test]
fn trace_signs_the_request_and_finds_the_transaction() {
    let mock = Mock::new(paid_update());
    let Trace::Found(update) = client(&mock).trace("TRACE-1").unwrap() else {
        panic!("expected the transaction to be found");
    };
    assert!(update.is_paid());
    assert_eq!(update.poll_url, POLL_URL);

    let (url, sent) = mock.sent();
    assert_eq!(url, "https://www.paynow.co.zw/interface/trace");
    assert_eq!(
        sent.iter().map(|(k, _)| k).collect::<Vec<_>>(),
        ["id", "merchanttrace", "status", "hash"]
    );
    assert_eq!(sent.get("id"), Some("12345"));
    assert_eq!(sent.get("merchanttrace"), Some("TRACE-1"));
    assert_eq!(sent.get("status"), Some("Message"));
    assert!(sent.verify(&KEY.to_lowercase()).is_ok());
}

#[test]
fn trace_reports_a_signed_not_found() {
    let mock = Mock::new(signed(&[("status", "NotFound")]));
    assert_eq!(client(&mock).trace("TRACE-1").unwrap(), Trace::NotFound);

    let mock = Mock::new("status=NotFound");
    assert!(matches!(
        client(&mock).trace("TRACE-1"),
        Err(Error::MissingHash)
    ));
}

#[test]
fn trace_failures_are_not_mistaken_for_not_found() {
    let mock = Mock::new("status=Error&error=Trace+failed");
    match client(&mock).trace("TRACE-1") {
        Err(Error::Api(message)) => assert_eq!(message, "Trace failed"),
        other => panic!("expected an API error, got {other:?}"),
    }
}

#[test]
fn request_hash_matches_the_go_sdk() {
    let mock = Mock::new("status=Error&error=stop");
    let client = Client::with_transport("12345", "KEY-ABC", &mock)
        .with_result_url("https://example.com/result");
    let mut payment = Payment::new("INV £1", "a@b.com");
    payment
        .add_with_quantity("T-shirt", 500, 2)
        .add("Delivery", 350);

    let _ = client.send_mobile(&payment, "0771234567", PaymentMethod::Ecocash);
    assert_eq!(
        mock.sent().1.get("hash"),
        Some(
            "E656727B1D39292180D832169A0E6CA74C769ABB8199D99E53A86E4EAAEA2806\
             D1F5A37C2D3701D371F8DEFC20254E1117B0BBAE874FA05F7F720916E8AB4245"
        )
    );
}
