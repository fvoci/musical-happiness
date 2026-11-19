//! How far the Y library may have to walk through a document to place the changes of
//! an update, counted before it is asked to.
//!
//! The library (yrs 0.28.1) places a change by the neighbours it names. Where the
//! change to its left is not followed by the change to its right, it walks on from
//! the left one: past every change that names the same left neighbour, and past every
//! change that names one it has walked past, until it meets the right one or a change
//! that is neither. Every change it walks past stands in the same list as the change
//! it places (the children of one node, the letters of one text, the values one
//! attribute had), so the walk is no longer than that list, with what was deleted
//! from it, which a document keeps. An editor's change is placed in a step or two; an
//! update made for it has each of its changes walk a whole list (5,400 changes of 59
//! kB walked the 50,000 children of one node for twelve seconds).
//!
//! So an update is charged, for each of its changes, the most the library may walk to
//! place it, and is refused where that is more than [`UpdateLimits::placing_steps`]
//! together:
//!
//! - A change that alone names a left neighbour of the same update, at the end of the
//!   change it names, is placed in one step: nothing else stands after that
//!   neighbour that the walk goes past. What is typed or pasted is one change placed
//!   in the document and the rest after it, each so.
//! - Every other change is charged the list it goes into: what the document holds of
//!   it before the update, the changes of the update that go into it, and the pieces
//!   the neighbours they name may cut its changes into.
//!
//! The lists are read from the document's state, which names for each change its
//! neighbours and, where it names none, its parent: a change belongs to the list of
//! the neighbour it names. Reading them is a walk of the whole state, so it is left out
//! where every change that is charged a list, charged the whole document, is within
//! the limit already: an editor's update, which has a handful.
//!
//! [`UpdateLimits::placing_steps`]: crate::UpdateLimits::placing_steps

use std::collections::HashMap;

use yrs::{ClientID, ID, IdSet, StateVector};

use crate::scan::{self, Change, Content, Id, Parent, Refusal};

/// A document as its state, with what the library read of it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Held<'bytes> {
    /// The state, which the walk has taken ([`scan::check`]).
    pub(crate) state: &'bytes [u8],
    /// How many changes the walk found in it ([`scan::Walked::changes`]).
    pub(crate) changes: u64,
    /// How far the library read each client's changes in it.
    pub(crate) ends: &'bytes StateVector,
}

/// An update, with what the library read of it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Sent<'bytes> {
    /// The update, which the walk has taken.
    pub(crate) update: &'bytes [u8],
    /// The places the library read its changes to cover.
    pub(crate) covered: &'bytes IdSet,
}

/// What the changes of an update are charged for a document: no less than the changes
/// the library walks past to place them. An update charged more than `limit` is
/// charged no more exactly than that it is past it.
///
/// # Errors
///
/// Returns [`Refusal::Lengths`] if the update, or the state, is not read here as long
/// as the library read it: the neighbours a change names would be found in another
/// place here than there.
pub(crate) fn charge(held: &Held<'_>, sent: &Sent<'_>, limit: u64) -> Result<u64, Refusal> {
    let update = Placed::read(sent, held.ends)?;
    let at_most = update.at_most(held.changes);
    if at_most <= limit {
        return Ok(at_most);
    }
    let clocks = Clocks::read(
        held.state,
        |_, _| {},
        |run| u64::from(held.ends.get(&ClientID::new(run.client))) == run.end,
    )?;
    Ok(Lists::of(held.state, &clocks, &update)?.charge())
}

fn count(number: usize) -> u64 {
    u64::try_from(number).unwrap_or(u64::MAX)
}

/// The changes of one client in a state or an update: the clocks they cover, and
/// which they are among all the changes.
#[derive(Debug, Clone, Copy)]
struct Run {
    client: u64,
    begin: u64,
    end: u64,
    /// The numbers of its first change and of the change after its last.
    first: usize,
    after: usize,
}

/// Where the changes of a state or an update begin, in the order they are written: a
/// change is found by an identifier of any place it covers.
#[derive(Debug)]
struct Clocks {
    /// By client. A state or an update has one run a client.
    runs: Vec<Run>,
    begins: Vec<u64>,
    /// Whether each is an item: not a stretch that was collected.
    items: Vec<bool>,
}

impl Clocks {
    /// Reads where the changes of a state or an update begin, and hands `each` every
    /// one. `as_the_library` says of each client's run whether the library read it as
    /// far.
    fn read<'bytes>(
        bytes: &'bytes [u8],
        mut each: impl FnMut(Id, &Change<'bytes>),
        as_the_library: impl Fn(&Run) -> bool,
    ) -> Result<Self, Refusal> {
        let mut clocks = Self {
            runs: Vec::new(),
            begins: Vec::new(),
            items: Vec::new(),
        };
        scan::walk(bytes, |(client, clock), change| {
            let end = clock.saturating_add(change.content.length());
            let number = clocks.begins.len();
            match clocks.runs.last_mut() {
                Some(run) if run.client == client => {
                    run.end = end;
                    run.after = number + 1;
                }
                _ => clocks.runs.push(Run {
                    client,
                    begin: clock,
                    end,
                    first: number,
                    after: number + 1,
                }),
            }
            clocks.begins.push(clock);
            clocks.items.push(is_item(change));
            each((client, clock), change);
        })?;
        // A length counted otherwise than the library counts it would find every later
        // change of that client under the wrong identifiers.
        if !clocks.runs.iter().all(as_the_library) {
            return Err(Refusal::Lengths);
        }
        clocks.runs.sort_unstable_by_key(|run| run.client);
        Ok(clocks)
    }

