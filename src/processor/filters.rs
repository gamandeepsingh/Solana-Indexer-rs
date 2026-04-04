pub const LARGE_TRANSFER_THRESHOLD_SOL: f64 = 1.0;

pub fn is_large_transfer(amount: f64) -> bool {
    amount > LARGE_TRANSFER_THRESHOLD_SOL
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_large_transfer() {
        assert!(is_large_transfer(1.1));
        assert!(is_large_transfer(10.0));
        assert!(is_large_transfer(100.0));
        assert!(is_large_transfer(1000.0));
    }

    #[test]
    fn ignores_small_transfer() {
        assert!(!is_large_transfer(0.0));
        assert!(!is_large_transfer(0.000005));
        assert!(!is_large_transfer(0.5));
        assert!(!is_large_transfer(1.0));
    }
}
