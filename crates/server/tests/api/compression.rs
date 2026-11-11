//! The compression of API answers: JSON is compressed as the client's
//! `Accept-Encoding` asks, nothing else is, and the configuration can turn it off.

use std::io::Read as _;

use fvoci_model::BoardType;
use reqwest::Method;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, VARY};

use crate::common::{ApiResponse, Client, Setup, TEST_TEAM_ID};

/// `GET path` asking for `encoding`.
async fn get_encoded(client: &Client, path: &str, encoding: &str) -> ApiResponse {
    Client::send(
        client
            .request(Method::GET, path)
            .header(ACCEPT_ENCODING, encoding),
    )
    .await
}

/// The `Content-Encoding` of an answer, or the empty string.
fn encoding(response: &ApiResponse) -> &str {
    response
        .headers
        .get(CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
}

/// The body of an answer, decoded as its `Content-Encoding` says.
fn decoded(response: &ApiResponse) -> Vec<u8> {
    let body = response.body.as_slice();
    let mut out = Vec::new();
    match encoding(response) {
        "zstd" => out = zstd::decode_all(body).expect("zstd"),
        "br" => {
            brotli::Decompressor::new(body, 4096)
                .read_to_end(&mut out)
                .expect("brotli");
        }
        "gzip" => {
            flate2::read::GzDecoder::new(body)
                .read_to_end(&mut out)
                .expect("gzip");
        }
        "" => out = body.to_vec(),
        other => panic!("an unexpected encoding {other}"),
    }
    out
}

#[tokio::test]
async fn json_answers_are_compressed_as_the_client_asks() {
    let ts = Setup::native().start().await.init_basic().await;
    for _ in 0..3 {
        ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    }
    let path = format!("/teams/{TEST_TEAM_ID}/boards");
    let plain = ts.client.get(&path).await;
    plain.check_ok();
    assert_eq!(encoding(&plain), "", "nothing asked, nothing compressed");

    for (asked, expected) in [
        ("gzip, deflate, br, zstd", "zstd"),
        ("gzip, br;q=0.9", "gzip"),
        ("br", "br"),
        ("deflate", ""),
        ("identity", ""),
    ] {
        let response = get_encoded(&ts.client, &path, asked).await;
        response.check_ok();
        assert_eq!(encoding(&response), expected, "{asked}");
        assert_eq!(decoded(&response), plain.body, "{asked}: the same answer");
        let vary = response
            .headers
            .get(VARY)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_lowercase();
        assert!(vary.contains("accept-encoding"), "{asked}: Vary names it");
    }
    ts.teardown().await;
}

/// An archive is sent as it is, with its length, whatever the client accepts.
#[tokio::test]
async fn an_export_is_not_compressed() {
    let ts = Setup::native().start().await.init_basic().await;
    let board = ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let response = get_encoded(
        &ts.client,
        &format!("/boards/{}/archive/export", board.id),
        "zstd, br, gzip",
    )
    .await;
    response.check_ok();
    assert_eq!(encoding(&response), "", "not compressed");
    let length = response
        .headers
        .get(CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok());
    assert_eq!(length, Some(response.body.len()), "sent with its length");
    ts.teardown().await;
}

#[tokio::test]
async fn compression_can_be_turned_off() {
    let ts = Setup::native()
        .configure(|config| config.compress_responses = false)
        .start()
        .await
        .init_basic()
        .await;
    ts.create_board(TEST_TEAM_ID, BoardType::Open).await;
    let response = get_encoded(
        &ts.client,
        &format!("/teams/{TEST_TEAM_ID}/boards"),
        "gzip, zstd",
    )
    .await;
    response.check_ok();
    assert_eq!(encoding(&response), "", "not compressed");
    ts.teardown().await;
}
