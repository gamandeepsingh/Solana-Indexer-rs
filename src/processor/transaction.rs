use chrono::Utc;
use colored::Colorize;

use crate::models::transaction::Transaction;
use crate::processor::filters::is_large_transfer;

pub fn print_tx(tx: &Transaction, console_log: bool) {
    if !console_log {
        return;
    }

    let ts = Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let sig = truncate(&tx.signature);

    if tx.failed {
        println!(
            "{} {} - {}  {} SOL",
            "[TX]".red(),
            ts.dimmed(),
            sig.dimmed(),
            format!("{:.4}", tx.amount).dimmed(),
        );
        return;
    }

    if let Some(ref memo) = tx.memo {
        println!(
            "{} {} - {}",
            "[MEMO]".magenta().bold(),
            ts.dimmed(),
            memo.white()
        );
    }

    if is_large_transfer(tx.amount) {
        let from = tx.from.as_deref().unwrap_or("unknown");
        let to = tx.to.as_deref().unwrap_or("unknown");
        println!(
            "{} {}\n  {} {}\n  {}   {}\n  {} {} SOL",
            "[TRANSFER]".yellow().bold(),
            ts.dimmed(),
            "From:".dimmed(),
            from.cyan(),
            "To:".dimmed(),
            to.cyan(),
            "Amount:".dimmed(),
            format!("{:.4}", tx.amount).green().bold(),
        );
    }

    if tx.memo.is_none() && !is_large_transfer(tx.amount) {
        println!(
            "{} {} - {}  {} SOL",
            "[TX]".white(),
            ts.dimmed(),
            sig.dimmed(),
            format!("{:.4}", tx.amount).white(),
        );
    }
}

pub(crate) fn truncate(s: &str) -> String {
    if s.len() <= 20 {
        return s.to_string();
    }
    format!("{}...{}", &s[..8], &s[s.len() - 8..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_long_signature() {
        let sig = "3vpDTvHgGjB3VeGa6sCjdST1H78CFM1ViWpXBRXBSPqM5J2cnqKs4enBaHKM6LXS9ABt7TCQDkz5oniFUqVwTY28";
        let result = truncate(sig);
        // format: first 8 + "..." + last 8
        assert_eq!(result.len(), 8 + 3 + 8);
        assert_eq!(&result[..8], &sig[..8]);
        assert_eq!(&result[result.len() - 8..], &sig[sig.len() - 8..]);
        assert!(result.contains("..."));
    }

    #[test]
    fn keeps_short_signature() {
        assert_eq!(truncate("short"), "short");
        assert_eq!(truncate("exactly20charsXXXXXX"), "exactly20charsXXXXXX");
    }

    #[test]
    fn large_transfer_sets_display_path() {
        assert!(is_large_transfer(5.0));
        assert!(!is_large_transfer(0.0001));
    }
}
