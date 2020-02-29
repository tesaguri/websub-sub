table! {
    active_subscriptions (id) {
        id -> BigInt,
        expires_at -> BigInt,
    }
}

table! {
    subscriptions (id) {
        id -> BigInt,
        hub -> Text,
        topic -> Text,
        secret -> Text,
    }
}

joinable!(active_subscriptions -> subscriptions (id));

allow_tables_to_appear_in_same_query!(
    active_subscriptions,
    subscriptions,
);
