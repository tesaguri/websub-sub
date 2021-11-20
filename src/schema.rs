table! {
    active_subscriptions (id) {
        id -> BigInt,
        expires_at -> BigInt,
    }
}

table! {
    pending_subscriptions (id) {
        id -> BigInt,
        created_at -> BigInt,
    }
}

table! {
    renewing_subscriptions (new) {
        old -> BigInt,
        new -> BigInt,
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
joinable!(pending_subscriptions -> subscriptions (id));
joinable!(renewing_subscriptions -> active_subscriptions (old));
joinable!(renewing_subscriptions -> pending_subscriptions (new));

allow_tables_to_appear_in_same_query!(
    active_subscriptions,
    pending_subscriptions,
    renewing_subscriptions,
    subscriptions,
);
