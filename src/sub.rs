use std::convert::TryInto;
use std::str;

use diesel::prelude::*;
use diesel::result::DatabaseErrorKind;
use diesel::SqliteConnection;
use futures::{future, Future, TryStreamExt};
use http::header::CONTENT_TYPE;
use http::uri::{Parts, Uri};
use hyper::client::connect::Connect;
use rand::RngCore;

use crate::schema::*;

#[derive(serde::Serialize)]
#[serde(tag = "hub.mode")]
#[serde(rename_all = "lowercase")]
enum Form<'a> {
    Subscribe {
        #[serde(rename = "hub.callback")]
        #[serde(serialize_with = "serialize_uri")]
        callback: &'a Uri,
        #[serde(rename = "hub.topic")]
        topic: &'a str,
        #[serde(rename = "hub.secret")]
        secret: &'a str,
    },
    Unsubscribe {
        #[serde(rename = "hub.callback")]
        #[serde(serialize_with = "serialize_uri")]
        callback: &'a Uri,
        #[serde(rename = "hub.topic")]
        topic: &'a str,
    },
}

pub fn subscribe<C>(
    host: &Uri,
    hub: &str,
    topic: &str,
    client: &hyper::Client<C>,
    conn: &SqliteConnection,
) -> impl Future<Output = ()>
where
    C: Connect + Clone + Send + Sync + 'static,
{
    let mut rng = rand::thread_rng();

    let secret = gen_secret(&mut rng);
    let secret = unsafe { str::from_utf8_unchecked(&secret) };
    // TODO: transaction
    let id = loop {
        let id = (rng.next_u64() >> 1) as i64;
        let result = diesel::insert_into(subscriptions::table)
            .values((
                subscriptions::id.eq(id),
                subscriptions::hub.eq(hub),
                subscriptions::topic.eq(topic),
                subscriptions::secret.eq(secret),
            ))
            .execute(conn);
        match result {
            Ok(_) => break id,
            Err(diesel::result::Error::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
                // retry
            }
            Err(e) => panic!("{:?}", e),
        }
    };

    diesel::insert_into(pending_subscriptions::table)
        .values(pending_subscriptions::id.eq(id))
        .execute(conn)
        .unwrap();

    log::info!("Subscribing to topic {} at hub {} ({})", topic, hub, id);

    let body = serde_urlencoded::to_string(Form::Subscribe {
        callback: &callback(host.clone(), id),
        topic,
        secret,
    })
    .unwrap();

    send_request(hub, body, client)
}

pub fn unsubscribe<C>(
    host: &Uri,
    id: i64,
    hub: &str,
    topic: &str,
    client: &hyper::Client<C>,
) -> impl Future<Output = ()>
where
    C: Connect + Clone + Send + Sync + 'static,
{
    log::info!("Unsubscribing from topic {} at hub {} ({})", topic, hub, id);

    let callback = &callback(host.clone(), id);
    let body = serde_urlencoded::to_string(Form::Unsubscribe { callback, topic }).unwrap();
    send_request(hub, body, client)
}

pub fn unsubscribe_all<'a, C>(
    host: &'a Uri,
    hub: &'a str,
    topic: &'a str,
    client: &'a hyper::Client<C>,
    conn: &SqliteConnection,
) -> impl Future<Output = ()> + 'a
where
    C: Connect + Clone + Send + Sync + 'static,
{
    log::info!("Unsubscribing from topic {} at hub {}", topic, hub);

    let rows = subscriptions::table
        .filter(subscriptions::hub.eq(hub))
        .filter(subscriptions::topic.eq(topic));
    let ids = rows.select(subscriptions::id).load::<i64>(conn).unwrap();
    // TODO: transaction
    diesel::delete(rows).execute(conn).unwrap();

    async move {
        for id in ids {
            let callback = &callback(host.clone(), id);
            let body = serde_urlencoded::to_string(Form::Unsubscribe { callback, topic }).unwrap();
            send_request(hub, body, client).await;
        }
    }
}

fn send_request<C>(hub: &str, body: String, client: &hyper::Client<C>) -> impl Future<Output = ()>
where
    C: Connect + Clone + Send + Sync + 'static,
{
    let req = http::Request::post(hub)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(hyper::Body::from(body))
        .unwrap();

    let res = client.request(req);

    async {
        let res = res.await.unwrap();
        let status = res.status();

        if status.is_success() {
            log::info!("Request succeeded");
            return;
        }

        // TODO: handle redirect

        let body = res
            .into_body()
            .try_fold(String::new(), |s, t| {
                future::ok(s + str::from_utf8(&t).unwrap())
            })
            .await
            .unwrap();
        log::error!("HTTP error {}: {}", status, body);
    }
}

fn callback(host: Uri, id: i64) -> Uri {
    let mut parts = Parts::from(host);
    parts.path_and_query = Some((*format!("/websub/callback/{}", id)).try_into().unwrap());
    parts.try_into().unwrap()
}

const SECRET_LEN: usize = 32;

fn gen_secret<R: RngCore>(mut rng: R) -> [u8; SECRET_LEN] {
    let mut ret = [0u8; SECRET_LEN];

    let mut rand = [0u8; SECRET_LEN * 6 / 8];
    rng.fill_bytes(&mut rand);

    let config = base64::Config::new(base64::CharacterSet::UrlSafe, false);
    base64::encode_config_slice(&rand, config, &mut ret);

    ret
}

fn serialize_uri<S: serde::Serializer>(uri: &Uri, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(uri)
}
