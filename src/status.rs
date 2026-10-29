use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaymentMethod {
    Ecocash,
    OneMoney,
    /// Responses carry an authorisation code, surfaced as [`crate::InnBucks`].
    Innbucks,
}

impl PaymentMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ecocash => "ecocash",
            Self::OneMoney => "onemoney",
            Self::Innbucks => "innbucks",
        }
    }
}

impl fmt::Display for PaymentMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Parsed case-insensitively, as Paynow is inconsistent about casing.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransactionStatus {
    Created,
    Sent,
    Pending,
    Paid,
    AwaitingDelivery,
    Delivered,
    Cancelled,
    Failed,
    Refunded,
    Disputed,
    Other(String),
}

impl TransactionStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Created => "Created",
            Self::Sent => "Sent",
            Self::Pending => "Pending",
            Self::Paid => "Paid",
            Self::AwaitingDelivery => "Awaiting Delivery",
            Self::Delivered => "Delivered",
            Self::Cancelled => "Cancelled",
            Self::Failed => "Failed",
            Self::Refunded => "Refunded",
            Self::Disputed => "Disputed",
            Self::Other(status) => status,
        }
    }

    pub fn is_paid(&self) -> bool {
        matches!(self, Self::Paid | Self::AwaitingDelivery | Self::Delivered)
    }

    /// Still in progress and worth polling again.
    pub fn is_pending(&self) -> bool {
        matches!(self, Self::Created | Self::Sent | Self::Pending)
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Cancelled | Self::Failed | Self::Disputed)
    }
}

impl From<&str> for TransactionStatus {
    fn from(status: &str) -> Self {
        let status = status.trim();
        [
            Self::Created,
            Self::Sent,
            Self::Pending,
            Self::Paid,
            Self::AwaitingDelivery,
            Self::Delivered,
            Self::Cancelled,
            Self::Failed,
            Self::Refunded,
            Self::Disputed,
        ]
        .into_iter()
        .find(|known| known.as_str().eq_ignore_ascii_case(status))
        .unwrap_or_else(|| Self::Other(status.to_owned()))
    }
}

impl fmt::Display for TransactionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates() {
        for (status, paid, pending, failed) in [
            ("Created", false, true, false),
            ("Sent", false, true, false),
            ("Pending", false, true, false),
            ("Paid", true, false, false),
            ("Awaiting Delivery", true, false, false),
            ("Delivered", true, false, false),
            ("Cancelled", false, false, true),
            ("Failed", false, false, true),
            ("Disputed", false, false, true),
            ("Refunded", false, false, false),
            ("Unknown", false, false, false),
        ] {
            let status = TransactionStatus::from(status);
            assert_eq!(status.is_paid(), paid, "{status}");
            assert_eq!(status.is_pending(), pending, "{status}");
            assert_eq!(status.is_failed(), failed, "{status}");
        }
    }

    #[test]
    fn parses_case_insensitively() {
        assert_eq!(TransactionStatus::from(" paid "), TransactionStatus::Paid);
        assert_eq!(
            TransactionStatus::from("AWAITING DELIVERY"),
            TransactionStatus::AwaitingDelivery
        );
        assert_eq!(
            TransactionStatus::from("Unknown"),
            TransactionStatus::Other("Unknown".into())
        );
    }
}
