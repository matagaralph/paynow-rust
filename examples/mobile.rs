use std::{thread, time::Duration};

use paynow_zw::{Client, Error, PaymentMethod, Trace};

fn main() -> Result<(), Error> {
    let client = Client::new("YOUR_INTEGRATION_ID", "YOUR_INTEGRATION_KEY")
        .with_result_url("https://example.com/paynow/result")
        .with_return_url("https://example.com/paynow/return");

    let mut payment = client.payment("INV-1001", "customer@example.com");
    payment
        .add_with_quantity("T-shirt", 1000, 2)
        .add("Delivery", 350);
    payment.merchant_trace = Some("INV-1001-1".into());

    let poll_url = match client.send_mobile(&payment, "0771234567", PaymentMethod::Ecocash) {
        Ok(response) => {
            if let Some(instructions) = &response.instructions {
                println!("Instructions: {instructions}");
            }
            response.poll_url
        }
        Err(Error::Transport(err)) => {
            println!("No reply from Paynow ({err}); tracing the transaction.");
            match client.trace("INV-1001-1")? {
                Trace::Found(update) => update.poll_url,
                Trace::NotFound => {
                    println!("Paynow never received it, so it is safe to try again.");
                    return Ok(());
                }
            }
        }
        Err(err) => return Err(err),
    };
    println!("Poll URL: {poll_url}");

    loop {
        let update = client.poll_transaction(&poll_url)?;
        println!("Status: {}", update.status);

        if update.is_paid() {
            println!("Payment successful!");
            return Ok(());
        }
        if update.status.is_failed() {
            println!("Payment failed.");
            return Ok(());
        }
        thread::sleep(Duration::from_secs(5));
    }
}
