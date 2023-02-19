use std::str;

use futures::{Future, TryFutureExt};
use http::header::{CONTENT_TYPE, LOCATION};
use http::uri::{Parts, PathAndQuery, Uri};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tower::ServiceExt;

use crate::db::Connection;
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

pub fn subscribe<C, S, B>(
    callback: &Uri,
    hub: String,
    topic: String,
    client: S,
    conn: &C,
) -> Result<impl Future<Output = Result<(), S::Error>>, C::Error>
where
    C: Connection,
    S: HttpService<B>,
    B: From<Vec<u8>>,
{
    let (id, secret) = match create_subscription(&hub, &topic, conn) {
        Ok((id, secret)) => (id as u64, secret),
        Err(e) => return Err(e),
    };

    log::info!("Subscribing to topic {} at hub {} ({})", topic, hub, id);

    let body = serde_urlencoded::to_string(Form::Subscribe {
        callback: make_callback(callback.clone(), id),
        topic: &*topic,
        secret: &*secret,
    })
    .unwrap();

    Ok(send_request(hub, topic, body, client))
}

pub fn unsubscribe<C, S, B>(
    callback: &Uri,
    id: u64,
    hub: String,
    topic: String,
    client: S,
    conn: &C,
) -> Result<impl Future<Output = Result<(), S::Error>>, C::Error>
where
    C: Connection,
    S: HttpService<B>,
    B: From<Vec<u8>>,
{
    log::info!("Unsubscribing from topic {} at hub {} ({})", topic, hub, id);

    conn.delete_subscriptions(id)?;

    let callback = make_callback(callback.clone(), id);
    let body = serde_urlencoded::to_string(Form::Unsubscribe {
        callback,
        topic: &topic,
    })
    .unwrap();

    Ok(send_request(hub, topic, body, client))
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

fn create_subscription<C>(hub: &str, topic: &str, conn: &C) -> Result<(u64, Secret), C::Error>
where
    C: Connection,
{
    let mut rng = rand::thread_rng();

    let secret = gen_secret(&mut rng);
    let id = conn.create_subscription(hub, topic, &secret)?;

    Ok((id, secret))
}

fn make_callback(prefix: Uri, id: u64) -> Uri {
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

    unsafe {
        // We cannot assume in unsafe code that the safe code of `base64` crate produces a valid
        // UTF-8 string.
        str::from_utf8(&ret).unwrap();

        // The `unchecked` is still required because `[u8; 32]` doesn't implement
        // `string::StableAsRef`.
        //
        // TODO: Use `string::TryFrom` once the `StableAsRef` implementation lands.
        // cf. <https://github.com/carllerche/string/pull/28>
        //
        // SAFETY:
        //
        // `[u8; 32]` satisfies the requirements of `StableAsRef` trait... maybe. At least, `string`
        // crate itself implements it for `[u8; N]` where `N <= 16`. This seems to be a reasonable
        // assumption to put on a standard library to keep holding in the past, present and future.
        // See also the discussion on trusting the impl of primitive types in the Rustonomicon:
        // <https://doc.rust-lang.org/1.67.1/nomicon/safe-unsafe-meaning.html>
        string::String::from_utf8_unchecked(ret)
    }
}