    /// The change that covers the place of an identifier, by its number among these.
    fn block_of(&self, (client, clock): Id) -> Option<usize> {
        let run = self
            .runs
            .binary_search_by_key(&client, |run| run.client)
            .ok()
            .map(|at| self.runs[at])?;
        if !(run.begin..run.end).contains(&clock) {
            return None;
        }
        // The last change of the run that begins at or before the clock; changes of no
        // length before it begin there as well and cover nothing.
        let within = self.begins[run.first..run.after].partition_point(|begin| *begin <= clock);
        within.checked_sub(1).map(|at| run.first + at)
    }
}

fn is_item(change: &Change<'_>) -> bool {
    !matches!(change.content, Content::Collected(_))
}

/// How the library places a change of an update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    /// Not at all: a stretch that was collected, or what the document has already.
    Left,
    /// In one step, after a change of the same update that nothing else follows.
    Follows,
    /// By a walk through its list.
    Walks,
}

/// The changes of an update, with how the library places each.
#[derive(Debug)]
struct Placed<'bytes> {
    clocks: Clocks,
    changes: Vec<Change<'bytes>>,
    /// The client of each change.
    clients: Vec<u64>,
    standings: Vec<Standing>,
    /// How far the document has each client's changes: what an update says again
    /// from before there, the library leaves out.
    known: &'bytes StateVector,
}

impl<'bytes> Placed<'bytes> {
    fn read(sent: &Sent<'bytes>, known: &'bytes StateVector) -> Result<Self, Refusal> {
        let covered = |client: u64, clock: u64| {
            u32::try_from(clock).is_ok_and(|clock| {
                sent.covered
                    .contains(&ID::new(ClientID::new(client), clock))
            })
        };
        let mut changes = Vec::new();
        let mut clients = Vec::new();
        let clocks = Clocks::read(
            sent.update,
            |(client, _), change| {
                changes.push(*change);
                clients.push(client);
            },
            |run| {
                !covered(run.client, run.end)
                    && (run.end == run.begin || covered(run.client, run.end - 1))
            },
        )?;
        let mut placed = Self {
            clocks,
            changes,
            clients,
            standings: Vec::new(),
            known,
        };
        placed.standings = placed.standings();
        Ok(placed)
    }

    fn known(&self, client: u64) -> u64 {
        u64::from(self.known.get(&ClientID::new(client)))
    }

    /// The change of this update that covers a place the document has not, by its
    /// number.
    fn new_item_of(&self, id: Id) -> Option<usize> {
        let at = self.clocks.block_of(id)?;
        (self.clocks.items[at] && id.1 >= self.known(id.0)).then_some(at)
    }

    /// The clock after the last place of a change.
    fn end_of(&self, at: usize) -> u64 {
        self.clocks.begins[at].saturating_add(self.changes[at].content.length())
    }

    fn standings(&self) -> Vec<Standing> {
        let comes = |at: usize| {
            let known = self.known(self.clients[at]);
            self.clocks.items[at] && !(self.clocks.begins[at] < known && self.end_of(at) <= known)
        };
        // How many changes that come name each place of the update to their left.
        let mut followers: HashMap<Id, u32> = HashMap::new();
        for (at, change) in self.changes.iter().enumerate() {
            if let Some(left) = change.left.filter(|_| comes(at)) {
                *followers.entry(left).or_insert(0) += 1;
            }
        }
        (0..self.changes.len())
            .map(|at| {
                if !comes(at) {
                    return Standing::Left;
                }
                let alone_after_a_whole_change = self.changes[at].left.is_some_and(|left| {
                    followers.get(&left) == Some(&1)
                        && self
                            .new_item_of(left)
                            .is_some_and(|named| self.end_of(named) == left.1 + 1)
                });
                if alone_after_a_whole_change {
                    Standing::Follows
                } else {
                    Standing::Walks
                }
            })
            .collect()
    }

    fn standing(&self, standing: Standing) -> u64 {
        count(
            self.standings
                .iter()
                .filter(|other| **other == standing)
                .count(),
        )
    }

    /// What the update is charged at most, whatever lists its changes go into: each
    /// that walks, the whole document and three times the update (its changes, and
    /// the two pieces more each may cut a change into).
    fn at_most(&self, held_changes: u64) -> u64 {
        let walks = self.standing(Standing::Walks);
        let follows = self.standing(Standing::Follows);
        let coming = walks.saturating_add(follows);
        let list = held_changes.saturating_add(coming.saturating_mul(3));
        walks.saturating_mul(list).saturating_add(follows)
    }
}

