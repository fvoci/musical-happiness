//! Positive parity for every workspace operation published in the HTTP contract.

#[path = "workspace_operations/actions.rs"]
mod actions;
#[path = "workspace_operations/files.rs"]
mod files;

use std::collections::{BTreeMap, BTreeSet};

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
    user: String,
    peer: String,
    seen: BTreeMap<&'static str, u16>,
    responses: BTreeMap<&'static str, Vec<Value>>,
    identifiers: Vec<String>,
    forbidden: Vec<String>,
}

impl Case {
    async fn founder(server: &TestServer, name: &str, forbidden: Vec<String>) -> Self {
        server
            .anonymous()
            .post(
                "/register",
                &json!({
                    "username": name, "email": format!("{name}@example.test"),
                    "password": PASSWORD, "organizationName": name,
                }),
            )
            .await
            .check_created();
        let client = server.login(name, PASSWORD).await;
        let user = client.me().await.id;
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
        Self {
            client,
            label: name.to_owned(),
            identifiers: vec![
                name.to_owned(),
                workspace.clone(),
                user.clone(),
                peer.id.clone(),
            ],
            workspace,
            user,
            peer: peer.id,
            seen: BTreeMap::new(),
            responses: BTreeMap::new(),
            forbidden,
        }
    }

    fn request(&self, operation: &str, parameters: Parameters<'_>) -> RequestBuilder {
        let (method, suffix, _) = WORKSPACE_ROUTES
            .iter()
            .find(|(_, _, name)| *name == operation)
            .unwrap_or_else(|| panic!("unknown operation {operation}"));
        let mut suffix = (*suffix).to_owned();
        for (key, value) in parameters {
            suffix = suffix.replace(&format!("{{{key}}}"), value);
        }
        assert!(
            !suffix.contains('{'),
            "unfilled parameter for {operation}: {suffix}"
        );
        self.client.request(
            method.parse().expect("contract HTTP method"),
            &format!("/w/{}{suffix}", self.workspace),
        )
    }

    async fn send(&mut self, operation: &'static str, request: RequestBuilder) -> ApiResponse {
        let response = Client::send(request).await;
        assert!(
            response.status.is_success(),
            "{operation} in {}: {} {}",
            self.workspace,
            response.status,
            response.text()
        );
        if let Some(previous) = self.seen.insert(operation, response.status.as_u16()) {
            assert_eq!(
                previous,
                response.status.as_u16(),
                "repeated {operation} has the same outcome"
            );
        }
        self.assert_is_ours(operation, &response.body);
        let structure = serde_json::from_slice(&response.body).map_or_else(
            |_| json!("binary"),
            |value| comparable_response(&value, "", &self.label),
        );
        self.responses.entry(operation).or_default().push(structure);
        response
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
        let contract: Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../contracts/openapi.json"
        )))
        .expect("contract JSON");
        let expected: BTreeSet<&str> = contract["paths"]
            .as_object()
            .expect("paths")
            .iter()
            .filter(|(path, _)| path.starts_with("/api/v1/w/{workspaceId}"))
            .flat_map(|(_, methods)| methods.as_object().expect("methods").values())
            .filter_map(|operation| operation["operationId"].as_str())
            .collect();
        assert_eq!(
            self.seen.keys().copied().collect::<BTreeSet<_>>(),
            expected,
            "every published workspace operation needs a successful positive case"
        );
    }
}

#[tokio::test]
async fn every_workspace_operation_succeeds_for_the_second_founder_as_for_the_first() {
    if db_type_of_the_tests() != DbType::Postgres {
        return;
    }
    let server = Setup::local_mode()
        .configure(open)
        .on_an_empty_database()
        .with_templates()
        .start()
        .await;
    let mut first = Case::founder(&server, "parity-first", Vec::new()).await;
    actions::exercise(&mut first).await;
    first.assert_complete();
    let mut second = Case::founder(&server, "parity-other", first.identifiers).await;
    actions::exercise(&mut second).await;
    second.assert_complete();
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
                || matches!(field, "token" | "fileId" | "uploadId" | "etag") =>
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
