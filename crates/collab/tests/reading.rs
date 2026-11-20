//! The reading of a state as it is counted before the library reads it: the lists it
//! leaves are the library's, whatever the changes name, and a document whose reading
//! takes more steps than a document may is not made by updates, nor read from what
//! is stored.

mod support;

use std::collections::HashMap;

use fvoci_collab::testing::{placing_steps, reading};
use fvoci_collab::{
    PLACING_STEPS, READING_STEPS, STORED_READING_STEPS, TakeError, UpdateError, UpdateLimits,
    from_body, from_state, new_doc, reading_steps, state_of, take_update,
};
use fvoci_model::BODY_MAX_NODES;
use serde_json::json;
use support::honest::SERVED;
use support::random::Random;
use support::wire::{Change, Content, Id, Parent, Run, Update};
use support::{body_of, edited};
use yrs::branch::{Branch, BranchID, BranchPtr};
use yrs::updates::decoder::Decode;
use yrs::{
    Any, Array, ArrayRef, Doc, Map, MapRef, Out, ReadTxn, Transact, XmlElementPrelim, XmlFragment,
    XmlTextPrelim,
};

/// What a place of a list holds, as both readings can name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Label {
    /// A shared type, by the change that made it.
    Type(Id),
    Letter(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Element,
    Text,
    /// Letters, from this one on.
    Letters(u32),
    Deleted,
    Collected,
}

/// A change of a state that is made up.
#[derive(Debug, Clone)]
struct Planned {
    id: Id,
    length: u64,
    kind: Kind,
    /// The parent and the member it is a value of.
    member: Option<(Id, &'static str)>,
}

impl Planned {
    fn is_type(&self) -> bool {
        matches!(self.kind, Kind::Element | Kind::Text)
    }

