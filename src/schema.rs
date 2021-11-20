table! {
    subscriptions (id) {
        id -> BigInt,
        hub -> Text,
        topic -> Text,
        secret -> Text,
        expires_at -> Nullable<BigInt>,
    }
}
