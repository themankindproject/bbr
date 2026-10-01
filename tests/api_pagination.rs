//! Pagination must follow opaque next links, never synthesize page numbers.

use bbr::api::{BitbucketClient, Paginated};
use bbr::auth::Credentials;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(base: &str) -> BitbucketClient {
    BitbucketClient::new(
        base,
        Credentials {
            username: "test".into(),
            secret: "fake".into(),
        },
    )
    .unwrap()
}

#[tokio::test]
async fn small_limits_follow_short_server_pages() {
    let server = MockServer::start().await;
    Mock::given(path("/items"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [1, 2], "next": format!("{}/more?cursor=a", server.uri())
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/more"))
        .and(query_param("cursor", "a"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"values": [3, 4]})))
        .expect(1)
        .mount(&server)
        .await;
    let result: Vec<u64> = client(&server.uri())
        .fetch_paginated("/items", 3)
        .await
        .unwrap();
    assert_eq!(result, vec![1, 2, 3]);
}

#[tokio::test]
async fn non_first_page_preserves_next_query_without_duplicate_page_parameter() {
    let server = MockServer::start().await;
    Mock::given(path("/items"))
        .and(query_param("page", "4"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [7, 8], "size": 10, "page": 4, "pagelen": 2,
            "next": format!("{}/items?page=5&sort=-created_on&cursor=x%2By", server.uri())
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/items"))
        .and(query_param("page", "5"))
        .and(query_param("sort", "-created_on"))
        .and(query_param("cursor", "x+y"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"values": [9, 10]})))
        .expect(1)
        .mount(&server)
        .await;
    let result: Vec<u64> = client(&server.uri())
        .fetch_all_pages("/items?page=4", 4)
        .await
        .unwrap();
    assert_eq!(result, vec![7, 8, 9, 10]);
    for request in server.received_requests().await.unwrap() {
        assert_eq!(
            request
                .url
                .query_pairs()
                .filter(|(key, _)| key == "page")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn cursor_with_page_substring_is_not_numeric_paging() {
    let server = MockServer::start().await;
    Mock::given(path("/items"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [1], "size": 2, "next": format!("{}/next?opaque_page=token", server.uri())
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/next"))
        .and(query_param("opaque_page", "token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"values": [2]})))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        client(&server.uri())
            .fetch_all_pages::<u64>("/items", 2)
            .await
            .unwrap(),
        vec![1, 2]
    );
}

#[tokio::test]
async fn stale_size_and_page_length_do_not_override_next_links() {
    let server = MockServer::start().await;
    Mock::given(path("/items")).respond_with(ResponseTemplate::new(200).set_body_json(json!({
        "values": [1, 2], "size": 1, "pagelen": 100, "next": format!("{}/next?page=2", server.uri())
    }))).expect(1).mount(&server).await;
    Mock::given(path("/next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"values": [3]})))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(
        client(&server.uri())
            .fetch_all_pages::<u64>("/items", 10)
            .await
            .unwrap(),
        vec![1, 2, 3]
    );
}

#[tokio::test]
async fn empty_intermediate_page_still_follows_its_next_link() {
    let server = MockServer::start().await;
    for (from, values, next) in [
        ("/items", vec![1], Some(format!("{}/empty", server.uri()))),
        ("/empty", vec![], Some(format!("{}/last", server.uri()))),
        ("/last", vec![2], None),
    ] {
        Mock::given(path(from))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"values":values,"next":next})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    assert_eq!(
        client(&server.uri())
            .fetch_all_pages::<u64>("/items", 10)
            .await
            .unwrap(),
        vec![1, 2]
    );
}

#[tokio::test]
async fn repeated_next_link_is_an_error_not_duplicate_results() {
    let server = MockServer::start().await;
    Mock::given(path("/items"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [1], "next": format!("{}/items", server.uri())
        })))
        .expect(1)
        .mount(&server)
        .await;
    let result = client(&server.uri())
        .fetch_all_pages::<u64>("items", 5)
        .await;
    assert!(result.unwrap_err().to_string().contains("cycle"));
}

#[tokio::test]
async fn longer_cycles_fail_instead_of_returning_partial_success() {
    let server = MockServer::start().await;
    for (from, to) in [("/items", "/next"), ("/next", "/items")] {
        Mock::given(path(from))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "values": [1], "next": format!("{}{to}", server.uri())
            })))
            .expect(1)
            .mount(&server)
            .await;
    }
    assert!(client(&server.uri())
        .fetch_all_pages::<u64>("/items", 5)
        .await
        .unwrap_err()
        .to_string()
        .contains("cycle"));
}

