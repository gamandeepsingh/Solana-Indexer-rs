pub struct Account {
    pub pubkey: String,
    pub lamports: i64,
    pub slot: i64,
    pub executable: bool,
    pub rent_epoch: i64,
}