/// Where a change belongs, as far as is known of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// In the list of another change, by its number: the neighbour it names.
    With(usize),
    /// In a list, by its number.
    In(usize),
    /// In none: it was collected, or names a neighbour that is no item.
    Nowhere,
}

/// How much of a list there is.
#[derive(Debug, Clone, Copy, Default)]
struct Size {
    /// The changes of the document in it.
    held: u64,
    /// The changes of the update that go into it.
    coming: u64,
    /// The neighbours in it that changes of the update name: each may cut a change
    /// of it in two.
    cuts: u64,
}

/// The lists of a document and of an update for it: the changes of the document come
/// first, by their numbers, and those of the update after them.
#[derive(Debug)]
struct Lists<'placed, 'bytes> {
    held: &'placed Clocks,
    sent: &'placed Placed<'bytes>,
    places: Vec<Place>,
    sizes: Vec<Size>,
}

/// The lists by what their first changes name: a parent, and a member of it for the
/// values of an attribute.
type Named<'bytes> = HashMap<(Parent<'bytes>, Option<&'bytes [u8]>), usize>;

impl<'placed, 'bytes> Lists<'placed, 'bytes> {
    fn of(
        state: &'bytes [u8],
        held: &'placed Clocks,
        sent: &'placed Placed<'bytes>,
    ) -> Result<Self, Refusal> {
        let mut lists = Self {
            held,
            sent,
            places: Vec::with_capacity(held.begins.len() + sent.changes.len()),
            sizes: Vec::new(),
        };
        let mut named = Named::new();
        scan::walk(state, |_, change| {
            let place = lists.named_place(change, &mut named);
            lists.places.push(place);
        })?;
        for (change, standing) in sent.changes.iter().zip(&sent.standings) {
            let place = match standing {
                Standing::Left => Place::Nowhere,
                Standing::Follows | Standing::Walks => lists.named_place(change, &mut named),
            };
            lists.places.push(place);
        }
        lists.sizes = vec![Size::default(); named.len()];
        let held = held.begins.len();
        for number in 0..held {
            if let Some(list) = lists.list_of(number) {
                lists.sizes[list].held += 1;
            }
        }
        for (at, change) in sent.changes.iter().enumerate() {
            if sent.standings[at] == Standing::Left {
                continue;
            }
            if let Some(list) = lists.list_of(held + at) {
                lists.sizes[list].coming += 1;
            }
            for neighbour in [change.left, change.right].into_iter().flatten() {
                let list = lists
                    .item_of(neighbour)
                    .and_then(|number| lists.list_of(number));
                if let Some(list) = list {
                    lists.sizes[list].cuts += 1;
                }
            }
        }
        Ok(lists)
    }

    /// The number of the item that covers the place of an identifier, in the document
    /// or among the changes of the update that come.
    fn item_of(&self, id: Id) -> Option<usize> {
        match self.held.block_of(id) {
            Some(at) => self.held.items[at].then_some(at),
            None => Some(self.held.begins.len() + self.sent.new_item_of(id)?),
        }
    }

    /// Where a change says it belongs: with the neighbour it names, the left one
    /// before the right one as the library takes its parent from them, or in the list
    /// of the parent it names.
    fn named_place(&self, change: &Change<'bytes>, named: &mut Named<'bytes>) -> Place {
        if !is_item(change) {
            return Place::Nowhere;
        }
        let neighbour = [change.left, change.right]
            .into_iter()
            .flatten()
            .find_map(|id| self.item_of(id));
        match (neighbour, change.parent) {
            (Some(number), _) => Place::With(number),
            (None, Some(parent)) => {
                let lists = named.len();
                Place::In(*named.entry((parent, change.member)).or_insert(lists))
            }
            (None, None) => Place::Nowhere,
        }
    }

    /// The list a change is in, found through the neighbours it names, and noted for
    /// every change on the way.
    fn list_of(&mut self, number: usize) -> Option<usize> {
        let mut passed = Vec::new();
        let mut at = number;
        let list = loop {
            match self.places.get(at).copied() {
                Some(Place::In(list)) => break Some(list),
                Some(Place::Nowhere) | None => break None,
                Some(Place::With(neighbour)) => {
                    // In no list until its list is found: changes of an update that
                    // name each other through their neighbours, which the library
                    // places nowhere, are met again here and are in none.
                    self.places[at] = Place::Nowhere;
                    passed.push(at);
                    at = neighbour;
                }
            }
        };
        let found = list.map_or(Place::Nowhere, Place::In);
        for number in passed {
            self.places[number] = found;
        }
        list
    }

    /// What the changes of the update are charged: one step each that follows a
    /// change of its own update alone, and each other the list it goes into.
    fn charge(mut self) -> u64 {
        let held = self.held.begins.len();
        let sent = self.sent;
        let mut charged = 0_u64;
        for (at, standing) in sent.standings.iter().enumerate() {
            let steps = match standing {
                Standing::Left => 0,
                Standing::Follows => 1,
                Standing::Walks => self.list_of(held + at).map_or(0, |list| {
                    let size = self.sizes[list];
                    size.held
                        .saturating_add(size.coming)
                        .saturating_add(size.cuts)
                }),
            };
            charged = charged.saturating_add(steps);
        }
        charged
    }
}
