//! Keeping a card's document small: its log folded into its state, and a document that
//! grew near its limit made again from its body. What it holds is the same afterwards.

use std::time::Instant;

use fvoci_collab::testing::Editor;
use fvoci_server::collab::DocChange;
use serde_json::{Value, json};

use crate::collab::{body_of, card, editing};
use crate::common::Setup;

fn paragraphs(texts: &[&str]) -> Value {
    let content: Vec<Value> = texts
        .iter()
        .map(|text| json!({"type": "paragraph", "content": [{"type": "text", "text": text}]}))
        .collect();
    json!({"type": "doc", "content": content})
}

/// A document that takes a thousand updates folds its log into its state every two
/// hundred; opened again from what is stored, it is what it was. Prints how long the
/// opening and the updates took.
#[tokio::test]
async fn a_thousand_updates_are_folded_into_the_state_and_the_document_stays_the_same() {
    let th = Setup::native().start().await.init_basic().await;
    let e = editing(&th, &paragraphs(&["first"])).await;
    let collab = e.app.collab();
    let began = Instant::now();
    let state = collab.open(&e.app, &e.card_id).await.expect("opened");
    let opened = began.elapsed();
    let editor = Editor::join(101, &state);
    let began = Instant::now();
    for n in 0..1000 {
        let update = if n % 3 == 2 {
            editor.delete_last()
        } else {
            editor.append_paragraph(&format!("line {n}"))
        };
        collab
            .take(&e.app, &e.card_id, &e.user1, &update)
            .await
            .expect("taken");
    }
    let taken = began.elapsed();
    eprintln!(
        "measured: open {} ms, 1000 updates {} ms ({} µs each, logged and folded)",
        opened.as_millis(),
        taken.as_millis(),
        taken.as_micros() / 1000
    );
    let wanted = editor.body().map(|body| body.document().clone());
    let stored = e
        .app
        .store()
        .get_body_doc(&e.card_id)
        .await
        .expect("read")
        .expect("a document");
    assert_eq!(
        (stored.state_seq, stored.log_end, stored.updates.len()),
        (1000, 1000, 0),
        "folded at the thousandth"
    );

    collab.abandon().await;
    let again = Editor::join(102, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    assert_eq!(again.body().map(|body| body.document().clone()), wanted);
    collab.close(&e.app, &e.card_id).await.expect("closed");
    assert_eq!(body_of(&card(&th.client, &e.card_id).await), wanted);
    th.teardown().await;
}

/// A document whose state nears its limit (what was deleted is kept in it) is written
/// to its card and made again from its body: the update that brought it there is taken,
/// the caller and the room are told, the body is the same, the state is small again, and
/// an update made against the document before is out of step.
#[tokio::test]
async fn a_document_near_its_limit_is_made_again_from_its_body() {
    let th = Setup::native()
        .configure(|config| config.collab.collab_state_bytes = 2 * 1024 * 1024)
        .start()
        .await
        .init_basic()
        .await;
    let e = editing(&th, &paragraphs(&["kept"])).await;
    let collab = e.app.collab();
    let mut events = collab.subscribe();
    let editor = Editor::join(101, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    let line = "z".repeat(60 * 1024);
    let mut remade = None;
    for round in 0..60 {
        let added = collab
            .take(
                &e.app,
                &e.card_id,
                &e.user1,
                &editor.append_paragraph(&line),
            )
            .await
            .expect("taken");
        if added.remade {
            remade = Some(round);
            break;
        }
        let deleted = collab
            .take(&e.app, &e.card_id, &e.user1, &editor.delete_last())
            .await
            .expect("taken");
        if deleted.remade {
            remade = Some(round);
            break;
        }
    }
    let round = remade.expect("made again before the state reached its limit");
    assert!(round > 10, "only once it was near the limit: round {round}");
    let event = events.try_recv().expect("an event");
    assert_eq!(
        (event.card_id, event.change),
        (e.card_id.clone(), DocChange::Remade)
    );
    let wanted = editor.body().map(|body| body.document().clone());
    let stored = e
        .app
        .store()
        .get_body_doc(&e.card_id)
        .await
        .expect("read")
        .expect("a document");
    assert!(
        stored.state.len() < 64 * 1024,
        "small again: {} bytes",
        stored.state.len()
    );
    assert_eq!(
        (stored.state_seq, stored.projected_seq, stored.updates.len()),
        (stored.log_end, stored.log_end, 0)
    );
    assert_eq!(
        body_of(&card(&th.client, &e.card_id).await),
        wanted,
        "written"
    );
    let again = Editor::join(102, &collab.open(&e.app, &e.card_id).await.expect("opened"));
    assert_eq!(again.body().map(|body| body.document().clone()), wanted);
    let refused = collab
        .take(
            &e.app,
            &e.card_id,
            &e.user1,
            &editor.append_paragraph("late"),
        )
        .await
        .expect_err("made against the document before");
    assert_eq!(
        refused.reason().map(fvoci_server::error::Reason::as_str),
        Some("update_out_of_step")
    );
    collab
        .take(&e.app, &e.card_id, &e.user1, &again.append_paragraph("on"))
        .await
        .expect("an editor that loaded it again goes on");
    th.teardown().await;
}