    fn content(&self) -> Content {
        match self.kind {
            Kind::Element => Content::element("p"),
            Kind::Text => Content::xml_text(),
            Kind::Letters(first) => Content::text(&letters(first, self.length)),
            Kind::Deleted | Kind::Collected => Content::Deleted(self.length),
        }
    }
}

fn letters(first: u32, length: u64) -> String {
    (0..u32::try_from(length).expect("a few letters"))
        .map(|at| char::from_u32(first + at).expect("a letter of the basic plane"))
        .collect()
}

const CLIENTS: [u64; 7] = [1, 2, 7, 300, 1 << 20, (1 << 40) + 5, (1 << 52) + 1];
const MEMBERS: [&str; 2] = ["a", "b"];

/// A state of a few clients whose changes name each other as no editor's do: mostly
/// changes planned before them, now and then any.
fn state(random: &mut Random) -> (Vec<Planned>, Vec<u8>) {
    let clients: Vec<u64> = {
        let mut pool = CLIENTS.to_vec();
        (0..2 + random.below(4))
            .map(|_| pool.swap_remove(random.below(pool.len())))
            .collect()
    };
    let mut clocks: HashMap<u64, u64> = HashMap::new();
    let mut letter = 0x4e00_u32;
    let mut planned: Vec<Planned> = Vec::new();
    for _ in 0..10 + random.below(80) {
        let client = *random.pick(&clients);
        let (kind, length) = match random.below(20) {
            0..=5 => (Kind::Element, 1),
            6..=8 => (Kind::Text, 1),
            9..=16 => {
                let length = 1 + random.below(4);
                let first = letter;
                letter += u32::try_from(length).expect("a few");
                (Kind::Letters(first), length)
            }
            17 | 18 => (Kind::Deleted, 1 + random.below(3)),
            _ => (Kind::Collected, 1 + random.below(2)),
        };
        let length = u64::try_from(length).expect("a few");
        let clock = clocks.entry(client).or_insert(0);
        planned.push(Planned {
            id: (client, *clock),
            length,
            kind,
            member: None,
        });
        *clock += length;
    }
    let types: Vec<Id> = planned
        .iter()
        .filter(|change| change.is_type())
        .map(|change| change.id)
        .collect();
    for change in &mut planned {
        if matches!(change.kind, Kind::Letters(_)) && !types.is_empty() && random.chance(5) {
            change.member = Some((*random.pick(&types), *random.pick(&MEMBERS)));
        }
    }
    let mut runs: HashMap<u64, Vec<Change>> = HashMap::new();
    let mut first_writes: HashMap<(Id, &str), u32> = HashMap::new();
    for (at, change) in planned.iter().enumerate() {
        let made = written(random, &planned, at, &mut first_writes);
        runs.entry(change.id.0).or_default().push(made);
    }
    let update = Update {
        runs: clients
            .iter()
            .filter_map(|client| {
                Some(Run {
                    client: *client,
                    clock: 0,
                    changes: runs.remove(client)?,
                })
            })
            .collect(),
        deleted: Vec::new(),
    };
    (planned, update.bytes())
}

/// A place of one of `among`, chosen from those planned before `at` mostly.
fn place(random: &mut Random, planned: &[Planned], among: &[usize], at: usize) -> Option<Id> {
    let before: Vec<usize> = among.iter().copied().filter(|other| *other < at).collect();
    let pool = if before.is_empty() || random.chance(60) {
        among
    } else {
        &before
    };
    if pool.is_empty() {
        return None;
    }
    let named = &planned[*random.pick(pool)];
    let offset =
        u64::try_from(random.below(usize::try_from(named.length).expect("a few"))).expect("a few");
    Some((named.id.0, named.id.1 + offset))
}

/// The change `at` as it is written: with the neighbours or the parent it names.
fn written(
    random: &mut Random,
    planned: &[Planned],
    at: usize,
    first_writes: &mut HashMap<(Id, &'static str), u32>,
) -> Change {
    let change = &planned[at];
    if change.kind == Kind::Collected {
        return Change::Collected(change.length);
    }
    if let Some((parent, key)) = change.member {
        // A value names an earlier value of its member, or the member.
        let values: Vec<usize> = (0..at)
            .filter(|other| planned[*other].member == change.member)
            .collect();
        let writes = first_writes.entry((parent, key)).or_insert(0);
        if values.is_empty() || (*writes < 8 && random.chance(3)) {
            *writes += 1;
            return Change::member(Parent::Change(parent), key, change.content());
        }
        let before = &planned[*random.pick(&values)];
        return Change::after(
            (before.id.0, before.id.1 + before.length - 1),
            change.content(),
        );
    }
    let others: Vec<usize> = (0..planned.len())
        .filter(|other| *other != at && planned[*other].member.is_none())
        .collect();
    let types: Vec<usize> = others
        .iter()
        .copied()
        .filter(|other| planned[*other].is_type())
        .collect();
    let left = (!random.chance(5))
        .then(|| place(random, planned, &others, at))
        .flatten();
    let right = random
        .chance(3)
        .then(|| place(random, planned, &others, at))
        .flatten();
    if random.chance(4) || (left.is_none() && right.is_none()) {
        let parent = match place(random, planned, &types, at) {
            Some(parent) if random.chance(2) => Parent::Change(parent),
            _ => Parent::body(),
        };
        return Change::under(parent, change.content());
    }
    Change::Item {
        left,
        right,
        parent: Parent::body(),
        key: None,
        content: change.content(),
    }
}

/// The lists of a document as the library holds them, for every parent that can be
/// reached from the root: its children, and the value of each member.
#[derive(Debug, Default, PartialEq, Eq)]
struct Lists {
    children: HashMap<Option<Id>, Vec<Label>>,
    values: HashMap<(Id, &'static str), Label>,
}

fn id_of(branch: &Branch) -> Id {
    match branch.id() {
        BranchID::Nested(id) => (id.client.get(), u64::from(id.clock)),
        BranchID::Root(name) => panic!("the root {name} is no child"),
    }
}

fn label_of(out: &Out) -> (Label, Option<BranchPtr>) {
    match out {
        Out::Any(Any::String(letter)) => {
            // A child is one letter; the value of a member is the letters of the
            // change that is its last, of which the last one is told.
            let letter = letter.chars().last().expect("a letter");
            (Label::Letter(letter), None)
        }
        Out::YXmlElement(element) => {
            let branch: &Branch = element.as_ref();
            (Label::Type(id_of(branch)), Some(BranchPtr::from(branch)))
        }
        Out::YXmlText(text) => {
            let branch: &Branch = text.as_ref();
            (Label::Type(id_of(branch)), Some(BranchPtr::from(branch)))
        }
        other => panic!("no state here holds {other:?}"),
    }
}

fn of_library(doc: &Doc) -> Lists {
    let root = doc.get_or_insert_xml_fragment("default");
    let txn = doc.transact();
    let mut lists = Lists::default();
    let root: &Branch = root.as_ref();
    let mut open: Vec<(Option<Id>, BranchPtr)> = vec![(None, BranchPtr::from(root))];
    while let Some((parent, branch)) = open.pop() {
        let mut children = Vec::new();
        for out in ArrayRef::from(branch).iter(&txn) {
            let (label, nested) = label_of(&out);
            children.push(label);
            if let (Label::Type(id), Some(nested)) = (label, nested) {
                open.push((Some(id), nested));
            }
        }
        lists.children.insert(parent, children);
        let Some(parent) = parent else { continue };
        for key in MEMBERS {
            if let Some(value) = MapRef::from(branch).get(&txn, key) {
                lists.values.insert((parent, key), label_of(&value).0);
            }
        }
    }
    lists
}

/// The lists the reading here leaves, for the parents the library reaches.
fn of_reading(planned: &[Planned], state: &[u8], reached: &Lists) -> Lists {
    let (_, read) = reading(state).expect("a whole state");
    let labels = |pieces: &[(u64, u64, u64)]| -> Vec<Label> {
        let mut labels = Vec::new();
        for (client, begin, end) in pieces {
            let change = planned
                .iter()
                .find(|change| {
                    change.id.0 == *client
                        && (change.id.1..change.id.1 + change.length).contains(begin)
                })
                .expect("a change of the state");
            match change.kind {
                Kind::Element | Kind::Text => labels.push(Label::Type(change.id)),
                Kind::Letters(first) => {
                    let whole: Vec<char> = letters(first, change.length).chars().collect();
                    let from = usize::try_from(begin - change.id.1).expect("a few");
                    let to = usize::try_from(end - change.id.1).expect("a few");
                    labels.extend(whole[from..to].iter().copied().map(Label::Letter));
                }
                Kind::Deleted => {}
                Kind::Collected => panic!("what was collected is in no list"),
            }
        }
        labels
    };
    let mut lists = Lists::default();
    for parent in reached.children.keys() {
        lists.children.insert(*parent, Vec::new());
    }
    for list in &read {
        if !reached.children.contains_key(&list.parent) {
            continue;
        }
        match (&list.member, list.parent) {
            (None, parent) => {
                lists.children.insert(parent, labels(&list.pieces));
            }
            (Some(member), Some(parent)) => {
                let key = MEMBERS
                    .into_iter()
                    .find(|key| key.as_bytes() == member.as_slice())
                    .expect("a member of the state");
                let last = labels(&list.pieces).last().copied().expect("a value");
                lists.values.insert((parent, key), last);
            }
            (Some(_), None) => panic!("the root has no members"),
        }
    }
    lists
}

#[test]
fn the_lists_of_a_state_are_read_here_as_the_library_reads_them() {
    let (mut whole, mut waiting) = (0, 0);
    for seed in 0..40000 {
        let mut random = Random::new(seed);
        let (planned, state) = state(&mut random);
        let doc = new_doc();
        let update = yrs::Update::decode_v1(&state).expect("an update");
        let missing = {
            let mut txn = doc.transact_mut();
            txn.apply_update(update).expect("applied");
            txn.has_missing_updates()
        };
        if missing {
            waiting += 1;
            assert_eq!(
                reading(&state).err(),
                Some("it leaves a gap in a client's changes"),
                "seed {seed}: the library left changes waiting"
            );
            continue;
        }
        whole += 1;
        let library = of_library(&doc);
        let read = of_reading(&planned, &state, &library);
        for (parent, children) in &library.children {
            assert_eq!(
                read.children.get(parent),
                Some(children),
                "seed {seed}: the children of {parent:?}, of {planned:?}"
            );
        }
        assert_eq!(
            of_reading(&planned, &state, &library),
            library,
            "seed {seed}"
        );
    }
    assert!(
        whole > 1000 && waiting > 100,
        "{whole} whole, {waiting} waiting"
    );
}

/// The limits of the server but for the steps a reading may take: what it took before
/// they were counted.
const UNCOUNTED: UpdateLimits = UpdateLimits {
    reading_steps: None,
    ..SERVED
};

/// The limits a worker reads what was stored with.
const STORED: UpdateLimits = UpdateLimits {
    placing_steps: None,
    clients: None,
    client_changes: None,
    reading_steps: Some(STORED_READING_STEPS),
    ..SERVED
};

/// The client that typed the list.
const TYPIST: u64 = 7;

/// A document of `children` empty paragraphs typed one after the other by one client.
fn typed_in_order(children: u64) -> Doc {
    let changes = (0..children)
        .map(|clock| match clock.checked_sub(1) {
            None => Change::under(Parent::body(), Content::element("paragraph")),
            Some(before) => Change::after((TYPIST, before), Content::element("paragraph")),
        })
        .collect();
    let roomy = UpdateLimits {
        update_bytes: usize::MAX,
        placing_steps: None,
        ..SERVED
    };
    take_update(&new_doc(), &Update::of(TYPIST, 0, changes).bytes(), &roomy)
        .expect("typed")
        .doc
}

/// An update of `changes` changes of `client` from `clock` on, each deleted content of
/// one place that names the last of `children` typed children to its right and nothing
/// to its left: the library walks the whole list to place each, a client above the
/// typist's, every time it reads the document.
fn before_the_last(client: u64, clock: u64, changes: u64, children: u64) -> Vec<u8> {
    let each = || Change::Item {
        left: None,
        right: Some((TYPIST, children - 1)),
        parent: Parent::body(),
        key: None,
        content: Content::Deleted(1),
    };
    Update::of(client, clock, (0..changes).map(|_| each()).collect()).bytes()
}

/// The most changes of [`before_the_last`] from `clock` on an update may hold for
/// `doc` ([`PLACING_STEPS`]).
fn most_placed(doc: &Doc, clock: u64, children: u64) -> u64 {
    let within = |changes: u64| {
        let update = before_the_last(ABOVE, clock, changes, children);
        placing_steps(doc, &update).expect("counted") <= PLACING_STEPS
    };
    let (mut most, mut past) = (1_u64, 2 * PLACING_STEPS / children + 2);
    assert!(within(most) && !within(past), "{most} to {past} changes");
    while past - most > 1 {
        let changes = most.midpoint(past);
        if within(changes) {
            most = changes;
        } else {
            past = changes;
        }
    }
    most
}

const ABOVE: u64 = 1 << 40;

/// As many paragraphs as a body may have, typed in order.
const CHILDREN: u64 = 49_998;

fn steps_of(doc: &Doc) -> u64 {
    reading(&state_of(doc)).expect("a whole state").0
}

/// Twelve updates, each of as many changes as an update may hold that walk a list of
/// 49,998 paragraphs every time the document is read, were taken one after the other:
/// each added three million steps (0.8 s) to every update after it and to every
/// opening, and after the twelfth no worker opened the document in its time. Each
/// leaves a document that takes more steps to read than a document may, and is
/// refused; fewer changes are taken until the document is there, and then no more: the
/// document is read in the steps it may take whatever was sent, and what is stored of
/// it is opened.
#[test]
fn updates_at_the_limit_one_after_the_other_leave_a_document_read_in_its_steps() {
    let typed = typed_in_order(CHILDREN);
    let most = most_placed(&typed, 0, CHILDREN);
    assert!(most >= 60, "{most} changes in an update");
    let mut doc = typed;
    let mut clock = 0;
    for round in 0..12 {
        let update = before_the_last(ABOVE, clock, most, CHILDREN);
        assert!(update.len() < 512, "{} bytes", update.len());
        match take_update(&doc, &update, &SERVED) {
            Ok(taken) => {
                doc = taken.doc;
                clock += most;
            }
            Err(TakeError::Update(UpdateError::TooManyReadingSteps { steps, limit })) => {
                assert_eq!(limit, READING_STEPS);
                // Counted no further than the change that went past the limit.
                assert!(steps <= limit + CHILDREN + most, "{steps} steps counted");
            }
            Err(other) => panic!("round {round}: {other}"),
        }
        let steps = steps_of(&doc);
        assert!(steps <= READING_STEPS, "round {round}: {steps} steps");
    }
    assert_eq!(
        clock, 0,
        "an update at the limit leaves three times the steps"
    );
    // Fewer changes in an update, until the document is at its steps.
    let few = 4;
    let mut taken_updates = 0;
    for round in 0..12 {
        let update = before_the_last(ABOVE, clock, few, CHILDREN);
        if let Ok(taken) = take_update(&doc, &update, &SERVED) {
            assert_eq!(taken.reading_steps, steps_of(&taken.doc), "round {round}");
            doc = taken.doc;
            clock += few;
            taken_updates += 1;
        }
        let steps = steps_of(&doc);
        assert!(steps <= READING_STEPS, "round {round}: {steps} steps");
    }
    assert_eq!(
        taken_updates,
        READING_STEPS / (few * CHILDREN),
        "{clock} changes"
    );
    let stored = state_of(&doc);
    let opened = from_state(&stored, &STORED).expect("opened");
    assert_eq!(opened.reading_steps, steps_of(&doc));
    // What an editor writes is still taken.
    let typed = edited(&doc, |root, txn| {
        root.push_back(txn, XmlElementPrelim::empty("paragraph"));
    });
    take_update(&doc, &typed, &SERVED).expect("a paragraph typed");
}

/// What was taken before the steps of a reading were counted is stored: a document
/// one update at the limit left is opened, with its steps told, so that what holds it
/// makes it again from its body; one that more of them left would take longer to read
/// than a worker has, and is refused before the library reads it, counted no further
/// than past the limit.
#[test]
fn a_stored_document_past_the_steps_it_may_take_is_not_read() {
    let mut doc = typed_in_order(CHILDREN);
    let mut stored = Vec::new();
    let mut changes = 0;
    for _ in 0..3 {
        let most = most_placed(&doc, changes, CHILDREN);
        let update = before_the_last(ABOVE, changes, most, CHILDREN);
        doc = take_update(&doc, &update, &UNCOUNTED)
            .expect("taken before the steps were counted")
            .doc;
        changes += most;
        stored.push((changes, state_of(&doc)));
    }
    let (changes, state) = &stored[0];
    let after_one = from_state(state, &STORED).expect("opened");
    assert!(walks_the_list(after_one.reading_steps, *changes));
    assert!(
        after_one.reading_steps > READING_STEPS && after_one.reading_steps <= STORED_READING_STEPS,
        "{} steps",
        after_one.reading_steps
    );
    for (changes, state) in stored.iter().skip(1) {
        let refused = from_state(state, &STORED).expect_err("past the steps");
        let TakeError::Update(UpdateError::TooManyReadingSteps { steps, limit }) = refused else {
            panic!("{changes} changes: {refused}");
        };
        assert_eq!(limit, STORED_READING_STEPS);
        assert!(steps <= limit + CHILDREN + changes, "{steps} steps counted");
        // The whole of it, counted to the end.
        let steps = reading_steps(state, u64::MAX).expect("a state");
        assert!(walks_the_list(steps, *changes), "{steps} steps");
    }
}

/// Whether `steps` are those of `changes` changes that each walk the list of
/// [`CHILDREN`], and no more than each other besides.
fn walks_the_list(steps: u64, changes: u64) -> bool {
    (changes * (CHILDREN - 1)..=changes * (CHILDREN + changes) + CHILDREN).contains(&steps)
}

/// The widest document a body makes, of as many paragraphs as a body may have, is read
/// in no steps: each child names the next to its right and stands before it. It is
/// opened from its state, and what an editor writes into it is taken, in a few steps.
#[test]
fn the_widest_document_of_a_body_is_read_in_no_steps() {
    let body = body_of(&json!({
        "type": "doc",
        "content": vec![json!({"type": "paragraph"}); BODY_MAX_NODES - 2],
    }));
    let doc = from_body(&body);
    let state = state_of(&doc);
    assert_eq!(reading(&state).expect("a whole state").0, 0);
    let opened = from_state(&state, &SERVED).expect("opened");
    assert_eq!(opened.reading_steps, 0);
    let typed = edited(&doc, |root, txn| {
        root.insert(txn, 25_000, XmlElementPrelim::empty("paragraph"));
    });
    let taken = take_update(&doc, &typed, &SERVED).expect("a paragraph typed");
    assert!(
        taken.reading_steps * 64 < READING_STEPS,
        "{} steps",
        taken.reading_steps
    );
}

/// A state that is no whole document is not counted: changes that name what the state
/// does not hold, which the library would leave waiting.
#[test]
fn a_state_that_names_what_it_does_not_hold_is_not_counted() {
    let state = Update::of(
        TYPIST,
        0,
        vec![Change::after((ABOVE, 3), Content::element("paragraph"))],
    )
    .bytes();
    assert_eq!(
        reading(&state),
        Err("it leaves a gap in a client's changes")
    );
    assert!(reading_steps(&state, 0).is_err());
}

/// A document that editors open one after the other, each writing a paragraph after
/// what the one before wrote, is read in few steps: each takes a client of its own,
/// whose changes wait for the client's before as the library reads the state, and the
/// library looks through those that wait.
#[test]
fn what_editors_write_one_session_after_the_other_is_read_in_few_steps() {
    const SESSIONS: u64 = 400;
    let mut doc = from_body(&body_of(&json!({
        "type": "doc",
        "content": [{"type": "paragraph", "content": [{"type": "text", "text": "first"}]}],
    })));
    let mut steps = 0;
    for session in 0..SESSIONS {
        let typed = edited(&doc, |root, txn| {
            let paragraph = root.push_back(txn, XmlElementPrelim::empty("paragraph"));
            paragraph.push_back(txn, XmlTextPrelim::new("typed"));
        });
        let taken = match take_update(&doc, &typed, &SERVED) {
            Ok(taken) => taken,
            Err(error) => panic!("session {session}: {error}"),
        };
        steps = taken.reading_steps;
        doc = taken.doc;
    }
    assert!(
        steps * 64 < READING_STEPS,
        "{steps} steps after {SESSIONS} sessions"
    );
    println!("{steps} steps after {SESSIONS} sessions");
}
