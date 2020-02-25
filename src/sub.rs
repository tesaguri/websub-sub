use std::str;

use futures::{future, TryStreamExt};
use hyper_tls::HttpsConnector;

#[derive(serde::Serialize)]
#[serde(tag = "hub.mode")]
pub enum Sub<'a> {
    #[serde(rename = "subscribe")]
    Subscribe {
        #[serde(rename = "hub.callback")]
        callback: &'a str,
        #[serde(rename = "hub.topic")]
        topic: &'a str,
        #[serde(rename = "hub.secret")]
        secret: &'a str,
    },
    #[serde(rename = "unsubscribe")]
    Unsubscribe {
        #[serde(rename = "hub.callback")]
        callback: &'a str,
        #[serde(rename = "hub.topic")]
        topic: &'a str,
    },
}

pub async fn send(hub: &str, sub: &Sub<'_>) {
    let conn = HttpsConnector::new();
    let client = hyper::Client::builder().build(conn);

    let body = serde_urlencoded::to_string(sub).unwrap();
    let req = hyper::Request::post(hub)
        .body(hyper::Body::from(body))
        .unwrap();

    let res = client.request(req).await.unwrap();
    if res.status().is_success() {
        eprintln!("success");
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
    eprintln!("error: {}", body);
}
