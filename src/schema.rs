diesel::table! {
    subscriptions (id) {
        id -> BigInt,
        hub -> Text,
        topic -> Text,
        secret -> Text,
        expires_at -> Nullable<BigInt>,
    }
}

// XXX: We need to manually implement this until the following patch lands:
// <https://github.com/diesel-rs/diesel/pull/3400>
impl Default for subscriptions::table {
    fn default() -> Self {
        subscriptions::table
    }
}
