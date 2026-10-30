//! Client for the [Paynow](https://www.paynow.co.zw/) Zimbabwe payment gateway.
//!
//! ```no_run
//! # #[cfg(feature = "ureq")] {
//! use paynow_zw::{Client, PaymentMethod};
//!
//! let client = Client::new("INTEGRATION_ID", "INTEGRATION_KEY")
//!     .with_result_url("https://example.com/paynow/result")
//!     .with_return_url("https://example.com/paynow/return");
//!
//! let mut payment = client.payment("INV-1001", "customer@example.com");
//! payment.add("T-shirt", 1000);
//!
//! let response = client.send_mobile(&payment, "0771234567", PaymentMethod::Ecocash)?;
//! let update = client.poll_transaction(&response.poll_url)?;
//! println!("{}", update.status);
//! # }
//! # Ok::<(), paynow_zw::Error>(())
//! ```
//!
//! Requests go through `ureq` by default. To use another HTTP client, sync or async, create the
//! client with [`Client::without_transport`], build each request with a `*_request` method such
//! as [`Client::mobile_request`], send it yourself and pass the reply to the matching `parse_*`
//! method. Disable the `ureq` feature if you do not need it.

mod client;
mod error;
mod form;
mod hash;
mod payment;
mod response;
mod status;
mod transport;

#[cfg(test)]
mod tests;

pub use client::{Client, Request};
pub use error::{Error, Result};
pub use form::Fields;
pub use payment::{Item, Payment};
pub use response::{InnBucks, MobileResponse, StatusResponse, Trace, WebResponse};
pub use status::{PaymentMethod, TransactionStatus};
pub use transport::Transport;
#[cfg(feature = "ureq")]
pub use transport::Ureq;
