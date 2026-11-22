//! Mending the grid of a table in a Y document that the allowlist refuses: what the
//! server does alone.
//!
//! Two edits that are each valid can merge into a table that does not fill its grid:
//! one adds a column while another adds a row, which has a cell fewer than the rest; one
//! deletes a row that a cell above reached into. Deleting cannot mend that without
//! losing what people wrote, so the server writes: a span is narrowed to what fits, a
//! row is given the empty cells it lacks at its end, and a cell that fails (one left
//! without content, which every editor deletes) is deleted and an empty one is put in
//! its place, so that the cells after it stay in their columns. Writing is not the same
//! whoever does it, as deleting is: every editor that wrote the same would add cells of
//! its own. So the server mends the one it holds, and its editors are sent what it
//! wrote. (The editor's table plugin pads an unfilled table after any change, another
//! editor's too, so two editors that see one would each pad it.)

use fvoci_model::{BodyError, CellFit, TableGrid};
use serde_json::{Map, Value, json};
use yrs::{Any, Number, TransactionMut, Xml, XmlElementRef, XmlFragment};

use crate::build;

/// Where a cell of a row that stays comes from.
#[derive(Debug)]
pub(crate) enum CellAt {
    /// The element the row holds.
    Kept(XmlElementRef),
    /// An empty cell put where one that fails was.
    Put,
}

/// A row of a table that stays, with where each of its cells comes from, in order.
#[derive(Debug)]
pub(crate) struct SurveyedRow {
    pub(crate) element: XmlElementRef,
    pub(crate) cells: Vec<CellAt>,
}

/// One change the server writes to mend a table, made after the deletions.
#[derive(Debug)]
pub(crate) enum Mend {
    /// An empty cell put among the children of a row at `index`, which is its place
    /// among them once the deletions are made and the cells before it are put.
    Put {
        row: XmlElementRef,
        index: u32,
        cell: Value,
    },
    /// Empty cells put at the end of a row.
    Pad {
        row: XmlElementRef,
        cells: Vec<Value>,
    },
    /// The spans of a cell written as they fit; `None` for a span that is left as it is.
    Spans {
        cell: XmlElementRef,
        columns: Option<u16>,
        rows: Option<u16>,
    },
}

impl Mend {
    /// How many cells it puts or writes the spans of.
    pub(crate) fn cells(&self) -> usize {
        match self {
            Self::Put { .. } | Self::Spans { .. } => 1,
            Self::Pad { cells, .. } => cells.len(),
        }
    }

    /// Makes the change.
    pub(crate) fn make(&self, txn: &mut TransactionMut<'_>) {
        match self {
            Self::Put { row, index, cell } => {
                // The index was counted in this row as it is now; inserting past its end
                // would panic in the library.
                let at = (*index).min(row.len(txn));
                build::insert_node(txn, row, at, cell);
            }
            Self::Pad { row, cells } => {
                for cell in cells {
                    let end = row.len(txn);
                    build::insert_node(txn, row, end, cell);
                }
            }
            Self::Spans {
                cell,
                columns,
                rows,
            } => {
                for (name, span) in [("colspan", columns), ("rowspan", rows)] {
                    if let Some(span) = span {
                        cell.insert_attribute(
                            txn,
                            name,
                            Any::Number(Number::Int(i64::from(*span))),
                        );
                    }
                }
            }
        }
    }
}

/// The JSON of an empty cell of a kind: one empty paragraph.
pub(crate) fn empty_cell(kind: &str) -> Value {
    json!({"type": kind, "content": [{"type": "paragraph"}]})
}

/// Mends the grid of a table whose rows each stay: the JSON of the table as its grid
/// mends it, and the changes that make the shared content so. `rows` are the rows of
/// the table's content, in order, each with where its cells come from.
///
/// # Errors
///
/// Returns [`BodyError::TableTooLarge`], with an empty path, for a table past a limit,
/// which no mending brings under it.
pub(crate) fn mend_grid(
    mut table: Value,
    rows: &[SurveyedRow],
) -> Result<(Value, Vec<Mend>), BodyError> {
    let grid = TableGrid::of(&table).map_err(|_| BodyError::TableTooLarge {
        path: String::new(),
    })?;
    let mut mends = Vec::new();
    let nodes = table
        .get_mut("content")
        .and_then(Value::as_array_mut)
        .map_or(&mut [][..], Vec::as_mut_slice);
    debug_assert_eq!(grid.rows().len(), rows.len(), "a row surveyed for each row");
    debug_assert_eq!(grid.rows().len(), nodes.len(), "a node for each row");
    for ((fit, surveyed), node) in grid.rows().iter().zip(rows).zip(nodes) {
        let cells = content_mut(node);
        debug_assert_eq!(
            fit.cells.len(),
            surveyed.cells.len(),
            "a cell surveyed for each"
        );
        debug_assert_eq!(fit.cells.len(), cells.len(), "a node for each cell");
        for (index, ((cell, at), json)) in fit
            .cells
            .iter()
            .zip(&surveyed.cells)
            .zip(cells.iter_mut())
            .enumerate()
        {
            write_spans(json, *cell);
            match at {
                CellAt::Put => mends.push(Mend::Put {
                    row: surveyed.element.clone(),
                    index: u32::try_from(index).unwrap_or(u32::MAX),
                    cell: json.clone(),
                }),
                CellAt::Kept(element) if cell.columns.changed || cell.rows.changed => {
                    mends.push(Mend::Spans {
                        cell: element.clone(),
                        columns: cell.columns.changed.then_some(cell.columns.span),
                        rows: cell.rows.changed.then_some(cell.rows.span),
                    });
                }
                CellAt::Kept(_) => {}
            }
        }
        if fit.missing > 0 {
            // A row of headers is given headers, a row of cells cells.
            let kind = cells
                .last()
                .and_then(|last| last.get("type"))
                .and_then(Value::as_str)
                .unwrap_or("tableCell")
                .to_owned();
            let pad = vec![empty_cell(&kind); fit.missing];
            cells.extend(pad.iter().cloned());
            mends.push(Mend::Pad {
                row: surveyed.element.clone(),
                cells: pad,
            });
        }
    }
    Ok((table, mends))
}

/// The children of a node, made a list if it has none.
fn content_mut(node: &mut Value) -> &mut Vec<Value> {
    let members = node
        .as_object_mut()
        .expect("a row read from shared content is an object");
    let content = members
        .entry("content")
        .or_insert_with(|| Value::Array(Vec::new()));
    if !content.is_array() {
        *content = Value::Array(Vec::new());
    }
    content.as_array_mut().expect("made a list just now")
}

/// Writes into the JSON of a cell the spans that changed, in the stored form: a span of
/// one is left out.
fn write_spans(cell: &mut Value, fit: CellFit) {
    let Some(members) = cell.as_object_mut() else {
        return;
    };
    let mut attrs = match members.remove("attrs") {
        Some(Value::Object(attrs)) => attrs,
        _ => Map::new(),
    };
    for (name, reach) in [("colspan", fit.columns), ("rowspan", fit.rows)] {
        if !reach.changed {
            continue;
        }
        if reach.span == 1 {
            attrs.remove(name);
        } else {
            attrs.insert(name.to_owned(), Value::from(reach.span));
        }
    }
    if !attrs.is_empty() {
        members.insert("attrs".to_owned(), Value::Object(attrs));
    }
}
