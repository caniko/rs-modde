use super::*;

#[tokio::test]
async fn direct_resolver_follows_workupload_style_download_link() {
    let bytes = b"direct archive";
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/file/abc"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"<html><a href="/download/abc">Download</a></html>"#),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download/abc"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .set_body_bytes(bytes),
        )
        .mount(&server)
        .await;

    let temp = TempDir::new().unwrap();
    let archive = MissingArchive {
        hash: xxh64(bytes, 0),
        name: "manual.7z".into(),
        size: bytes.len() as u64,
        source_kind: MissingArchiveSourceKind::Manual,
        url: Some(format!("{}/file/abc", server.uri())),
        source_hint: "test".into(),
    };
    let client = Client::builder().cookie_store(true).build().unwrap();
    let final_path = temp.path().join("manual.7z");

    let resolved = resolve_manual_http_flow(
        &client,
        Url::parse(archive.url.as_ref().unwrap()).unwrap(),
        &archive,
        &final_path,
    )
    .await
    .unwrap();

    assert_eq!(resolved, Some((final_path.clone(), archive.hash)));
    assert_eq!(tokio::fs::read(final_path).await.unwrap(), bytes);
}

#[tokio::test]
async fn direct_resolver_handles_sharemods_style_two_step_form() {
    let bytes = b"sharemods archive";
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/file.html"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"
                <form method="post" action="/create">
                  <input name="op" value="download2">
                  <button>Create download link</button>
                </form>
                "#,
        ))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/create"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html")
                .set_body_string(r#"<html><a href="/start">Start Download</a></html>"#),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/start"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .set_body_bytes(bytes),
        )
        .mount(&server)
        .await;

    let temp = TempDir::new().unwrap();
    let archive = MissingArchive {
        hash: xxh64(bytes, 0),
        name: "EyesMod3_FullCompressed1K.7z".into(),
        size: bytes.len() as u64,
        source_kind: MissingArchiveSourceKind::Manual,
        url: Some(format!("{}/file.html", server.uri())),
        source_hint: "test".into(),
    };
    let client = Client::builder().cookie_store(true).build().unwrap();
    let final_path = temp.path().join("EyesMod3_FullCompressed1K.7z");

    let resolved = resolve_manual_http_flow(
        &client,
        Url::parse(archive.url.as_ref().unwrap()).unwrap(),
        &archive,
        &final_path,
    )
    .await
    .unwrap();

    assert_eq!(resolved, Some((final_path.clone(), archive.hash)));
    assert_eq!(tokio::fs::read(final_path).await.unwrap(), bytes);
}

#[tokio::test]
async fn direct_resolver_rejects_wrong_hash_download() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/file"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"<html><a href="/download">Download</a></html>"#),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .set_body_bytes(b"wrong archive"),
        )
        .mount(&server)
        .await;

    let temp = TempDir::new().unwrap();
    let archive = MissingArchive {
        hash: xxh64(b"correct archive", 0),
        name: "manual.7z".into(),
        size: 15,
        source_kind: MissingArchiveSourceKind::Manual,
        url: Some(format!("{}/file", server.uri())),
        source_hint: "test".into(),
    };
    let client = Client::builder().cookie_store(true).build().unwrap();
    let final_path = temp.path().join("manual.7z");

    let resolved = resolve_manual_http_flow(
        &client,
        Url::parse(archive.url.as_ref().unwrap()).unwrap(),
        &archive,
        &final_path,
    )
    .await
    .unwrap();

    assert!(resolved.is_none());
    assert!(!final_path.exists());
}

#[tokio::test]
async fn loverslab_dns_failure_is_actionable() {
    let archive = MissingArchive {
        hash: xxh64(b"archive", 0),
        name: "00 - Normal Version.7z".into(),
        size: 7,
        source_kind: MissingArchiveSourceKind::Manual,
        url: Some("https://www.loverslab.com/files/file/5051".into()),
        source_hint: "test".into(),
    };

    let result = resolve_loverslab_manual("definitely-not-a-host.invalid", &archive)
        .await
        .unwrap();

    let DirectAcquireOutcome::Final(result) = result else {
        panic!("expected DNS-unresolved final result");
    };
    assert_eq!(result.status, AcquireStatus::DnsUnresolved);
    assert!(
        result
            .message
            .as_deref()
            .unwrap()
            .contains("did not resolve")
    );
}
