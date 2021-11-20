use std::str;

use diesel::prelude::*;
use diesel::result::DatabaseErrorKind;
use diesel::SqliteConnection;
use futures::{Future, TryFutureExt};
use http::header::{CONTENT_TYPE, LOCATION};
use http::uri::{Parts, PathAndQuery, Uri};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tower::ServiceExt;

use crate::schema::*;
use crate::util;
use crate::util::consts::APPLICATION_WWW_FORM_URLENCODED;
use crate::util::HttpService;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "hub.mode")]
#[serde(rename_all = "lowercase")]
pub enum Form<S = String> {
    Subscribe {
        #[serde(rename = "hub.callback")]
        #[serde(with = "http_serde::uri")]
        callback: Uri,
        #[serde(rename = "hub.topic")]
        topic: S,
        #[serde(rename = "hub.secret")]
        secret: S,
    },
    Unsubscribe {
        #[serde(rename = "hub.callback")]
        #[serde(with = "http_serde::uri")]
        callback: Uri,
        #[serde(rename = "hub.topic")]
        topic: S,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "hub.mode")]
#[serde(rename_all = "lowercase")]
pub enum Verify<S = String> {
    Subscribe {
        #[serde(rename = "hub.topic")]
        topic: S,
        #[serde(rename = "hub.challenge")]
        challenge: S,
        #[serde(rename = "hub.lease_seconds")]
        #[serde(deserialize_with = "crate::util::deserialize_from_str")]
        lease_seconds: u64,
    },
    Unsubscribe {
        #[serde(rename = "hub.topic")]
        topic: S,
        #[serde(rename = "hub.challenge")]
        challenge: S,
    },
}

const SECRET_LEN: usize = 32;
type Secret = string::String<[u8; SECRET_LEN]>;

pub fn subscribe<S, B>(
    callback: &Uri,
    hub: String,
    topic: String,
    client: S,
    conn: &SqliteConnection,
) -> impl Future<Output = Result<(), S::Error>>
where
    S: HttpService<B>,
    B: From<Vec<u8>>,
{
    let (id, secret) = create_subscription(&hub, &topic, conn);

    log::info!("Subscribing to topic {} at hub {} ({})", topic, hub, id);

    let body = serde_urlencoded::to_string(Form::Subscribe {
        callback: make_callback(callback.clone(), id),
        topic: &*topic,
        secret: &*secret,
    })
    .unwrap();

    send_request(hub, topic, body, client)
}

pub fn unsubscribe<S, B>(
    callback: &Uri,
    id: i64,
    hub: String,
    topic: String,
    client: S,
    conn: &SqliteConnection,
) -> impl Future<Output = Result<(), S::Error>>
where
    S: HttpService<B>,
    B: From<Vec<u8>>,
{
    log::info!("Unsubscribing from topic {} at hub {} ({})", topic, hub, id);

    diesel::delete(subscriptions::table.find(id))
        .execute(conn)
        .unwrap();

    let callback = make_callback(callback.clone(), id);
    let body = serde_urlencoded::to_string(Form::Unsubscribe {
        callback,
        topic: &topic,
    })
    .unwrap();
    send_request(hub, topic, body, client)
}

pub fn unsubscribe_all<S, B>(
    callback: &Uri,
    topic: String,
    client: S,
    conn: &SqliteConnection,
) -> impl Iterator<Item = impl Future<Output = Result<(), S::Error>>>
where
    S: HttpService<B> + Clone,
    B: From<Vec<u8>>,
{
    log::info!("Unsubscribing from topic {} at all hubs", topic);

    let rows = subscriptions::table.filter(subscriptions::topic.eq(&topic));
    let subscriptions = rows
        .select((subscriptions::id, subscriptions::hub))
        .load::<(i64, String)>(conn)
        .unwrap();

    diesel::delete(rows).execute(conn).unwrap();

    let callback = callback.clone();
    subscriptions.into_iter().map(move |(id, hub)| {
        let callback = make_callback(callback.clone(), id);
        let body = serde_urlencoded::to_string(Form::Unsubscribe {
            callback,
            topic: &topic,
        })
        .unwrap();
        send_request(hub, topic.clone(), body, client.clone())
    })
}

fn send_request<S, B>(
    hub: String,
    topic: String,
    body: String,
    client: S,
) -> impl Future<Output = Result<(), S::Error>>
where
    S: HttpService<B>,
    B: From<Vec<u8>>,
{
    let req = http::Request::post(&hub)
        .header(CONTENT_TYPE, APPLICATION_WWW_FORM_URLENCODED)
        .body(B::from(body.into_bytes()))
        .unwrap();

    client.into_service().oneshot(req).map_ok(move |res| {
        let status = res.status();

        if status.is_success() {
            return;
        }

        if status.is_redirection() {
            // TODO: more proper handling.
            if let Some(to) = res.headers().get(LOCATION) {
                let to = String::from_utf8_lossy(to.as_bytes());
                log::warn!("Topic {} at hub {} redirects to {}", topic, hub, to);
            }
        }

        log::warn!(
            "Topic {} at hub {} returned HTTP status code {}",
            topic,
            hub,
            status
        );
    })
}

fn create_subscription(hub: &str, topic: &str, conn: &SqliteConnection) -> (i64, Secret) {
    let mut rng = rand::thread_rng();

    let secret = gen_secret(&mut rng);

    let id = conn
        .transaction(|| {
            let id = loop {
                let id = (rng.next_u64() >> 1) as i64;
                let result = diesel::insert_into(subscriptions::table)
                    .values((
                        subscriptions::id.eq(id),
                        subscriptions::hub.eq(hub),
                        subscriptions::topic.eq(topic),
                        subscriptions::secret.eq(&*secret),
                    ))
                    .execute(conn);
                match result {
                    Ok(_) => break id,
                    Err(diesel::result::Error::DatabaseError(
                        DatabaseErrorKind::UniqueViolation,
                        _,
                    )) => {} // retry
                    Err(e) => return Err(e),
                }
            };

            Ok(id)
        })
        .unwrap();

    (id, secret)
}

fn make_callback(prefix: Uri, id: i64) -> Uri {
    let id = id.to_le_bytes();
    let id = util::callback_id::encode(&id);
    let mut parts = Parts::from(prefix);
    // `subscriber::prepare_callback_prefix` ensures that `path_and_query` is `Some`.
    let path = format!("{}{}", parts.path_and_query.unwrap(), id);
    parts.path_and_query = Some(PathAndQuery::try_from(path).unwrap());
    parts.try_into().unwrap()
}

fn gen_secret<R: RngCore>(mut rng: R) -> Secret {
    let mut ret = [0_u8; SECRET_LEN];

    let mut rand = [0_u8; SECRET_LEN * 6 / 8];
    rng.fill_bytes(&mut rand);

    base64::encode_config_slice(&rand, base64::URL_SAFE_NO_PAD, &mut ret);

    unsafe { string::String::from_utf8_unchecked(ret) }
}