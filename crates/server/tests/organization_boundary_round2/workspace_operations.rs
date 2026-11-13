//! Positive parity for every operation published in the HTTP contract.

#[path = "workspace_operations/actions.rs"]
mod actions;
#[path = "workspace_operations/files.rs"]
mod files;
#[path = "workspace_operations/identity.rs"]
mod identity;
#[path = "workspace_operations/service.rs"]
mod service;
#[path = "workspace_operations/shared.rs"]
mod shared;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use fvoci_model::{User, UserWorkspace, new_id};
use fvoci_store::DbType;
use reqwest::RequestBuilder;
use serde_json::{Value, json};

use super::common::{ApiResponse, Client, PASSWORD, Setup, TestServer};
use super::routes::WORKSPACE_ROUTES;
use super::{db_type_of_the_tests, open};

type Parameters<'a> = &'a [(&'a str, &'a str)];

struct Case {
    client: Client,
    label: String,
    workspace: String,
    org: String,
    instance_admin: bool,
    user: String,
    peer: String,
    seen: BTreeMap<&'static str, u16>,
    responses: BTreeMap<&'static str, Vec<Value>>,
    identifiers: Vec<String>,
    forbidden: Vec<String>,
}

impl Case {
    async fn founder(server: &TestServer, name: &str, forbidden: Vec<String>) -> Self {
        let registered = server
            .anonymous()
            .post(
                "/register",
                &json!({
                    "username": name, "email": format!("{name}@example.test"),
                    "password": PASSWORD, "organizationName": name,
                }),
            )
            .await;
        registered.check_created();
        let logged_in = server
            .anonymous()
            .post("/login", &json!({"username": name, "password": PASSWORD}))
            .await;
        assert_eq!(logged_in.status.as_u16(), 204, "login succeeds");
        let client = Client::new(
            &server.base_url,
            &logged_in.session_cookie().expect("login cookie"),
        );
        let account = client.me().await;
        let user = account.id;
        let listed: Vec<UserWorkspace> = client.get("/users/me/workspaces").await.ok_json();
        assert_eq!(listed.len(), 1, "a founder sees only their workspace");
        let workspace = listed[0].id.clone();
        let access = server
            .app
            .store()
            .resolve_workspace(&user, &workspace)
            .await
            .expect("workspace directory")
            .expect("founder admission");
        let store = server
            .app
            .store()
            .for_org(&access.org_id)
            .expect("organization scope");
        let mut peer = User {
            id: new_id(),
            username: format!("{name}-peer"),
            email: format!("{name}-peer@example.test"),
            ..User::default()
        };
        store
            .create_user(&mut peer)
            .await
            .expect("another member in this organization");
        let mut case = Self {
            client,
            label: name.to_owned(),
            identifiers: vec![
                name.to_owned(),
                workspace.clone(),
                user.clone(),
                peer.id.clone(),
                access.org_id.clone(),
            ],
            workspace,
            org: access.org_id,
            instance_admin: account.is_admin,
            user,
            peer: peer.id,
            seen: BTreeMap::new(),
            responses: BTreeMap::new(),
            forbidden,
        };
        case.record("register", &registered);
        case.record("login", &logged_in);
        case
    }

    fn request(&self, operation: &str, parameters: Parameters<'_>) -> RequestBuilder {
        let (method, mut path) = if let Some((method, suffix, _)) = WORKSPACE_ROUTES
            .iter()
            .find(|(_, _, name)| *name == operation)
        {
            (
                (*method).to_owned(),
                format!("/w/{}{suffix}", self.workspace),
            )
        } else {
            contract()["paths"]
                .as_object()
                .expect("paths")
                .iter()
                .find_map(|(path, methods)| {
                    methods
                        .as_object()
                        .expect("methods")
                        .iter()
                        .find_map(|(method, spec)| {
                            (spec["operationId"] == operation).then(|| {
                                (
                                    method.to_uppercase(),
                                    path.trim_start_matches("/api/v1").to_owned(),
                                )
                            })
                        })
                })
                .unwrap_or_else(|| panic!("unknown operation {operation}"))
        };
        for (key, value) in parameters {
            path = path.replace(&format!("{{{key}}}"), value);
        }
        path = path
            .replace("{orgId}", &self.org)
            .replace("{workspaceId}", &self.workspace);
        assert!(
            !path.contains('{'),
            "unfilled parameter for {operation}: {path}"
        );
        self.client
            .request(method.parse().expect("contract HTTP method"), &path)
    }

    async fn send(&mut self, operation: &'static str, request: RequestBuilder) -> ApiResponse {
        let response = Client::send(request).await;
        assert!(
            response.status.is_success()
                || (operation == "finishOidcLogin" && response.status.as_u16() == 303),
            "{operation} in {}: {} {}",
            self.workspace,
            response.status,
            response.text()
        );
        self.record(operation, &response);
        response
    }

