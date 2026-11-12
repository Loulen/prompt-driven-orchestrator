//! The credentials a network git operation runs with, against a remote that
//! asks for them (`git_http.rs`): what an extension provides reaches git's
//! environment, a credential helper answers with it, and no process ever
//! carries the secret on its command line (ADR-0098).

use std::time::Duration;

use pdo_workspace::adapters::git::clone::{clone_full, clone_shallow, ls_remote, CloneError};
use pdo_workspace::adapters::git::fetch::fetch_all_remotes;
use pdo_workspace::domain::branches::FetchErrorKind;
use pdo_workspace::domain::clone::{is_credential_refusal, CloneFailure};

use crate::git_http::{bare_env, helper_log, provided_env, write_helper, AuthGitServer};

#[tokio::test]
async fn a_full_clone_authenticates_with_the_provided_helper_off_every_command_line() {
    let server = AuthGitServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let (helper, log) = write_helper(tmp.path());
    let env = provided_env(&helper, &log, &server.secret);
    let (_tx, rx) = tokio::sync::oneshot::channel();
    clone_full(
        &server.url,
        &tmp.path().join("app"),
        &env,
        &|_: &str| {},
        rx,
    )
    .await
    .unwrap();
    assert!(tmp.path().join("app/README.md").is_file());
    let said = helper_log(&log);
    assert!(said.contains("called"), "the helper answered: {said:?}");
    assert!(!said.contains("leaked"), "{said}");
}

#[tokio::test]
async fn without_provided_credentials_the_clone_is_refused_its_credentials() {
    let server = AuthGitServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let (_tx, rx) = tokio::sync::oneshot::channel();
    let err = clone_full(
        &server.url,
        &tmp.path().join("app"),
        &bare_env(),
        &|_: &str| {},
        rx,
    )
    .await
    .unwrap_err();
    let CloneFailure::Failed(stderr) = err else {
        panic!("{err:?}");
    };
    assert!(is_credential_refusal(&stderr), "{stderr}");
    assert!(!tmp.path().join("app").exists());
}

#[tokio::test]
async fn a_shallow_clone_and_the_check_of_a_remote_take_the_provided_helper() {
    let server = AuthGitServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let (helper, log) = write_helper(tmp.path());
    let env = provided_env(&helper, &log, &server.secret);
    let (_tx, rx) = tokio::sync::oneshot::channel();
    clone_shallow(
        &server.url,
        None,
        &tmp.path().join("scan"),
        &env,
        rx,
        Duration::from_secs(60),
    )
    .await
    .unwrap();
    assert!(tmp.path().join("scan/README.md").is_file());
    ls_remote(&server.url, &env).await.unwrap();
    let said = helper_log(&log);
    assert_eq!(said.matches("called").count(), 2, "{said}");
    assert!(!said.contains("leaked"), "{said}");

    let (_tx, rx) = tokio::sync::oneshot::channel();
    let err = clone_shallow(
        &server.url,
        None,
        &tmp.path().join("refused"),
        &bare_env(),
        rx,
        Duration::from_secs(60),
    )
    .await
    .unwrap_err();
    assert!(
        matches!(&err, CloneError::Failed(stderr) if is_credential_refusal(stderr)),
        "{err:?}"
    );
    let CloneFailure::Failed(stderr) = ls_remote(&server.url, &bare_env()).await.unwrap_err()
    else {
        panic!("a refusal, not a cancel");
    };
    assert!(is_credential_refusal(&stderr), "{stderr}");
}

#[tokio::test]
async fn a_fetch_authenticates_with_the_provided_helper_and_names_a_refusal() {
    let server = AuthGitServer::start().await;
    let tmp = tempfile::tempdir().unwrap();
    let (helper, log) = write_helper(tmp.path());
    let env = provided_env(&helper, &log, &server.secret);
    let (_tx, rx) = tokio::sync::oneshot::channel();
    let work = tmp.path().join("work");
    clone_full(&server.url, &work, &env, &|_: &str| {}, rx)
        .await
        .unwrap();

    assert_eq!(fetch_all_remotes(&work, &env).await, None);
    let said = helper_log(&log);
    assert_eq!(said.matches("called").count(), 2, "{said}");
    assert!(!said.contains("leaked"), "{said}");

    let refused = fetch_all_remotes(&work, &bare_env())
        .await
        .expect("no credentials, no fetch");
    assert_eq!(refused.kind, FetchErrorKind::Auth, "{refused:?}");
}
