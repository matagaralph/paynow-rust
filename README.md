# Paynow Zimbabwe Rust SDK

A Rust client for the [Paynow Zimbabwe](https://www.paynow.co.zw/) payment gateway.

- **Web payments**: redirect the customer to Paynow to choose how to pay.
- **Mobile (express checkout)**: charge EcoCash, OneMoney or InnBucks directly.
- **Status polling** and **result URL webhooks**, with automatic hash verification.
- **Merchant tracing**, to recover a mobile transaction whose reply was lost.
- A single optional dependency, [`ureq`](https://crates.io/crates/ureq), with a pluggable HTTP transport.

## Installation

```toml
[dependencies]
paynow_zw = "0.1"
```

Requests go through `ureq` using the platform's TLS library: OpenSSL on Linux, SChannel on Windows and Security.framework on macOS. Paynow only accepts TLS 1.2 with CBC cipher suites, which pure Rust TLS (rustls) refuses, so the platform library is required. On Linux, building needs the OpenSSL headers (`libssl-dev` on Debian and Ubuntu, `openssl` on Arch).

For a crate with no dependencies at all, turn off the default feature and supply your own transport (see [Custom transport](#custom-transport)):

```toml
paynow_zw = { version = "0.1", default-features = false }
```

## Getting started

Create a client with your integration credentials, available from your Paynow merchant dashboard:

```rust
use paynow_zw::Client;

let client = Client::new("YOUR_INTEGRATION_ID", "YOUR_INTEGRATION_KEY")
    .with_result_url("https://example.com/paynow/result") // where Paynow posts status updates
    .with_return_url("https://example.com/paynow/return"); // where the customer is sent back to
```

Build up a payment. It behaves like a shopping cart: add one or more items and the total is worked out for you. Amounts are in cents, so there are no floating point rounding errors:

```rust
let mut payment = client.payment("INV-1001", "customer@example.com");
payment
    .add_with_quantity("T-shirt", 1000, 2) // $10.00 each
    .add("Delivery", 350); // $3.50
```

### Web payment

```rust
let response = client.send(&payment)?;

// Send the customer to the redirect URL to pay, and store the poll URL with the order.
println!("{}", response.redirect_url);
```

### Mobile (express checkout)

Mobile transactions charge the customer directly and require a valid auth email. Give each one a merchant trace (at most 32 characters, unique per merchant) so it can be recovered if the reply is lost; see [Recovering after a timeout](#recovering-after-a-timeout).

```rust
use paynow_zw::PaymentMethod;

payment.merchant_trace = Some("INV-1001-1".into());
let response = client.send_mobile(&payment, "0771234567", PaymentMethod::Ecocash)?;

println!("{}", response.poll_url);
if let Some(instructions) = &response.instructions {
    println!("{instructions}"); // for example a USSD prompt for the customer to confirm
}
```

Supported methods: `PaymentMethod::Ecocash`, `PaymentMethod::OneMoney` and `PaymentMethod::Innbucks`.

For InnBucks, the response carries the authorisation code and a deep link that opens the InnBucks app with the code filled in. Show the code as text or render it as a QR code yourself:

```rust
if let Some(innbucks) = &response.innbucks {
    println!("Code: {}", innbucks.authorisation_code);
    println!("Deep link: {}", innbucks.deep_link_url);
}
```


### Recovering after a timeout

If `send_mobile` fails with `Error::Transport`, you cannot tell whether Paynow received the request. Retrying blindly could charge the customer twice. Look the transaction up by its merchant trace instead:

```rust
use paynow_zw::{Error, Trace};

match client.send_mobile(&payment, "0771234567", PaymentMethod::Ecocash) {
    Ok(response) => save_poll_url(&response.poll_url),
    Err(Error::Transport(_)) => match client.trace("INV-1001-1")? {
        Trace::Found(update) => save_poll_url(&update.poll_url),
        Trace::NotFound => retry_later(), // Paynow never received it
    },
    Err(err) => return Err(err),
}
```

Only `Trace::NotFound` proves Paynow has no such transaction. An `Error::Api` from `trace` does not, so in that case try the trace again later rather than starting a new payment.

## Checking transaction status

### Polling

```rust
let update = client.poll_transaction(&response.poll_url)?;

if update.status.is_paid() {
    println!("Paid!");
} else if update.status.is_failed() {
    println!("Failed.");
} else {
    println!("Still pending: {}", update.status);
}
```

### Result URL webhook

When a transaction's status changes, Paynow posts an update to your result URL. Pass the raw request body to `process_status_update` and the hash is verified for you. The body must be passed exactly as received, because the hash covers the order of the fields.

A valid hash is not enough on its own before handing over goods:

- Paynow retries an update up to ten times when your server answers with an HTTP error, so the same update can arrive more than once. Fulfilment must be idempotent.
- Paynow recommends confirming important updates by polling before acting on them.
- The update must match a transaction you created, for the amount you expected.

```rust
// `orders` stands for your own storage.
fn handle_result(body: &str) -> Result<(), Box<dyn std::error::Error>> {
    let update = client.process_status_update(body)?;
    let Some(order) = orders.find(&update.reference)? else {
        return Ok(()); // not one of ours
    };
    if order.fulfilled || !update.is_paid() {
        return Ok(());
    }

    // Confirm with Paynow using the poll URL stored when the payment was made.
    let confirmed = client.poll_transaction(&order.poll_url)?;
    if confirmed.is_paid()
        && confirmed.reference == order.reference
        && confirmed.amount == order.total_cents
    {
        // Mark it fulfilled atomically, for example with
        // `UPDATE orders SET fulfilled = true WHERE id = $1 AND NOT fulfilled`,
        // and only fulfil when that changed a row.
        orders.fulfil_once(&order)?;
    }
    Ok(())
}
```

Answer with a success status once the update is handled. Return an HTTP error only when you want Paynow to send it again.

## Error handling

Every fallible call returns `paynow_zw::Result`, whose error type is the `paynow_zw::Error` enum:

```rust
use paynow_zw::Error;

match client.send(&payment) {
    Ok(response) => println!("{}", response.poll_url),
    Err(Error::Api(reason)) => println!("Paynow said: {reason}"),
    Err(err) => println!("Payment failed: {err}"),
}
```

| Variant | Meaning |
|---------|---------|
| `Error::EmptyCart` | The payment has no items. |
| `Error::ZeroTotal` | The payment's total is zero. |
| `Error::InvalidEmail` | A mobile payment lacks a valid auth email. |
| `Error::InvalidMerchantTrace` | The merchant trace is empty or longer than 32 characters. |
| `Error::MissingHash` | A response that should be signed had no hash. |
| `Error::HashMismatch` | A response hash did not match, so it may have been tampered with. |
| `Error::MalformedResponse` | A response could not be decoded. |
| `Error::InvalidField(name)` | A signed response lacked a field the operation needs, such as the poll URL, or held an unusable value. |
| `Error::UnexpectedStatus(status)` | A signed response carried a status the operation does not expect. |
| `Error::Api(reason)` | Paynow rejected the request, for example because of an invalid integration ID. |
| `Error::Transport(err)` | The request failed, timed out or got an HTTP error from something other than Paynow. Paynow may still have received it. |

## Custom transport

`Client::new` uses the default `Ureq` transport, which times out after 30 seconds. To choose another timeout:

```rust
use std::time::Duration;
use paynow_zw::Ureq;

let client = Client::with_transport(id, key, Ureq::with_timeout(Duration::from_secs(60)));
```

Paynow answers its own errors with HTTP 200, so `Ureq` treats any other HTTP status as a transport error, unless the body is a Paynow reply.

`Ureq` also converts from your own `ureq::Agent`, for proxies or other settings. Bear in mind that it must use the native TLS provider to reach Paynow.

To use another HTTP client, implement `Transport`, or pass a closure that posts a form-encoded body and returns the response body. Closures also make the client easy to mock in tests:

```rust
let client = Client::with_transport(id, key, |url: &str, body: &str| {
    // post `body` to `url` with Content-Type: application/x-www-form-urlencoded
    Ok(String::from("status=Error&error=Not+implemented"))
});
```

## Crate layout

| File | Responsibility |
|------|----------------|
| `client.rs` | `Client`: sending, polling, tracing and status updates |
| `payment.rs` | Building a payment and its cart, amounts and email validation |
| `response.rs` | `WebResponse`, `MobileResponse`, `StatusResponse`, `Trace` and `InnBucks` |
| `status.rs` | Payment methods and transaction statuses |
| `form.rs` | Ordered form encoding and response parsing |
| `hash.rs` | SHA-512 request and response signing |
| `transport.rs` | `Transport` and the default `Ureq` transport |
| `error.rs` | `Error` and `Result` |

A complete, runnable flow lives in [`examples/mobile.rs`](examples/mobile.rs):

```bash
cargo run --example mobile
```

## Licence

MIT. See [LICENSE](LICENSE).