    fn record(&mut self, operation: &'static str, response: &ApiResponse) {
        if let Some(previous) = self.seen.insert(operation, response.status.as_u16()) {
            assert_eq!(
                previous,
                response.status.as_u16(),
                "repeated {operation} has the same outcome"
            );
        }
        self.assert_is_ours(operation, &response.body);
        // Instance authority is intentionally different from organization ownership.
        if operation == "setUserAdmin" {
            return;
        }
        let body = serde_json::from_slice(&response.body).unwrap_or_else(|_| {
            if response.body.starts_with(b"PK") {
                json!("<archive>")
            } else {
                json!(response.text())
            }
        });
        self.responses
            .entry(operation)
            .or_default()
            .push(comparable_response(&body, "", &self.label));
    }

    fn assert_is_ours(&self, operation: &str, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        for value in &self.forbidden {
            assert!(
                !text.contains(value),
                "{operation} leaked first-organization data {value}"
            );
        }
    }

    async fn run(
        &mut self,
        operation: &'static str,
        parameters: Parameters<'_>,
        body: Value,
    ) -> ApiResponse {
        self.send(operation, self.request(operation, parameters).json(&body))
            .await
    }

    async fn present(
        &mut self,
        operation: &'static str,
        parameters: Parameters<'_>,
        expected: &str,
    ) -> ApiResponse {
        let response = self.run(operation, parameters, json!({})).await;
        assert!(
            response.text().contains(expected),
            "{operation} must return the seeded item {expected}: {}",
            response.text()
        );
        response
    }

    fn remember(&mut self, value: &Value, field: &str) -> String {
        let id = value[field]
            .as_str()
            .unwrap_or_else(|| panic!("{field} in {value}"))
            .to_owned();
        self.identifiers.push(id.clone());
        id
    }

    fn assert_complete(&self) {
        let expected: BTreeSet<&str> = contract()["paths"]
            .as_object()
            .expect("paths")
            .iter()
            .flat_map(|(_, methods)| methods.as_object().expect("methods").values())
            .filter_map(|operation| operation["operationId"].as_str())
            .collect();
        assert_eq!(
            self.seen.keys().copied().collect::<BTreeSet<_>>(),
            expected,
            "every published operation needs a case, including instance authority"
        );
    }
}

#[tokio::test]
async fn every_contract_operation_is_exercised_by_both_founders() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let fake = super::fake_oidc::FakeProvider::start().await;
    let smtp = super::fake_smtp::FakeSmtp::start_on(0).await;
    let server = Setup::local_mode()
        .trusting_oidc_provider(&fake.certificate_der)
        .smtp(smtp.settings())
        .configure(open)
        .configure(identity::configure)
        .on_an_empty_database()
        .with_templates()
        .start()
        .await;
    let mut first = Case::founder(&server, "parity-first", Vec::new()).await;
    actions::exercise(&mut first).await;
    service::exercise(&mut first, &server, &fake, &smtp).await;
    first.assert_complete();
    let mut second = Case::founder(&server, "parity-other", first.identifiers).await;
    actions::exercise(&mut second).await;
    service::exercise(&mut second, &server, &fake, &smtp).await;
    second.assert_complete();
    assert_eq!(
        first.seen.remove("setUserAdmin"),
        Some(200),
        "first founder has instance authority"
    );
    assert_eq!(
        second.seen.remove("setUserAdmin"),
        Some(403),
        "later founder has only organization authority"
    );
    assert_eq!(
        first.seen, second.seen,
        "all operations return the same successful status in each organization"
    );
    for (operation, responses) in first.responses {
        assert_eq!(
            responses, second.responses[operation],
            "{operation} returns the same fields and collection contents in both organizations"
        );
    }
    server.teardown().await;
    smtp.stop().await;
}

/// Keep response semantics while replacing generated identifiers, times and the
/// fixture's founder label. Instance administrator status deliberately differs.
fn comparable_response(value: &Value, field: &str, founder: &str) -> Value {
    if field == "isAdmin" {
        return json!("<instance-role>");
    }
    match value {
        Value::Number(_) if field.ends_with("At") => json!("<time>"),
        Value::String(text)
            if uuid::Uuid::try_parse(text).is_ok()
                || matches!(
                    field,
                    "token" | "fileId" | "uploadId" | "etag" | "authorizationUrl" | "redirectUri"
                ) =>
        {
            json!("<generated>")
        }
        Value::String(text) => json!(text.replace(founder, "<founder>")),
        Value::Array(values) => {
            let mut values: Vec<_> = values
                .iter()
                .map(|value| comparable_response(value, field, founder))
                .collect();
            values.sort_by_cached_key(Value::to_string);
            Value::Array(values)
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, value)| (key.clone(), comparable_response(value, key, founder)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn contract() -> &'static Value {
    static CONTRACT: LazyLock<Value> = LazyLock::new(|| {
        serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/openapi.json"
        )))
        .expect("contract JSON")
    });
    &CONTRACT
}

fn with_query(builder: RequestBuilder, key: &str, value: &str) -> RequestBuilder {
    let (client, request) = builder.build_split();
    let mut request = request.expect("a valid test request");
    request.url_mut().query_pairs_mut().append_pair(key, value);
    RequestBuilder::from_parts(client, request)
}