#[tokio::test]
async fn limit_reached_does_not_fetch_or_validate_an_unused_next_link() {
    let server = MockServer::start().await;
    let c = client(&server.uri());
    let first = Paginated {
        values: vec![1, 2],
        next: Some("invalid unused next".into()),
        ..Default::default()
    };
    assert_eq!(c.paginate_from(first, "/items", 1).await.unwrap(), vec![1]);
    assert!(c
        .fetch_all_pages::<u64>("/items", 0)
        .await
        .unwrap()
        .is_empty());
    assert!(c
        .fetch_paginated::<u64>("/items", 0)
        .await
        .unwrap()
        .is_empty());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn next_link_must_stay_under_exact_api_base() {
    let server = MockServer::start().await;
    let c = client(&format!("{}/2.0", server.uri()));
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"values":[99]})))
        .mount(&server)
        .await;
    for next in [
        format!("{}/2.0evil/items", server.uri()),
        format!("{}/2.0/../private", server.uri()),
        format!("{}/2.0/%2e%2e/private", server.uri()),
        format!("{}/2.0/items#fragment", server.uri()),
        format!("{}/2.0/items\n", server.uri()),
        format!("http://user:secret@{}/2.0/items", server.address()),
    ] {
        let first = Paginated {
            values: vec![1u64],
            next: Some(next),
            ..Default::default()
        };
        assert!(c.paginate_from(first, "/items", 2).await.is_err());
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn cross_origin_next_link_is_rejected_before_sending_credentials() {
    let server = MockServer::start().await;
    let other = MockServer::start().await;
    let c = client(&server.uri());
    let first = Paginated {
        values: vec![1u64],
        next: Some(format!("{}/items?page=2", other.uri())),
        size: 2,
        ..Default::default()
    };
    assert!(c.paginate_from(first, "/items", 2).await.is_err());
    assert!(server.received_requests().await.unwrap().is_empty());
    assert!(other.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn endless_unique_empty_pages_hit_the_page_budget() {
    let server = MockServer::start().await;
    let base = server.uri();
    Mock::given(path("/items"))
        .respond_with(move |request: &wiremock::Request| {
            let page = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "page")
                .map(|(_, value)| value.parse::<usize>().unwrap())
                .unwrap_or(1);
            ResponseTemplate::new(200).set_body_json(json!({
                "values": [], "next": format!("{base}/items?page={}", page + 1)
            }))
        })
        .expect(10000)
        .mount(&server)
        .await;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        client(&server.uri()).fetch_all_pages::<u64>("/items", usize::MAX),
    )
    .await
    .expect("page-budget test must terminate");
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("10000-page safety limit"));
}

#[tokio::test]
async fn terminal_link_stops_despite_overstated_size() {
    let server = MockServer::start().await;
    Mock::given(path("/next"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "values": [2], "size": 9000, "page": 2
        })))
        .expect(1)
        .mount(&server)
        .await;
    let first = Paginated {
        values: vec![1u64],
        size: 9000,
        next: Some(format!("{}/next?page=2", server.uri())),
        ..Default::default()
    };
    assert_eq!(
        client(&server.uri())
            .paginate_from(first, "/items", 10)
            .await
            .unwrap(),
        vec![1, 2]
    );
}

#[tokio::test]
async fn continuation_errors_are_not_silently_returned_as_partial_lists() {
    let server = MockServer::start().await;
    Mock::given(path("/next"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    let first = Paginated {
        values: vec![1u64],
        next: Some(format!("{}/next", server.uri())),
        ..Default::default()
    };
    let error = client(&server.uri())
        .paginate_from(first, "/items", 5)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), "auth");
}
