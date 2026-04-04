pub struct Transaction {
    pub signature: String,
    pub slot: i64,
    pub amount: f64,
    pub failed: bool,
    pub memo: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}
