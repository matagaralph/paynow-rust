use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub title: String,
    /// Unit price in cents.
    pub amount: u64,
    /// Zero counts as one.
    pub quantity: u32,
}

impl Item {
    /// In cents.
    pub fn subtotal(&self) -> u64 {
        self.amount.saturating_mul(u64::from(self.quantity.max(1)))
    }
}

/// A transaction built up like a shopping cart before it is sent to Paynow.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Payment {
    pub reference: String,
    /// Required for mobile transactions, optional for web ones.
    pub auth_email: String,
    /// Sent with mobile transactions so [`crate::Client::trace`] can recover one whose response
    /// was lost. At most 32 characters and unique per merchant.
    pub merchant_trace: Option<String>,
    items: Vec<Item>,
}

impl Payment {
    pub fn new(reference: impl Into<String>, auth_email: impl Into<String>) -> Self {
        Self {
            reference: reference.into(),
            auth_email: auth_email.into(),
            merchant_trace: None,
            items: Vec::new(),
        }
    }

    /// Adds an item priced in cents, so `add("Bread", 150)` is $1.50.
    pub fn add(&mut self, title: impl Into<String>, amount: u64) -> &mut Self {
        self.add_with_quantity(title, amount, 1)
    }

    pub fn add_with_quantity(
        &mut self,
        title: impl Into<String>,
        amount: u64,
        quantity: u32,
    ) -> &mut Self {
        self.items.push(Item {
            title: title.into(),
            amount,
            quantity,
        });
        self
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// In cents.
    pub fn total(&self) -> u64 {
        self.items
            .iter()
            .fold(0, |total, item| total.saturating_add(item.subtotal()))
    }

    /// The item titles, comma separated, shown to the customer by Paynow.
    pub fn info(&self) -> String {
        let mut info = String::new();
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                info.push_str(", ");
            }
            info.push_str(&item.title);
        }
        info
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.items.is_empty() {
            Err(Error::EmptyCart)
        } else if self.total() == 0 {
            Err(Error::ZeroTotal)
        } else if self
            .merchant_trace
            .as_ref()
            .is_some_and(|trace| trace.is_empty() || trace.chars().count() > 32)
        {
            Err(Error::InvalidMerchantTrace)
        } else {
            Ok(())
        }
    }
}

pub(crate) fn format_cents(cents: u64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

pub(crate) fn parse_cents(amount: &str) -> Option<u64> {
    let amount = amount.trim();
    let (units, fraction) = amount.split_once('.').unwrap_or((amount, ""));
    if units.is_empty() || !units.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let fraction = match *fraction.as_bytes() {
        [] => 0,
        [tenths] if tenths.is_ascii_digit() => u64::from(tenths - b'0') * 10,
        [tenths, hundredths] if tenths.is_ascii_digit() && hundredths.is_ascii_digit() => {
            u64::from(tenths - b'0') * 10 + u64::from(hundredths - b'0')
        }
        _ => return None,
    };
    units
        .parse::<u64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(fraction)
}

/// A basic shape check only; Paynow decides which addresses it accepts.
pub(crate) fn is_valid_email(address: &str) -> bool {
    let Some((local, domain)) = address.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !address.contains(|c: char| c.is_whitespace() || c.is_control())
        && !domain.contains('@')
        && domain.contains('.')
        && domain.split('.').all(|label| !label.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_and_info() {
        let mut payment = Payment::new("INV-1", "buyer@example.com");
        payment
            .add_with_quantity("Widget", 550, 2)
            .add("Sticker", 100);

        assert_eq!(payment.total(), 1200);
        assert_eq!(payment.info(), "Widget, Sticker");
        assert_eq!(payment.items().len(), 2);
    }

    #[test]
    fn zero_quantity_counts_as_one() {
        let mut payment = Payment::new("INV-2", "");
        payment.add_with_quantity("Book", 2000, 0);
        assert_eq!(payment.total(), 2000);
    }

    #[test]
    fn validation() {
        let mut payment = Payment::new("INV", "");
        assert!(matches!(payment.validate(), Err(Error::EmptyCart)));
        payment.add("Free", 0);
        assert!(matches!(payment.validate(), Err(Error::ZeroTotal)));
        payment.add("Pen", 200);
        assert!(payment.validate().is_ok());

        payment.merchant_trace = Some("x".repeat(33));
        assert!(matches!(
            payment.validate(),
            Err(Error::InvalidMerchantTrace)
        ));
        payment.merchant_trace = Some(String::new());
        assert!(matches!(
            payment.validate(),
            Err(Error::InvalidMerchantTrace)
        ));
        payment.merchant_trace = Some("x".repeat(32));
        assert!(payment.validate().is_ok());
    }

    #[test]
    fn cents_round_trip() {
        assert_eq!(format_cents(0), "0.00");
        assert_eq!(format_cents(5), "0.05");
        assert_eq!(format_cents(1350), "13.50");
        assert_eq!(format_cents(123_456_789), "1234567.89");

        assert_eq!(parse_cents("13.50"), Some(1350));
        assert_eq!(parse_cents(" 13.5 "), Some(1350));
        assert_eq!(parse_cents("13"), Some(1300));
        assert_eq!(parse_cents("0.05"), Some(5));
        for invalid in ["", ".50", "13.505", "-1.00", "+1.00", "1,000.00", "1e3"] {
            assert_eq!(parse_cents(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn email_validation() {
        for valid in [
            "buyer@example.com",
            "first.last@mail.example.co.zw",
            "a_b-c@x-y.org",
            "buyer@example.info",
            "buyer+receipts@example.com",
            "o'brien@example.ie",
            "bü@exämple.com",
        ] {
            assert!(is_valid_email(valid), "{valid}");
        }
        for invalid in [
            "",
            "not-an-email",
            "@example.com",
            "buyer@",
            "buyer@example",
            "buyer@.example.com",
            "buyer@example.",
            "buyer@example..com",
            "a@b@example.com",
            "buyer @example.com",
            "buyer@example.com\n",
        ] {
            assert!(!is_valid_email(invalid), "{invalid}");
        }
    }
}
